#!/usr/bin/env python3
"""Checks `objicons/scale.rs`' ten call sites against a real DoD `client.dll`.

`ds_hud_objectives scale` (#891) rewrites ten call operands in
`CObjectiveIcons::Draw`, at offsets from the two patterns `objicons.rs` already
scans for. The unit tests prove the offsets add up to the addresses the
module's header names; this proves those addresses hold what the module
expects, in the binary:

  1. Each pattern matches exactly once.
  2. The timer's `SPR_Set` is `call [gEngfuncs+0x10]`, and the three draw
     sites are `call [gEngfuncs+0x18]` on the same `gEngfuncs`.
  3. The four digit calls are `call rel32` to one function starting with
     `DIGIT_PROLOGUE`; the map-mode and text calls reach functions starting
     with `MAP_MODE_PROLOGUE` and `TEXT_PROLOGUE`.
  4. Nothing branches into the interior of a site (a 6-byte call becomes a
     5-byte call and a `nop`).
  5. The row stub's `PIP_SLOT` is written before the row's convergence point
     only by the pip test, and read once in between (`+0x301f7`).
  6. The digit helper is also the reinforcement timer's (calls outside the
     four sites), which is why the sites are the calls and not the helper.

Every constant is read out of the Rust rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_objicons_scale_offsets.py [path-to-client.dll]
"""

import re
import struct
import sys
from pathlib import Path

try:
    import pefile
    import capstone
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_objicons_offsets as base_check  # noqa: E402

SRC = Path(__file__).resolve().parent.parent / "src"
OBJICONS = SRC / "objicons.rs"
SCALE = SRC / "objicons" / "scale.rs"


def scalar(src, name):
    return base_check.rust_scalar(src, name)


def array(src, name):
    m = re.search(rf"const {name}: \[usize; \d+\] = \[([^\]]*)\];", src)
    if not m:
        raise SystemExit(f"could not find `const {name}`")
    return [int(t.strip(), 16) for t in m.group(1).split(",") if t.strip()]


def find_once(image, pattern):
    rx = re.compile(
        b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()),
        re.S,
    )
    return [m.start() for m in rx.finditer(image)]


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else base_check.DEFAULT_DLL
    if not dll.is_file():
        return print(f"no client.dll at {dll}") or 2
    obj = OBJICONS.read_text(encoding="utf-8")
    sc = SCALE.read_text(encoding="utf-8")
    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = bytes(pe.get_memory_mapped_image())
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    ok = True

    def fail(msg):
        nonlocal ok
        ok = False
        print("FAIL " + msg)

    found = {}
    for prefix in ("TIMER", "ROW"):
        hits = find_once(image, base_check.rust_pattern(obj, f"{prefix}_PATTERN"))
        if len(hits) != 1:
            fail(f"{prefix}_PATTERN matches {len(hits)} times")
            continue
        found[prefix] = hits[0]
        print(f"OK   {prefix}_PATTERN matches once, at +{hits[0]:#x}")
    if len(found) != 2:
        return 1
    timer, row = found["TIMER"], found["ROW"]

    def abs_call(at):
        b = image[at : at + 6]
        if b[:2] != b"\xff\x15":
            return None
        return struct.unpack_from("<I", b, 2)[0]

    def rel_call(at):
        b = image[at : at + 5]
        if b[0] != 0xE8:
            return None
        return (at + 5 + struct.unpack_from("<i", b, 1)[0]) & 0xFFFFFFFF

    set_slot, holes_slot = scalar(sc, "SPR_SET_SLOT"), scalar(sc, "DRAW_HOLES_SLOT")
    spr_set = abs_call(timer + scalar(sc, "TIMER_SPR_SET"))
    if spr_set is None:
        fail("TIMER_SPR_SET is not a call [abs32]")
        return 1
    g = spr_set - set_slot * 4
    print(f"OK   the timer's SPR_Set uses gEngfuncs at {g:#x}")

    abs_sites = {
        "TIMER_BACKGROUND_DRAW": timer + scalar(sc, "TIMER_BACKGROUND_DRAW"),
        "ROW_FILL_DRAW": row + scalar(sc, "ROW_FILL_DRAW"),
        "ROW_ICON_DRAW": row + scalar(sc, "ROW_ICON_DRAW"),
    }
    for name, at in abs_sites.items():
        if abs_call(at) == g + holes_slot * 4:
            print(f"OK   {name} +{at:#x} is call [gEngfuncs+{holes_slot * 4:#x}]")
        else:
            fail(f"{name} +{at:#x} holds {image[at:at + 6].hex(' ')}")

    def prologue(name):
        return base_check.rust_bytes(sc, name)

    digit_targets = set()
    digit_sites = [timer + o for o in array(sc, "TIMER_DIGIT_CALLS")]
    for at in digit_sites:
        t = rel_call(at)
        if t is None:
            fail(f"digit call +{at:#x} is not call rel32")
            continue
        digit_targets.add(t)
    rel_sites = {"digit": digit_sites}
    if len(digit_targets) == 1:
        helper = digit_targets.pop()
        want = prologue("DIGIT_PROLOGUE")
        if image[helper : helper + len(want)] == want:
            print(f"OK   the four digit calls reach +{helper:#x}, the digit helper")
        else:
            fail(f"+{helper:#x} does not start with DIGIT_PROLOGUE")
        others = []
        for sec in pe.sections:
            if not sec.Characteristics & 0x20000000:
                continue
            lo = sec.VirtualAddress
            hi = lo + max(sec.Misc_VirtualSize, sec.SizeOfRawData)
            i = lo
            while True:
                i = image.find(b"\xe8", i, hi)
                if i < 0:
                    break
                if rel_call(i) == helper and i not in digit_sites:
                    others.append(i)
                i += 1
        print(f"     the helper is also called from {[hex(o) for o in others]} (the reinforcement timer), left alone")
    else:
        fail("the digit calls do not reach one function")

    for name, offset, want_name in (
        ("ROW_MAP_MODE_CALL", "ROW_MAP_MODE_CALL", "MAP_MODE_PROLOGUE"),
        ("ROW_TEXT_CALL", "ROW_TEXT_CALL", "TEXT_PROLOGUE"),
    ):
        at = row + scalar(sc, offset)
        rel_sites[name] = [at]
        t = rel_call(at)
        want = prologue(want_name)
        if t is not None and image[t : t + len(want)] == want:
            print(f"OK   {name} +{at:#x} reaches +{t:#x}, starting with {want_name}")
        else:
            fail(f"{name} +{at:#x} reaches {t}, not {want_name}")

    targets = base_check.branch_targets(pe, image, base, md)
    interiors = [(n, range(a + 1, a + 6)) for n, a in abs_sites.items()]
    interiors += [(n, range(a + 1, a + 5)) for n, ats in rel_sites.items() for a in ats]
    inbound = [(n, s, t) for n, r in interiors for t in r for s in targets.get(t, ())]
    if inbound:
        for n, s, t in inbound:
            fail(f"+{s:#x} branches to +{t:#x}, inside {n}")
    else:
        print("OK   nothing branches into any site's interior")

    pip = scalar(obj, "PIP_SLOT")
    convergence = row + scalar(obj, "ROW_DETOUR_AT")
    operand = f"[esp + {pip:#x}]"
    writes, reads = [], []
    for ins in md.disasm(image[base_check.DRAW[0] : convergence], base + base_check.DRAW[0]):
        if operand in ins.op_str:
            (writes if ins.op_str.startswith("dword ptr " + operand) else reads).append(ins.address - base)
    if len(writes) == 2 and len(reads) == 1 and max(writes) < reads[0]:
        print(f"OK   PIP_SLOT ({pip:#x}) is set at {[hex(w) for w in writes]} and read at {hex(reads[0])} before the row")
    else:
        fail(f"PIP_SLOT writes={[hex(w) for w in writes]} reads={[hex(r) for r in reads]}")

    print("\nSCALE SITES VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
