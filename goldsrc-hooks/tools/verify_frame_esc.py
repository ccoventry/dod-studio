#!/usr/bin/env python3
"""Checks `frame_esc.rs` against the real `GameUI.dll`s (issues #369, #408).

`frame_esc.rs` turns the first two bytes of the `CloseFrameButtonPressed`
block in the 25th Anniversary `Frame::OnKeyCodeTyped` into a short jump over
it, so ESC posts only `Cancel`, as on the pre-Anniversary build. The unit
test proves the jump lands where the constants say; this proves what the
patch relies on:

  1. `PATTERN` matches exactly once in the code section, and that function is
     `Frame@vgui2`'s vftable slot 100 (`OnKeyCodeTyped`).
  2. It is `thiscall` with one argument (`this` from `ecx`, every return
     `ret 4`) and compares that argument, `[ebp+8]`, with `KEY_ESCAPE`.
  3. `CLOSE_BLOCK` and `CANCEL_BLOCK` each start an instruction `push 0x18`;
     the close block pushes `"CloseFrameButtonPressed"` at `CLOSE_PUSH`, the
     cancel block pushes `"Command"` and `"Cancel"`; and the cancel block
     follows the close block directly, with nothing between them that the
     skip would drop.
  4. Nothing branches into the two overwritten bytes but their own start.
  5. Which windows it reaches: every GameUI class whose vftable slot 100 is
     that function (listed, by RTTI).
  6. The pre-Anniversary GameUI has no match, so the module refuses there,
     and its own `Frame::OnKeyCodeTyped` posts no `CloseFrameButtonPressed`:
     nothing to fix.

Every constant comes out of `frame_esc.rs` rather than being restated here,
for the reason in `verify_deathmsg_offsets.py`.

Usage:
    python goldsrc-hooks/tools/verify_frame_esc.py [anniversary-GameUI.dll [pre-GameUI.dll]]

Defaults to the two movie installs. Needs `pip install pefile capstone`.
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

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
ANNIVERSARY = STEAM / "Half-Life - POST-Anniversary for Movies" / "valve" / "cl_dlls" / "GameUI.dll"
PRE = STEAM / "Half-Life - PRE-Anniversary for Movies" / "valve" / "cl_dlls" / "GameUI.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "frame_esc.rs"
SLOT = 100


def rust_const(src, name):
    match = re.search(rf"const {name}: [^=]+= (.*?);", src, re.S)
    if not match:
        raise SystemExit(f"could not find `const {name}` in frame_esc.rs")
    return match.group(1).strip()


class Image:
    def __init__(self, path):
        self.pe = pefile.PE(str(path), fast_load=True)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.img = bytes(self.pe.get_memory_mapped_image())
        code = next(s for s in self.pe.sections if s.Characteristics & 0x20000000)
        self.code = (code.VirtualAddress, code.VirtualAddress + code.Misc_VirtualSize)

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

    def rva(self, va):
        return va - self.base

    def string_at(self, va):
        at = self.rva(va)
        if not 0 <= at < len(self.img):
            return None
        return self.img[at:self.img.index(b"\0", at)].decode("ascii", "replace")

    def vftables(self):
        """{primary vftable rva: RTTI class name} for every class."""
        names = {m.start() - 8: m.group()[:-1].decode()
                 for m in re.finditer(rb"\.\?AV[\w@]+@@\0", self.img)}
        by_va = {struct.pack("<I", self.base + td): n for td, n in names.items()}
        found = {}
        for col in range(0, len(self.img) - 20, 4):
            name = by_va.get(self.img[col + 12:col + 16])
            if not name or self.u32(col) != 0 or self.u32(col + 4) != 0:
                continue
            for ref in re.finditer(re.escape(struct.pack("<I", self.base + col)), self.img):
                if ref.start() % 4 == 0:
                    found[ref.start() + 4] = name
        return found

    def matches(self, pattern):
        rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)]))
                                 for t in pattern.split()), re.S)
        lo, hi = self.code
        return [lo + m.start() for m in rx.finditer(self.img[lo:hi])]

    def branch_targets(self, md):
        lo, hi = self.code
        targets = {}
        for ins in md.disasm(self.img[lo:hi], self.base + lo):
            if ins.mnemonic.startswith("j") or ins.mnemonic == "call":
                m = re.fullmatch(r"0x([0-9a-f]+)", ins.op_str)
                if m:
                    targets.setdefault(int(m.group(1), 16) - self.base, []).append(ins.address - self.base)
        return targets


def main():
    anni_path = Path(sys.argv[1]) if len(sys.argv) > 1 else ANNIVERSARY
    pre_path = Path(sys.argv[2]) if len(sys.argv) > 2 else PRE
    src = RUST.read_text(encoding="utf-8")
    key = int(rust_const(src, "KEY_ESCAPE"), 0)
    close_block = int(rust_const(src, "CLOSE_BLOCK"), 0)
    cancel_block = int(rust_const(src, "CANCEL_BLOCK"), 0)
    close_push = int(rust_const(src, "CLOSE_PUSH"), 0)
    pattern = " ".join(rust_const(src, "PATTERN").replace('"', " ").replace("\\", " ").split())
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.skipdata = True
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"== {anni_path} ==")
    img = Image(anni_path)
    found = img.matches(pattern)
    check(len(found) == 1, f"the signature matches exactly once in the code section: {[hex(f) for f in found]}")
    if not found:
        return 1
    fn = found[0]
    tables = img.vftables()
    frame = next((vt for vt, n in tables.items() if n == ".?AVFrame@vgui2@@"), None)
    check(frame and img.rva(img.u32(frame + 4 * SLOT)) == fn,
          f"it is Frame@vgui2's vftable slot {SLOT} (OnKeyCodeTyped), +{fn:#x}")

    body = []
    for ins in md.disasm(img.img[fn:fn + 0x200], img.base + fn):
        body.append((ins.address - img.base, f"{ins.mnemonic} {ins.op_str}".strip()))
        if ins.mnemonic == "int3":
            break
    text = [t for _, t in body]
    at = {a: t for a, t in body}
    check("mov esi, ecx" in text, "`this` comes in ecx (thiscall)")
    rets = [t for t in text if t.startswith("ret")]
    check(rets and all(t == "ret 4" for t in rets), f"every return pops one argument: {rets}")
    check(f"cmp dword ptr [ebp + 8], {key:#x}" in text, f"it compares its argument with KEY_ESCAPE ({key:#x})")

    check(at.get(fn + close_block) == "push 0x18", f"+{close_block:#x} starts an instruction: {at.get(fn + close_block)!r}")
    check(at.get(fn + cancel_block) == "push 0x18", f"+{cancel_block:#x} starts an instruction: {at.get(fn + cancel_block)!r}")
    push = at.get(fn + close_push, "")
    pushed = img.string_at(int(push.split()[1], 16)) if push.startswith("push 0x") else None
    check(pushed == "CloseFrameButtonPressed", f"the close block pushes {pushed!r}")
    close_strings = [img.string_at(int(t.split()[1], 16)) for a, t in body
                     if fn + close_block <= a < fn + cancel_block and re.fullmatch(r"push 0x[0-9a-f]{6,}", t)]
    check(close_strings == ["CloseFrameButtonPressed"],
          f"and nothing else between the two blocks that the skip would drop: {close_strings}")
    cancel_strings = [img.string_at(int(t.split()[1], 16)) for a, t in body
                      if a >= fn + cancel_block and re.fullmatch(r"push 0x[0-9a-f]{6,}", t)]
    check({"Command", "Cancel"} <= set(cancel_strings), f"the cancel block posts Command \"Cancel\": {cancel_strings}")

    targets = img.branch_targets(md)
    into = [hex(src_) for off in (1,) for src_ in targets.get(fn + close_block + off, [])]
    check(not into, f"nothing branches into the second overwritten byte: {into}")
    onto = targets.get(fn + close_block, [])
    print(f"     branches onto the block start (still fine, it now jumps on): {[hex(s) for s in onto]}")

    reached = sorted(n for vt, n in tables.items() if img.rva(img.u32(vt + 4 * SLOT)) == fn)
    print(f"     reaches {len(reached)} classes: " + ", ".join(n[4:-2] for n in reached))
    check(any("CDemoPlayerDialog" in n for n in reached) and any("CGameConsoleDialog" in n for n in reached),
          "including the VCR bar and the console")

    print(f"\n== {pre_path} ==")
    pre = Image(pre_path)
    check(not pre.matches(pattern), "the signature doesn't match here: the module refuses")
    pre_tables = pre.vftables()
    pre_frame = next(vt for vt, n in pre_tables.items() if n == ".?AVFrame@vgui2@@")
    pre_fn = pre.rva(pre.u32(pre_frame + 4 * SLOT))
    close = pre.img.find(b"CloseFrameButtonPressed\0")
    pushes_close = close >= 0 and struct.pack("<BI", 0x68, pre.base + close) in pre.img[pre_fn:pre_fn + 0x100]
    check(not pushes_close, f"its Frame::OnKeyCodeTyped (+{pre_fn:#x}) doesn't post CloseFrameButtonPressed: nothing to fix")

    print("\nVERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
