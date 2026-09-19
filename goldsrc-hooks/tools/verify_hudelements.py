#!/usr/bin/env python3
"""Checks `hudelement.rs`'s element table against a real DoD `client.dll`.

`hudelement.rs` is the one module in this DLL built on **fixed RVAs** rather
than on a signature scan, because a vftable has no code to sign. What replaces
the signature is MSVC RTTI: every entry names the class its vftable is supposed
to belong to, and the DLL refuses to write anything until `vftable[-1]` leads to
that name. This script is the offline half of the same argument.

Every constant below is read out of the Rust rather than restated.

  1. Each declared vftable's RTTI complete object locator leads to the class
     name the table claims.
  2. Each declared `Draw` (slot 3) points into the executable section.
  3. Each declared element genuinely *overrides* `Draw` -- one whose slot 3 is
     already `CHudBase::Draw` would be listed as hideable while doing nothing.
  4. The donor the DLL takes `CHudBase::Draw`'s address from does not override
     `Draw`, and that address decodes to `xor eax, eax; ret 4`.
  5. No vftable is listed twice, and none is the donor's.
  6. **Completeness**: every class in the image whose `Init` registers it with
     `CHud::AddHudElem` and which overrides `Draw` is in the table, or named in
     `KNOWN_EXCLUDED` with a reason. This is the check that catches an element
     nobody thought of, rather than one that is wrong.

Usage:
    python goldsrc-hooks/tools/verify_hudelements.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install. Requires `pefile` and
`capstone` (`pip install pefile capstone`); both are analysis-only and are not
build dependencies of anything in the workspace.
"""

import collections
import re
import struct
import sys
from pathlib import Path

try:
    import pefile
    import capstone
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

DEFAULT_DLL = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\dod\cl_dlls\client.dll"
)
SRC = Path(__file__).resolve().parent.parent / "src" / "hudelement.rs"

# `CHud::AddHudElem`, from docs/goldsrc_client_dll_survey.md section 1. Used
# only for the completeness check, and confirmed below by the number of
# elements that reach it.
ADD_HUD_ELEM = 0x22330

BASE_DRAW_CODE = b"\x33\xc0\xc2\x04\x00"

# Classes that override `Draw` and register themselves -- so the completeness
# check below would otherwise flag them as a missed element -- but are left
# out of `hudelement.rs` on purpose. See that file's "What is not listed"
# section for why each one is here.
KNOWN_EXCLUDED = {
    # Every draw call in CHudAmmo::Draw lands after one of its own
    # CHud::ShouldDraw(3) calls; the stock cl_hud_ammo cvar already hides all
    # of it, and (unlike crosshair) CHud::Redraw does not force it back.
    ".?AVCHudAmmo@@",
    # Draw is `mov eax, 1; ret 4` -- eight bytes, no calls. Draws nothing,
    # hook or no hook. The overview map renders some other way (not found).
    ".?AVCHudDoDMap@@",
    # Draw calls one gHUD helper that checks a flag and an observer sub-mode
    # and returns a plain boolean. No FillRGBA/SPR_Draw call in either
    # function. There is no mortar aiming HUD in this build.
    ".?AVCMortarHud@@",
    # Draw is 45 bytes ending in a real `ret 4` immediately followed (no
    # padding) by an unrelated function capstone's linear scan would
    # otherwise fold in. It checks observer mode and conditionally calls a
    # method on what looks like a VGUI2 interface pointer -- never draws.
    ".?AVCHudSpectator@@",
}


def rust_elements(src: str):
    """Every `Element { ... }` literal, as (name, class, vftable_rva)."""
    out = []
    for m in re.finditer(r"Element\s*\{(.*?)\}", src, re.S):
        body = m.group(1)
        name = re.search(r'name:\s*"([^"]*)"', body)
        klass = re.search(r'class:\s*"([^"]*)"', body)
        rva = re.search(r"vftable_rva:\s*(0x[0-9a-fA-F]+)", body)
        if not (name and klass and rva):
            continue
        out.append((name.group(1), klass.group(1), int(rva.group(1), 16)))
    return out


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    if not dll.is_file():
        sys.exit(f"no client.dll at {dll}")

    src = SRC.read_text(encoding="utf-8")
    entries = rust_elements(src)
    if not entries:
        sys.exit("could not parse any Element literals out of hudelement.rs")
    # `BASE_DRAW_DONOR` is declared before `pub const ELEMENTS`, so position in
    # the file is what tells the donor apart from the table.
    elements_at = src.index("pub const ELEMENTS")
    donor = [e for e in entries if src.index(f"vftable_rva: {hex(e[2])}") < elements_at]
    table = [e for e in entries if src.index(f"vftable_rva: {hex(e[2])}") > elements_at]
    if len(donor) != 1:
        sys.exit(f"expected exactly one BASE_DRAW_DONOR, parsed {len(donor)}")
    donor = donor[0]

    slot = int(re.search(r"const DRAW_SLOT:\s*usize\s*=\s*(\d+)", src).group(1))

    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    text = pe.sections[0]
    text_lo, text_hi = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    failures = []

    def check(ok, message):
        print(("  ok   " if ok else "  FAIL ") + message)
        if not ok:
            failures.append(message)

    def u32(rva):
        return struct.unpack_from("<I", img, rva)[0]

    def cstr(rva):
        return img[rva : img.index(b"\x00", rva)].decode("latin-1")

    def class_of(vftable_rva):
        """The decorated name vftable[-1]'s RTTI leads to, or None."""
        try:
            col = u32(vftable_rva - 4) - base
            if not 0 <= col < len(img) or u32(col) != 0:
                return None
            td = u32(col + 0x0C) - base
            if not 0 <= td < len(img):
                return None
            return cstr(td + 8)
        except (struct.error, ValueError):
            return None

    print(f"client.dll at {dll}")
    print(f"image base {base:#x}, {len(table)} elements declared, donor {donor[1]}\n")

    # 4. the donor
    donor_class = class_of(donor[2])
    check(donor_class == donor[1], f"donor +{donor[2]:#x} is {donor_class}")
    base_draw = u32(donor[2] + slot * 4) - base
    check(
        img[base_draw : base_draw + len(BASE_DRAW_CODE)] == BASE_DRAW_CODE,
        f"CHudBase::Draw at +{base_draw:#x} is `xor eax, eax; ret 4`",
    )

    # 1 + 2 + 3 + 5
    seen = set()
    for name, klass, rva in table:
        found = class_of(rva)
        check(found == klass, f"{name:<12} +{rva:#07x} is {found}")
        draw = u32(rva + slot * 4) - base
        check(text_lo <= draw < text_hi, f"{name:<12} Draw +{draw:#07x} is in .text")
        check(draw != base_draw, f"{name:<12} overrides Draw")
        check(rva not in seen, f"{name:<12} vftable is listed once")
        seen.add(rva)
    check(donor[2] not in seen, "the donor is not also offered as hideable")

    # 6. completeness
    calls = [
        m.start()
        for m in re.finditer(rb"\xe8(....)", img[text_lo:text_hi], re.S)
        if m.start() + 5 + struct.unpack("<i", m.group(1))[0] + text_lo == ADD_HUD_ELEM
    ]
    check(len(calls) > 0, f"CHud::AddHudElem at +{ADD_HUD_ELEM:#x} has {len(calls)} caller(s)")

    # Every class in the image, by walking type descriptors back to vftables.
    positions = collections.defaultdict(list)
    for off in range(0, len(img) - 4, 4):
        positions[struct.unpack_from("<I", img, off)[0]].append(off)

    registering = {}
    for m in re.finditer(rb"\.\?AV[\x20-\x7e]{1,120}@@\x00", img):
        td = m.start() - 8
        name = m.group(0)[:-1].decode("latin-1")
        for col in positions.get(base + td, []):
            col -= 0x0C
            if col < 0 or u32(col) != 0:
                continue
            for vft in positions.get(base + col, []):
                vft += 4
                init = u32(vft + 4) - base if vft + 8 <= len(img) else 0
                if not text_lo <= init < text_hi:
                    continue
                # Does this class' Init reach AddHudElem before its first ret?
                window = img[init : init + 0x400]
                found = False
                for ins in md.disasm(bytes(window), base + init):
                    if ins.mnemonic == "ret":
                        break
                    if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
                        if int(ins.op_str, 16) - base == ADD_HUD_ELEM:
                            found = True
                            break
                if found:
                    registering[name] = vft

    overriding = {n: v for n, v in registering.items() if u32(v + slot * 4) - base != base_draw}
    declared = {klass for _, klass, _ in table}
    missing = sorted(set(overriding) - declared - KNOWN_EXCLUDED)
    check(
        not missing,
        f"every drawing element that registers itself is in the table or KNOWN_EXCLUDED"
        + (f" -- missing {missing}" if missing else f" ({len(overriding)} found, {len(KNOWN_EXCLUDED)} excluded)"),
    )
    extra = sorted(declared - set(registering))
    check(not extra, "every declared element registers itself" + (f" -- {extra}" if extra else ""))
    stale_exclusions = sorted(KNOWN_EXCLUDED - set(overriding))
    check(
        not stale_exclusions,
        "every KNOWN_EXCLUDED class still overrides Draw and registers itself"
        + (f" -- {stale_exclusions} no longer do(es)" if stale_exclusions else ""),
    )

    print()
    if failures:
        print(f"{len(failures)} check(s) FAILED")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
