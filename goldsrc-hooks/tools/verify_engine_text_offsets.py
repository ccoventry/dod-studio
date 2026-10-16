#!/usr/bin/env python3
"""Checks #677's two `hw.dll` call patches, and the `client.dll` facts the
rest of #677's later rows rely on, against real binaries.

`engine_center_print.rs` (the `svc_centerprint` half of
`dodstudio_hide_center_text`), for each `hw.dll`:

  1. Exactly one of its `BUILDS` matches, exactly once.
  2. The match is the svc table's opcode 26 (`svc_centerprint`) handler.
  3. Its first call is `MSG_ReadString`: the same function `svc_print`'s
     handler reads its text with.
  4. The patched call goes to `SCR_CenterPrint`: `strncpy` of 1023 bytes
     (`push 0x3ff`) from its argument, `[ebp+8]`.
  5. `pop ecx; ret` follows, so five `nop`s leave the stack as it was, and
     the replacement in the Rust is five `nop`s.

`server_hud_text.rs` (`dodstudio_hide_server_hud_text`), for each `hw.dll`:

  6. Exactly one of its `BUILDS` matches, exactly once.
  7. The table its first instruction indexes holds the four
     `"__NETMESSAGE__1".."4"` channel names (so it is `TE_TEXTMESSAGE`), and
     the pattern stores the name at `+0x24` and the text at `+0x28`: the
     offsets `engine::ClientTextMessage` declares.
  8. The patched call goes to `CL_HudMessage`, which pushes `"HudText"` and
     calls the by-name dispatcher, and returns with a plain `ret` (cdecl, one
     argument -- what `filtered_hud_message` is).
  9. That dispatcher calls every matching record: after its indirect call it
     goes on to the next record (`+0x18`) instead of returning, which is why
     a `HudText` hook can't drop these.
 10. `add esp, 0x10` follows the call: the caller pops the argument.

Both patched calls:

 11. Nothing branches into the middle of one, and no relocated dword points
     into it.

`client.dll` (one file, identical in every install):

 12. `pfnTextMessageGet` is `gEngfuncs` slot 25: five calls through it, each
     popping one argument, and one reads `[eax+0x28]` (the text) from the
     result.
 13. The `VGUIMenu` and `MOTD` thunks in `msglog.rs`'s table are referenced
     once each (their `pfnHookUserMsg` registration) and never called
     directly, so dropping the messages (`vgui_menus.rs`) is the only way in.

Every pattern and offset is read out of the Rust rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_engine_text_offsets.py [--hw PATH]... [--client PATH]

Defaults to both movie installs' `hw.dll` (pre-Anniversary and 25th
Anniversary) and the pre-Anniversary `client.dll`. Requires `pefile` and
`capstone` (`pip install pefile capstone`).
"""

import re
import struct
import sys
from pathlib import Path

try:
    import capstone
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_HW = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "hw.dll",
]
DEFAULT_CLIENT = STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll"
SRC = Path(__file__).resolve().parent.parent / "src"

SVC_PRINT = 8
SVC_CENTERPRINT = 26


def rust_builds(path: Path):
    """[(name, pattern, at)] from `const BUILDS: [Build; N]` in `path`."""
    src = path.read_text(encoding="utf-8")
    table = re.search(r"const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S)
    if not table:
        sys.exit(f"could not find `const BUILDS` in {path.name}")
    builds = []
    for name, raw, at in re.findall(
        r'name: "([^"]+)",\s*pattern: "(.*?)",\s*at: (\d+)', table.group(1), re.S
    ):
        pattern = " ".join(re.sub(r"\\\s*\n\s*", " ", raw).split())
        builds.append((name, pattern, int(at)))
    if not builds:
        sys.exit(f"`BUILDS` in {path.name} has no entries this script can read")
    return builds


def rust_hidden(path: Path) -> bytes:
    src = path.read_text(encoding="utf-8")
    m = re.search(r"const HIDDEN: \[u8; CALL_LEN\] = \[0x([0-9a-fA-F]{2}); CALL_LEN\];", src)
    if not m:
        sys.exit(f"could not find `const HIDDEN` in {path.name}")
    return bytes([int(m.group(1), 16)] * 5)


def rust_offset(path: Path, field: str) -> int:
    src = path.read_text(encoding="utf-8")
    m = re.search(rf"offset_of!\(ClientTextMessage, {field}\) == (0x[0-9a-fA-F]+)\)", src)
    if not m:
        sys.exit(f"could not find the {field} offset assert in {path.name}")
    return int(m.group(1), 16)


class Image:
    def __init__(self, path: Path):
        self.path = path
        self.pe = pefile.PE(str(path), fast_load=True)
        self.img = bytes(self.pe.get_memory_mapped_image())
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.code = [
            (s.VirtualAddress, s.VirtualAddress + s.Misc_VirtualSize)
            for s in self.pe.sections
            if s.Characteristics & 0x20000000  # IMAGE_SCN_MEM_EXECUTE
        ]
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self._sweep = None
        self._relocs = None

    def find_all(self, pattern: str):
        rx = re.compile(
            b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()),
            re.S,
        )
        out = []
        for lo, hi in self.code:
            out += [m.start() for m in rx.finditer(self.img, lo, hi)]
        return out

    def dis(self, rva, count=40):
        out = []
        for ins in self.md.disasm(self.img[rva : rva + count * 16], self.base + rva):
            out.append(ins)
            if len(out) >= count:
                break
        return out

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

    def cstr(self, va):
        rva = va - self.base
        if not 0 <= rva < len(self.img):
            return None
        end = self.img.find(b"\0", rva, rva + 256)
        return self.img[rva:end] if end >= 0 else None

    def call_target(self, rva):
        """Where the `E8 rel32` at `rva` goes, as an RVA."""
        rel = struct.unpack_from("<i", self.img, rva + 1)[0]
        return rva + 5 + rel

    def string_rva(self, text: bytes):
        i = self.img.find(text + b"\0")
        while i > 0 and self.img[i - 1] != 0:
            i = self.img.find(text + b"\0", i + 1)
        return i if i >= 0 else None

    def svc_handler(self, opcode, name: bytes):
        """The handler in the svc table's record for `opcode`, found through
        the record's name pointer."""
        at = self.string_rva(name)
        if at is None:
            return None
        ptr = struct.pack("<I", self.base + at)
        i = self.img.find(ptr)
        while i >= 0:
            op, _name, func = struct.unpack_from("<III", self.img, i - 4)
            if op == opcode:
                return func - self.base
            i = self.img.find(ptr, i + 1)
        return None

    def sweep(self):
        if self._sweep is None:
            md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
            md.skipdata = True
            self._sweep = []
            for lo, hi in self.code:
                for addr, _size, mnem, op in md.disasm_lite(self.img[lo:hi], self.base + lo):
                    if (mnem == "call" or mnem.startswith("j")) and op.startswith("0x"):
                        self._sweep.append((addr - self.base, mnem, int(op, 16) - self.base))
        return self._sweep

    def branches_into(self, start, end):
        return [(a, m, t) for a, m, t in self.sweep() if start < t < end]

    def relocs_into(self, start, end):
        if self._relocs is None:
            self.pe.parse_data_directories(
                directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_BASERELOC"]]
            )
            self._relocs = []
            for block in getattr(self.pe, "DIRECTORY_ENTRY_BASERELOC", []):
                for entry in block.entries:
                    if entry.type == 3:
                        self._relocs.append((entry.rva, self.u32(entry.rva) - self.base))
        return [(at, t) for at, t in self._relocs if start < t < end]


def one_build(image, builds, check, label):
    """The single matching build's (name, match rva, at), or None."""
    hits = {name: image.find_all(pattern) for name, pattern, _ in builds}
    for name, found in hits.items():
        print(f"     {label} {name} signature: {len(found)} match(es) {[hex(m) for m in found]}")
    matching = [(name, hits[name], at) for name, _pattern, at in builds if hits[name]]
    if not check(len(matching) == 1 and len(matching[0][1]) == 1, f"{label}: exactly one build matches, exactly once"):
        return None
    name, found, at = matching[0]
    return name, found[0], at


def check_center_print(image, check):
    rust = SRC / "engine_center_print.rs"
    found = one_build(image, rust_builds(rust), check, "svc_centerprint")
    if not found:
        return None
    name, start, at = found
    call = start + at
    handler = image.svc_handler(SVC_CENTERPRINT, b"svc_centerprint")
    check(handler == start, f"the match (+{start:#x}) is svc_centerprint's handler (+{handler or 0:#x}) ({name})")
    print_handler = image.svc_handler(SVC_PRINT, b"svc_print")
    read = image.call_target(start) if image.img[start] == 0xE8 else None
    print_read = image.call_target(print_handler) if print_handler is not None and image.img[print_handler] == 0xE8 else None
    check(read is not None and read == print_read, f"its first call (+{read or 0:#x}) is what svc_print reads its text with: MSG_ReadString")
    target = image.call_target(call)
    body = image.dis(target, 6)
    shape = [f"{i.mnemonic} {i.op_str}".strip() for i in body[:5]]
    check(
        shape[:5] == ["push ebp", "mov ebp, esp", "push esi", "mov esi, dword ptr [ebp + 8]", "push 0x3ff"],
        f"the patched call goes to SCR_CenterPrint (+{target:#x}): {shape[:5]}",
    )
    after = image.dis(call + 5, 2)
    check(
        [i.mnemonic for i in after] == ["pop", "ret"] and after[0].op_str == "ecx",
        "pop ecx; ret follows the call",
    )
    hidden = rust_hidden(rust)
    check(hidden == b"\x90" * 5, f"the replacement is five nops: {hidden.hex(' ')}")
    return call


def check_hud_message(image, check):
    rust = SRC / "server_hud_text.rs"
    found = one_build(image, rust_builds(rust), check, "TE_TEXTMESSAGE")
    if not found:
        return None
    name, start, at = found
    call = start + at
    first = image.dis(start, 1)[0]
    m = re.match(r"mov e\w\w, dword ptr \[e\w\w\*4 \+ (0x[0-9a-f]+)\]", first.op_str and f"{first.mnemonic} {first.op_str}")
    names = []
    if m:
        table = int(m.group(1), 16) - image.base
        names = [image.cstr(image.u32(table + 4 * i)) for i in range(4)]
    check(
        names == [f"__NETMESSAGE__{i}".encode() for i in range(1, 5)],
        f"the indexed table holds the four channel names ({name}): {names}",
    )
    stores = [f"{i.mnemonic} {i.op_str}" for i in image.dis(start, 12)]
    p_name = rust_offset(SRC / "engine.rs", "p_name")
    p_message = rust_offset(SRC / "engine.rs", "p_message")
    check(
        any(re.fullmatch(rf"mov dword ptr \[e\w\w \+ {p_name:#x}\], e\w\w", s) for s in stores)
        and any(re.fullmatch(rf"mov dword ptr \[e\w\w \+ {p_message:#x}\], e\w\w", s) for s in stores),
        f"the name is stored at +{p_name:#x} and the text at +{p_message:#x}, as ClientTextMessage says",
    )
    target = image.call_target(call)
    body = []
    for ins in image.dis(target, 24):
        body.append(ins)
        if ins.mnemonic == "ret":
            break
    pushes_hud_text = any(
        i.mnemonic == "push" and i.op_str.startswith("0x") and image.cstr(int(i.op_str, 16)) == b"HudText"
        for i in body
    )
    rets = [i for i in body if i.mnemonic == "ret"]
    check(pushes_hud_text, f"the patched call goes to CL_HudMessage (+{target:#x}), which pushes \"HudText\"")
    check(bool(rets) and rets[0].op_str == "", "CL_HudMessage returns with a plain ret (cdecl)")
    calls = [i for i in body if i.mnemonic == "call" and i.op_str.startswith("0x")]
    dispatcher = int(calls[-1].op_str, 16) - image.base if calls else None
    ok_loop = False
    if dispatcher is not None:
        disp = image.dis(dispatcher, 60)
        indirect = next((k for k, i in enumerate(disp) if i.mnemonic == "call" and not i.op_str.startswith("0x")), None)
        if indirect is not None:
            tail = disp[indirect + 1 : indirect + 6]
            text = [f"{i.mnemonic} {i.op_str}" for i in tail]
            nxt = next((k for k, s in enumerate(text) if re.fullmatch(r"mov (e\w\w), dword ptr \[\1 \+ 0x18\]", s)), None)
            ok_loop = (
                nxt is not None
                and all(i.mnemonic != "ret" for i in tail[:nxt])
                and any(i.mnemonic.startswith("j") and int(i.op_str, 16) < tail[nxt].address for i in tail[nxt + 1 :] if i.op_str.startswith("0x"))
            )
    check(ok_loop, f"the by-name dispatcher (+{dispatcher or 0:#x}) goes on to the next record after a match")
    after = image.dis(call + 5, 1)[0]
    check(f"{after.mnemonic} {after.op_str}" == "add esp, 0x10", "add esp, 0x10 follows the call: the caller pops")
    return call


def check_client(path: Path, check):
    print(f"== {path} ==")
    image = Image(path)
    # gEngfuncs: Initialize copies 0x87 dwords into it (`mov ecx, 0x87` ...
    # `mov edi, imm32`, `push eax`, `rep movsd`).
    m = re.search(rb"\xb9\x87\x00\x00\x00.{0,32}?\xbf(....)\x50?\xf3\xa5", image.img, re.S)
    if not check(m is not None, "found gEngfuncs (Initialize's copy of the engine table)"):
        return
    gengfuncs = struct.unpack("<I", m.group(1))[0]
    slot = struct.pack("<I", gengfuncs + 25 * 4)
    sites = []
    i = image.img.find(b"\xff\x15" + slot)
    while i >= 0:
        sites.append(i)
        i = image.img.find(b"\xff\x15" + slot, i + 1)
    # `add esp, 4` within the next two instructions (one site keeps the
    # result first: `mov esi, eax`).
    popped = all(
        any(f"{i.mnemonic} {i.op_str}" == "add esp, 4" for i in image.dis(s + 6, 2)) for s in sites
    )
    reads_text = any(b"\x8b\x40\x28" in image.img[s + 6 : s + 16] for s in sites)
    check(len(sites) == 5 and popped, f"slot 25 has 5 calls, each popping one argument: {[hex(s) for s in sites]}")
    check(reads_text, "one reads [eax+0x28], the text, from the result")
    msglog = (SRC / "msglog.rs").read_text(encoding="utf-8")
    for message in ("VGUIMenu", "MOTD"):
        mt = re.search(rf'\("{message}", (0x[0-9a-f]+)\)', msglog)
        if not check(mt is not None, f"msglog.rs has a thunk for {message}"):
            continue
        thunk = int(mt.group(1), 16)
        refs = []
        ptr = struct.pack("<I", image.base + thunk)
        j = image.img.find(ptr)
        while j >= 0:
            refs.append(j)
            j = image.img.find(ptr, j + 1)
        direct = [a for a, mn, t in image.sweep() if t == thunk]
        check(
            len(refs) == 1 and not direct,
            f"{message}'s thunk +{thunk:#x} is referenced once (+{refs[0] if refs else 0:#x}) and never called directly",
        )


def main() -> int:
    args = sys.argv[1:]
    hws, client = [], DEFAULT_CLIENT
    while args:
        flag = args.pop(0)
        if flag == "--hw" and args:
            hws.append(Path(args.pop(0)))
        elif flag == "--client" and args:
            client = Path(args.pop(0))
        else:
            print(__doc__)
            return 2
    hws = hws or DEFAULT_HW
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)
        return bool(passed)

    for path in hws:
        if not path.is_file():
            check(False, f"no hw.dll at {path}")
            continue
        print(f"== {path} ==")
        image = Image(path)
        spans = [s for s in (check_center_print(image, check), check_hud_message(image, check)) if s is not None]
        for call in spans:
            # The sweep must have decoded the call itself, or "nothing
            # branches in" would prove nothing about this part of the code.
            seen = (call, "call", image.call_target(call)) in image.sweep()
            check(seen, f"the linear sweep decodes the call at +{call:#x} ({len(image.sweep())} branches in all)")
            into = image.branches_into(call, call + 5)
            relocs = image.relocs_into(call, call + 5)
            check(not into and not relocs, f"nothing branches or points into +{call:#x}..+{call + 5:#x}: {into} {relocs}")
    if client.is_file():
        check_client(client, check)
    else:
        check(False, f"no client.dll at {client}")
    print("ALL OK" if ok else "SOME CHECKS FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
