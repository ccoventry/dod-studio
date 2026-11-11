#!/usr/bin/env python3
"""Checks `demo_next_map.rs` against a real `Core.dll` and `DemoPlayer.dll`.

`demo_next_map.rs` (R&D, issue #1372) makes `viewdemo` play on into a demo's
next map: it changes which `Server` method the demo's `reconnect` calls, wraps
two vftable slots, and holds `DemoFile`'s reader through two of its fields.
The unit tests prove the pure parts; only the binaries can prove what the
patch relies on:

  0. Exactly one of the builds in `BUILDS` (pre-Anniversary, 25th
     Anniversary) matches this `Core.dll`, exactly once, and the others match
     nothing.
  1. The byte at the build's `site` is the vftable offset of
     `call dword ptr [eax + 0x48]`, reached only when the method at vftable
     offset 0x60 returned true, in the branch that follows a comparison with
     the string "reconnect". The other side of that test calls
     `[reg + 0x44]` the same way (`this` in `ecx`, nothing pushed), so the
     two slots have one calling shape.
  2. `Server`'s vftable (by RTTI): slot 18 (0x48) is the method that prints
     "Demo playback stopped." (`Disconnect`), slot 17 (0x44) the one that
     prints "Reconnecting..." (`Reconnect`), slot 23 is `m_ServerState >= 6`
     (`IsConnected`), and slot 24 (`IsDemoFile`) is
     `add ecx, <the build's demo_file>; jmp`. Neither `Reconnect` nor
     `Disconnect` pops arguments.
  3. `Server::ParseServerinfo` stops only for a demo whose state is 7
     (RUNNING): after `Reconnect` (state 6) the next map's signon goes
     through.
  4. `DemoFile::ReadDemoPacket` begins by returning when its file handle
     (`DEMO_FILE_HANDLE`) is 0, or when `DEMO_FILE_NEXT_READ_TIME` is ahead
     of the clock and `DEMO_FILE_CONTINUOUS` is set.
  5. `DemoPlayer.dll`: `IsLoading` (slot 28) is "`m_Server`
     (`PLAYER_FIELD_SERVER`) -> its vftable offset 0x5c" (`IsConnected`).
     `ReadDemoMessage` (slot 45) switches on `FIELD_PLAYER_STATE`, zeroes the
     last sent frame number on the way to `PLAYER_STATE_RUNNING`, and
     `NewGame` (`PLAYER_SLOT_NEW_GAME`) sets the state to 2, pops its two
     arguments, and is called through the vftable by `ReceiveSignal` (slot
     3), so a wrap of the slot sees every call. (The two state constants are
     `demo_seek.rs`'s, added for `played_to_end`.)

Usage:
    python goldsrc-hooks/tools/verify_demo_next_map_offsets.py [game folder]
    python goldsrc-hooks/tools/verify_demo_next_map_offsets.py --anniversary

Defaults to the pre-Anniversary movies install; `--anniversary` is the 25th
Anniversary movies install. Needs `pip install pefile capstone`.
"""

import hashlib
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
DEFAULT_GAME = STEAM / "Half-Life - PRE-Anniversary for Movies"
ANNIVERSARY_GAME = STEAM / "Half-Life - POST-Anniversary for Movies"
SRC = Path(__file__).resolve().parent.parent / "src"
RUST = SRC / "demo_next_map.rs"
SEEK_RUST = SRC / "demo_seek.rs"

SERVER_SLOTS = 26
DEMOPLAYER_SLOTS = 47
SLOT_DISCONNECT = 18
SLOT_IS_CONNECTED = 23
SLOT_RECEIVE_SIGNAL = 3
SLOT_READ_DEMO_MESSAGE = 45
SERVER_RUNNING = 7
SERVER_CONNECTED = 6


def rust_const(src: str, name: str, file: str) -> int:
    match = re.search(rf"const {name}: (?:usize|u8|u32) = (0x[0-9a-f_]+|\d+);", src)
    if not match:
        raise SystemExit(f"could not find `const {name}` in {file}")
    return int(match.group(1).replace("_", ""), 0)


def rust_builds(src: str):
    """[(name, pattern, site, demo_file)] from `BUILDS` in demo_next_map.rs."""
    table = re.search(r"const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S)
    if not table:
        raise SystemExit("could not find `const BUILDS` in demo_next_map.rs")
    builds = [
        (name, " ".join(pattern.split()), int(site), int(demo_file.replace("_", ""), 0))
        for name, pattern, site, demo_file in re.findall(
            r'name: "([^"]+)",.*?pattern: "(.*?)",\s*site: (\d+),\s*demo_file: (0x[0-9a-f_]+),',
            table.group(1),
            re.S,
        )
    ]
    if not builds:
        raise SystemExit("`BUILDS` in demo_next_map.rs has no entries this script can read")
    return builds


class Image:
    def __init__(self, path: Path):
        self.path = path
        self.raw = path.read_bytes()
        self.pe = pefile.PE(str(path), fast_load=True)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.image = bytes(self.pe.get_memory_mapped_image())
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self.md.skipdata = True
        self.code = [
            (s.VirtualAddress, s.VirtualAddress + s.Misc_VirtualSize)
            for s in self.pe.sections
            if s.Characteristics & 0x20000000  # IMAGE_SCN_MEM_EXECUTE
        ]

    def describe(self) -> str:
        return (f"{len(self.raw):,} bytes, md5 {hashlib.md5(self.raw).hexdigest()}, "
                f"timestamp {self.pe.FILE_HEADER.TimeDateStamp:#x}, "
                f"image size {self.pe.OPTIONAL_HEADER.SizeOfImage:#x}")

    def u32(self, rva: int) -> int:
        return struct.unpack_from("<I", self.image, rva)[0]

    def in_code(self, rva: int) -> bool:
        return any(lo <= rva < hi for lo, hi in self.code)

    def lines(self, rva: int, size: int):
        """`(rva, text)` for the instructions in `size` bytes from `rva`."""
        return [
            (ins.address - self.base, f"{ins.mnemonic} {ins.op_str}".strip())
            for ins in self.md.disasm(self.image[rva : rva + size], self.base + rva)
        ]

    def body(self, rva: int, limit: int = 0x600):
        """The instructions from `rva` up to the padding after a `ret` or a
        tail `jmp`."""
        out = []
        for at, text in self.lines(rva, limit):
            ended = out and (out[-1][1].startswith("ret") or out[-1][1].startswith("jmp"))
            if text.split()[0] in ("int3", "nop") and ended:
                break
            out.append((at, text))
        return out

    def string(self, text: str) -> int:
        return self.image.find(text.encode() + b"\0")

    def refs(self, rva: int):
        needle = struct.pack("<I", self.base + rva)
        return [m.start() for m in re.finditer(re.escape(needle), self.image)]

    def matches(self, pattern: str):
        rx = re.compile(
            b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()), re.S
        )
        return [m.start() for lo, hi in self.code for m in rx.finditer(self.image, lo, hi)]

    def vftables(self, cls: str, slots: int):
        """vftables of `cls` (MSVC RTTI, the object's primary one) whose first
        `slots` entries are all code."""
        found = []
        name = f".?AV{cls}@@\0".encode()
        for at in [m.start() for m in re.finditer(re.escape(name), self.image)]:
            descriptor = at - 8
            for hit in self.refs(descriptor):
                col = hit - 12
                if self.u32(col) != 0 or self.u32(col + 4) != 0:
                    continue
                for ref in self.refs(col):
                    table = ref + 4
                    entries = [self.u32(table + 4 * k) - self.base for k in range(slots)]
                    if all(self.in_code(e) for e in entries):
                        found.append((table, entries))
        return found


def mentions(body, text: str) -> bool:
    return any(text in line for _, line in body)


def main() -> int:
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    game = ANNIVERSARY_GAME if arg == "--anniversary" else Path(arg) if arg else DEFAULT_GAME
    core_path, player_path = game / "Core.dll", game / "DemoPlayer.dll"
    for path in (core_path, player_path):
        if not path.is_file():
            return print(f"no {path.name} at {path}") or 2

    src = RUST.read_text(encoding="utf-8")
    seek_src = SEEK_RUST.read_text(encoding="utf-8")
    builds = rust_builds(src)
    const = lambda name: rust_const(src, name, "demo_next_map.rs")
    seek_const = lambda name: rust_const(seek_src, name, "demo_seek.rs")
    core, player = Image(core_path), Image(player_path)
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    # -- 0. which build ------------------------------------------------------
    print(f"== {core_path} ==")
    print(f"     {core.describe()}")
    hits = {b[0]: core.matches(b[1]) for b in builds}
    for name, found in hits.items():
        print(f"     {name} signature: {len(found)} match(es) {[hex(m) for m in found]}")
    matching = [b for b in builds if hits[b[0]]]
    if len(matching) != 1 or len(hits[matching[0][0]]) != 1:
        print("FAIL exactly one build's signature should match, exactly once")
        print("\nMISMATCH -- do not ship")
        return 1
    name, pattern, site, demo_file = matching[0]
    found = hits[name][0]
    at = found + site
    check(True, f"this is the {name} Core.dll; the signature matches once, at +{found:#x}")

    # -- 1. the call ---------------------------------------------------------
    print("\n== the reconnect branch ==")
    disconnect_at = const("CALL_DISCONNECT")
    reconnect_at = const("CALL_RECONNECT")
    call = core.lines(at - 2, 3)
    check(call and call[0][1] == f"call dword ptr [eax + {disconnect_at:#x}]",
          f"+{at - 2:#x}: `{call[0][1] if call else '?'}`, the byte at +{at:#x} its vftable offset")
    check(core.image[at] == disconnect_at, f"the byte at +{at:#x} is {disconnect_at:#x}")
    before = core.lines(found - 0x30, 0x30 + site - 2)
    texts = [t for _, t in before]
    check(texts[-2:] == ["mov eax, dword ptr [esi]", "mov ecx, esi"],
          f"the call's `this` is esi, with nothing pushed: {texts[-2:]}")
    je = next(((a, t) for a, t in reversed(before) if t.startswith("je ")), None)
    check(je and texts[texts.index(je[1]) - 1] == "test al, al",
          f"`test al, al; {je[1] if je else '?'}` guards it")
    check(any("0x60]" in t for t in texts[-8:]),
          "the tested call is the vftable's offset 0x60 (IsDemoFile): "
          + "; ".join(t for t in texts[-8:] if "0x60]" in t or t.startswith("call")))
    word = core.string("reconnect")
    pushes = [a for a in core.refs(word) if found - 0x30 <= a < found]
    check(word > 0 and pushes, f'"reconnect" (+{word:#x}) is pushed just before, at '
          + ", ".join(f"+{a - 1:#x}" for a in pushes))
    if je:
        other = int(je[1].split()[1], 16) - core.base
        elsewhere = [t for _, t in core.lines(other, 0x60)]
        via = [t for t in elsewhere if re.fullmatch(rf"call dword ptr \[e.x \+ {reconnect_at:#x}\]", t)]
        check(via and not any(t.startswith("push") for t in elsewhere[max(0, elsewhere.index(via[0]) - 2) : elsewhere.index(via[0])]) if via else False,
              f"the other side of the test (+{other:#x}) calls offset {reconnect_at:#x} the same way: {via[:1]}")

    # -- 2. Server's vftable ---------------------------------------------------
    print("\n== Server's vftable ==")
    tables = core.vftables("Server", SERVER_SLOTS)
    check(len(tables) == 1, f"one Server vftable by RTTI: {[hex(t) for t, _ in tables]}")
    if tables:
        table, entries = tables[0]
        slot_reconnect = const("SERVER_SLOT_RECONNECT")
        slot_is_demo = const("SERVER_SLOT_IS_DEMO_FILE")
        check(slot_reconnect * 4 == reconnect_at and SLOT_DISCONNECT * 4 == disconnect_at,
              f"slots {slot_reconnect} and {SLOT_DISCONNECT} are offsets {reconnect_at:#x} and {disconnect_at:#x}")
        stopped = core.string("Demo playback stopped.\n")
        disconnect = core.body(entries[SLOT_DISCONNECT])
        check(mentions(disconnect, hex(core.base + stopped)),
              f'slot {SLOT_DISCONNECT} (+{entries[SLOT_DISCONNECT]:#x}) prints "Demo playback stopped."')
        reconnecting = core.string("Reconnecting...\n")
        reconnect = core.body(entries[slot_reconnect])
        check(mentions(reconnect, hex(core.base + reconnecting)),
              f'slot {slot_reconnect} (+{entries[slot_reconnect]:#x}) prints "Reconnecting..."')
        check(mentions(reconnect[:4], f"+ 0x1794], {SERVER_CONNECTED}"),
              f"Reconnect acts only from state {SERVER_CONNECTED} up: {[t for _, t in reconnect[:4]]}")
        for label, body in (("Reconnect", reconnect), ("Disconnect", disconnect)):
            popped = [t for _, t in body if re.fullmatch(r"ret 0x[0-9a-f]+|ret [1-9]\d*", t)]
            check(not popped, f"{label} pops no arguments" + (f": {popped}" if popped else ""))
        connected = [t for _, t in core.body(entries[SLOT_IS_CONNECTED])]
        check(any("0x1794]" in t for t in connected) and any(t.startswith("setge") for t in connected)
              and any(t.endswith(f", {SERVER_CONNECTED}") for t in connected),
              f"slot {SLOT_IS_CONNECTED} (IsConnected, +{entries[SLOT_IS_CONNECTED]:#x}) is m_ServerState >= {SERVER_CONNECTED}: {connected}")
        thunk = core.image[entries[slot_is_demo] : entries[slot_is_demo] + 7]
        check(thunk[:2] == b"\x81\xc1" and thunk[6] == 0xE9
              and int.from_bytes(thunk[2:6], "little") == demo_file,
              f"slot {slot_is_demo} (IsDemoFile, +{entries[slot_is_demo]:#x}) is `add ecx, {demo_file:#x}; jmp`: {thunk.hex(' ')}")

    # -- 3. ParseServerinfo ----------------------------------------------------
    print("\n== Server::ParseServerinfo ==")
    received = core.string("Serverinfo packet received.\n")
    refs = [a for a in core.refs(received) if core.in_code(a)]
    check(len(refs) == 1, f'"Serverinfo packet received." is pushed once: {[hex(a) for a in refs]}')
    if refs:
        head = [t for _, t in core.lines(refs[0] - 0x40, 0x40)]
        # Linear disassembly from 0x40 back may start mid-instruction; the
        # compare and the call sit in its last few lines either way.
        check(any(re.fullmatch(rf"cmp dword ptr \[e.. \+ 0x1794\], {SERVER_RUNNING}", t) for t in head)
              and any(re.fullmatch(rf"call dword ptr \[e.x \+ {disconnect_at:#x}\]", t) for t in head),
              f"it calls Disconnect only when m_ServerState is {SERVER_RUNNING} (RUNNING)")

    # -- 4. DemoFile::ReadDemoPacket ------------------------------------------
    print("\n== DemoFile::ReadDemoPacket ==")
    handle, next_read, continuous = (const("DEMO_FILE_HANDLE"), const("DEMO_FILE_NEXT_READ_TIME"),
                                     const("DEMO_FILE_CONTINUOUS"))
    bad = core.string("WARNING! DemoFile::ReadDemoPacket: Bad demo length.\n")
    refs = [a for a in core.refs(bad) if core.in_code(a)]
    check(len(refs) == 1, f"ReadDemoPacket's \"Bad demo length\" is pushed once: {[hex(a) for a in refs]}")
    if refs:
        # The function's start: the nearest 16-aligned code address below the
        # string reference that a direct call targets and padding precedes
        # (a stray E8 byte "targets" anywhere).
        targets = set()
        for lo, hi in core.code:
            i = lo
            while True:
                i = core.image.find(b"\xe8", i, hi - 4)
                if i < 0:
                    break
                targets.add(i + 5 + struct.unpack_from("<i", core.image, i + 1)[0])
                i += 1
        start = max(t for t in targets
                    if t <= refs[0] and core.in_code(t) and t % 16 == 0
                    and core.image[t - 1] in (0x90, 0xCC, 0xC3))
        guard = core.lines(start, 0xa0)
        texts = [t for _, t in guard]
        check(refs[0] - start < 0x800, f"ReadDemoPacket starts at +{start:#x}")
        first_handle = next((k for k, t in enumerate(texts) if f"+ {handle:#x}]" in t), None)
        first_time = next((k for k, t in enumerate(texts) if f"+ {next_read:#x}]" in t), None)
        first_cont = next((k for k, t in enumerate(texts)
                           if re.fullmatch(rf"cmp byte ptr \[e.. \+ {continuous:#x}\], (bl|0)", t)), None)
        check(first_handle is not None and first_time is not None and first_cont is not None
              and first_handle < first_time < first_cont,
              f"it tests the handle (+{handle:#x}), then m_nextReadTime (+{next_read:#x}) against the clock, "
              f"then m_Continuous (+{continuous:#x})")
        if first_cont is not None:
            leave = texts[first_cont + 1]
            first_exit = next((t for t in texts[: first_time or 0] if t.startswith("je ")), "")
            check(leave.startswith("jne ") and first_exit and leave.split()[1] == first_exit.split()[1],
                  f"both tests leave for the same place: `{first_exit}` and `{leave}`")

    # -- 5. DemoPlayer.dll -----------------------------------------------------
    print(f"\n== {player_path} ==")
    print(f"     {player.describe()}")
    tables = player.vftables("DemoPlayer", DEMOPLAYER_SLOTS)
    check(len(tables) == 1, f"one DemoPlayer vftable by RTTI: {[hex(t) for t, _ in tables]}")
    if tables:
        _, entries = tables[0]
        server_field = const("PLAYER_FIELD_SERVER")
        slot_loading = const("PLAYER_SLOT_IS_LOADING")
        loading = [t for _, t in player.body(entries[slot_loading], 0x20)]
        check(loading[0] == f"mov ecx, dword ptr [ecx + {server_field:#x}]"
              and any(f"+ {SLOT_IS_CONNECTED * 4:#x}]" in t for t in loading),
              f"slot {slot_loading} (IsLoading, +{entries[slot_loading]:#x}) is m_Server (+{server_field:#x}) -> "
              f"vftable offset {SLOT_IS_CONNECTED * 4:#x}: {loading}")
        state = seek_const("FIELD_PLAYER_STATE")
        running = seek_const("PLAYER_STATE_RUNNING")
        last_seq = seek_const("FIELD_LAST_FRAME_SEQ_NR")
        message = [t for _, t in player.lines(entries[SLOT_READ_DEMO_MESSAGE], 0x400)]
        loads = next((k for k, t in enumerate(message) if t == f"mov eax, dword ptr [esi + {state:#x}]"), None)
        check(loads is not None and f"cmp eax, {running}" in message[loads : loads + 4],
              f"ReadDemoMessage (slot {SLOT_READ_DEMO_MESSAGE}) switches on m_PlayerState (+{state:#x}), "
              f"{running} its last case")
        to_connected = next((k for k, t in enumerate(message)
                             if t == f"mov dword ptr [esi + {state:#x}], {running - 1}"), None)
        to_running = next((k for k, t in enumerate(message)
                           if t == f"mov dword ptr [esi + {state:#x}], {running}"), None)
        zeroed = [k for k, t in enumerate(message)
                  if re.fullmatch(rf"mov dword ptr \[esi \+ {last_seq:#x}\], (0|ebp)", t)]
        check(to_connected is not None and to_running is not None
              and any(to_connected < k < to_running for k in zeroed),
              f"it zeroes the last sent frame number (+{last_seq:#x}) between setting the state to "
              f"{running - 1} and to {running}")
        slot_new_game = const("PLAYER_SLOT_NEW_GAME")
        new_game = [t for _, t in player.body(entries[slot_new_game], 0x100)]
        check(f"mov dword ptr [esi + {state:#x}], 2" in new_game,
              f"NewGame (slot {slot_new_game}, +{entries[slot_new_game]:#x}) sets the state to 2 (CONNECTING)")
        first_ret = next((t for t in new_game if t.startswith("ret")), "?")
        check(first_ret == "ret 8", f"NewGame pops two arguments: `{first_ret}`")
        signal = [t for _, t in player.lines(entries[SLOT_RECEIVE_SIGNAL], 0x140)]
        via = [k for k, t in enumerate(signal)
               if re.fullmatch(rf"call dword ptr \[e.x \+ {slot_new_game * 4:#x}\]", t)]
        check(len(via) == 1 and sum(1 for t in signal[via[0] - 5 : via[0]] if t.startswith("push")) == 2,
              f"ReceiveSignal (slot {SLOT_RECEIVE_SIGNAL}) calls NewGame through the vftable (offset "
              f"{slot_new_game * 4:#x}) with two arguments pushed")

    print("\nOFFSETS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
