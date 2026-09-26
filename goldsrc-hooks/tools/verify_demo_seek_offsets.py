#!/usr/bin/env python3
"""Checks `demo_seek.rs` against the real `DemoPlayer.dll` and `hw.dll`.

`demo_seek.rs` calls `DemoPlayer.dll`'s `IDemoPlayer` through its vftable and,
for a clean landing, writes two of the player's fields. Nothing at runtime can
prove those slots and offsets are right, so this does, for every install it is
pointed at (by default both movie installs):

  1. `DemoPlayer.dll` is a build in `BUILDS` (PE timestamp and image size).
  2. It exports only `CreateInterface`, and `demoplayer001`'s factory returns
     one static object (a singleton, so it is the player `hw.dll` drives).
  3. The `DemoPlayer` vftable has 47 slots, and each slot the module calls does
     what its HL SDK name says: `SetWorldTime` writes the clock `GetWorldTime`
     reads, `IsLoading` asks the loader (`this+0x150`), `IsActive` reads the
     player state, `GetStartTime`/`GetEndTime` read the first/last world frame's
     time through `this+FIELD_WORLD`.
  4. `ReadDemoMessage` (slot 45) is what the clean landing assumes: it takes the
     world frame at the clock through `IWorld` slot `WORLD_SLOT_GET_FRAME_BY_TIME`,
     compares `this+FIELD_LAST_FRAME_SEQ_NR` with the frame's sequence number at
     `FRAME_SEQ_NR`, sends director events from `this+FIELD_LAST_FRAME_TIME` and
     then stores the clock there, and runs each frame's demo data from the mark
     it read on entry -- so moving the two marks is what skips the burst.
  5. Issue #405 item 1: `ExecuteDemoFileCommands` hands a `ConsoleCommand`
     frame's 64 bytes to the engine's validity check and then its filtered
     command buffer (`IEngineWrapper` slots 27 and 28), and in `hw.dll` those
     two slots are the very functions `playdemo`'s own type-3 reader calls. So
     a command that runs under `playdemo` runs under `viewdemo`.

Every constant comes out of `demo_seek.rs` rather than being restated here.

Usage:
    python goldsrc-hooks/tools/verify_demo_seek_offsets.py [game-folder ...]

A game folder is the one holding `hl.exe`. Needs `pip install pefile capstone`.
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
DEFAULT_GAMES = [
    STEAM / "Half-Life - PRE-Anniversary for Movies",
    STEAM / "Half-Life - POST-Anniversary for Movies",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "demo_seek.rs"

SLOT_READ_DEMO_MESSAGE = 45
DEMOPLAYER_SLOTS = 47


def rust_const(src: str, name: str) -> int:
    match = re.search(rf"const {name}: usize = (0x[0-9a-f_]+|\d+);", src)
    if not match:
        raise SystemExit(f"could not find `const {name}` in demo_seek.rs")
    return int(match.group(1).replace("_", ""), 0)


def rust_builds(src: str):
    return [
        (name, int(stamp.replace("_", ""), 0), int(size.replace("_", ""), 0))
        for name, stamp, size in re.findall(
            r'name: "([^"]+)",\s*time_date_stamp: (0x[0-9a-f_]+),\s*size_of_image: (0x[0-9a-f_]+),',
            src,
        )
    ]


class Image:
    def __init__(self, path: Path):
        self.pe = pefile.PE(str(path))
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.image = bytes(self.pe.get_memory_mapped_image())
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self.md.skipdata = True
        self.code = [
            (s.VirtualAddress, s.VirtualAddress + s.Misc_VirtualSize)
            for s in self.pe.sections
            if s.Characteristics & 0x20000000  # IMAGE_SCN_MEM_EXECUTE
        ]

    def u32(self, rva: int) -> int:
        return struct.unpack_from("<I", self.image, rva)[0]

    def in_code(self, rva: int) -> bool:
        return any(lo <= rva < hi for lo, hi in self.code)

    def body(self, rva: int, limit: int = 0x600):
        """`(rva, text)` up to the padding after a `ret` -- or the next function
        when there is none, which the checks below tolerate."""
        out = []
        for ins in self.md.disasm(self.image[rva : rva + limit], self.base + rva):
            if ins.mnemonic in ("int3", "nop") and out and out[-1][1].startswith("ret"):
                break
            out.append((ins.address - self.base, f"{ins.mnemonic} {ins.op_str}".strip()))
        return out

    def direct_calls(self, body):
        calls = []
        for _, text in body:
            match = re.fullmatch(r"call 0x([0-9a-f]+)", text)
            if match:
                calls.append(int(match.group(1), 16) - self.base)
        return calls

    def string(self, text: bytes) -> int:
        return self.image.find(text + b"\0")

    def refs(self, rva: int):
        needle = struct.pack("<I", self.base + rva)
        return [m.start() for m in re.finditer(re.escape(needle), self.image)]

    def vftable(self, cls: str, slots: int) -> list:
        """vftables of `cls` (MSVC RTTI) whose first `slots` entries are all code."""
        found = []
        for name in self.refs_bytes(f".?AV{cls}@@\0".encode()):
            descriptor = name - 8
            for hit in self.refs(descriptor):
                col = hit - 12
                if self.u32(col) != 0 or self.u32(col + 4) != 0:
                    continue  # the object's primary vftable only
                for ref in self.refs(col):
                    table = ref + 4
                    entries = [self.u32(table + 4 * k) - self.base for k in range(slots)]
                    if all(self.in_code(e) for e in entries):
                        found.append((table, entries))
        return found

    def refs_bytes(self, needle: bytes):
        return [m.start() for m in re.finditer(re.escape(needle), self.image)]

    def e8_calls_to(self, target: int):
        sites = []
        for lo, hi in self.code:
            i = lo
            while True:
                i = self.image.find(b"\xe8", i, hi)
                if i < 0:
                    break
                if i + 5 + struct.unpack_from("<i", self.image, i + 1)[0] == target:
                    sites.append(i)
                i += 1
        return sites


def disp(n: int) -> str:
    """A displacement as capstone prints it: decimal below 10, hex from there."""
    return str(n) if n < 10 else hex(n)


def first_ret(body) -> str:
    return next((t for _, t in body if t.startswith("ret")), "")


def has(body, pattern: str) -> bool:
    return any(re.fullmatch(pattern, text) for _, text in body)


def verify(game: Path, src: str) -> bool:
    world = rust_const(src, "FIELD_WORLD")
    last_time = rust_const(src, "FIELD_LAST_FRAME_TIME")
    last_seq = rust_const(src, "FIELD_LAST_FRAME_SEQ_NR")
    seq_nr = rust_const(src, "FRAME_SEQ_NR")
    by_time = rust_const(src, "WORLD_SLOT_GET_FRAME_BY_TIME") * 4
    slot = {n: rust_const(src, f"SLOT_{n}") for n in
            ("SET_WORLD_TIME", "IS_LOADING", "IS_ACTIVE", "GET_WORLD_TIME", "GET_START_TIME", "GET_END_TIME")}

    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"\n==== {game} ====")
    dp = Image(game / "DemoPlayer.dll")

    # -- 1. the build ---------------------------------------------------------
    stamp, size = dp.pe.FILE_HEADER.TimeDateStamp, dp.pe.OPTIONAL_HEADER.SizeOfImage
    build = next((n for n, s, z in rust_builds(src) if (s, z) == (stamp, size)), None)
    check(build, f"DemoPlayer.dll timestamp {stamp:#x}, size {size:#x} is a build in BUILDS ({build})")

    # -- 2. the interface -----------------------------------------------------
    exports = [e.name.decode() for e in dp.pe.DIRECTORY_ENTRY_EXPORT.symbols if e.name]
    check(exports == ["CreateInterface"], f"exports only CreateInterface: {exports}")
    name = dp.string(b"demoplayer001")
    registrations = []
    for ref in dp.refs(name):
        # `push "demoplayer001"; push factory; mov ecx, reg; call InterfaceReg`
        if dp.image[ref - 1] == 0x68 and dp.image[ref + 4] == 0x68 and dp.image[ref + 9] == 0xB9:
            factory = dp.u32(ref + 5) - dp.base
            registrations.append(dp.body(factory, 0x10)[:2])
    singleton = [r for r in registrations
                 if len(r) == 2 and re.fullmatch(r"mov eax, 0x[0-9a-f]+", r[0][1]) and r[1][1] == "ret"]
    check(len(registrations) == 1 and singleton,
          f"demoplayer001 is registered once, by a factory returning one static: {registrations}")

    # -- 3. the slots ---------------------------------------------------------
    tables = dp.vftable("DemoPlayer", DEMOPLAYER_SLOTS)
    check(len(tables) == 1, f"one {DEMOPLAYER_SLOTS}-slot DemoPlayer vftable: {[hex(t) for t, _ in tables]}")
    if not tables:
        return False
    entries = tables[0][1]
    after = dp.u32(tables[0][0] + 4 * DEMOPLAYER_SLOTS) - dp.base
    check(not dp.in_code(after), f"and slot {DEMOPLAYER_SLOTS} is not code, so it has exactly {DEMOPLAYER_SLOTS}")

    body = {n: dp.body(entries[i], 0x80) for n, i in slot.items()}
    check(has(body["SET_WORLD_TIME"], r"(fstp|movsd) qword ptr \[ecx \+ 0x3a8\](, xmm0)?")
          and first_ret(body["SET_WORLD_TIME"]) == "ret 0xc",
          f"slot {slot['SET_WORLD_TIME']} SetWorldTime(double, bool) stores this+0x3a8 and pops 12 bytes")
    check([t for _, t in body["GET_WORLD_TIME"]][:2] == ["fld qword ptr [ecx + 0x3a8]", "ret"],
          f"slot {slot['GET_WORLD_TIME']} GetWorldTime returns this+0x3a8")
    check(body["IS_LOADING"][0][1] == "mov ecx, dword ptr [ecx + 0x150]",
          f"slot {slot['IS_LOADING']} IsLoading asks the loader at this+0x150")
    check(has(body["IS_ACTIVE"], r"(mov e\w\w, |cmp )dword ptr \[ecx \+ 0x37c\].*"),
          f"slot {slot['IS_ACTIVE']} IsActive reads the player state at this+0x37c")
    for which, world_slot in (("GET_START_TIME", 0x58), ("GET_END_TIME", 0x54)):
        texts = [t for _, t in body[which]]
        check(texts[:3] == [f"mov ecx, dword ptr [ecx + {world:#x}]", "mov eax, dword ptr [ecx]",
                            f"call dword ptr [eax + {world_slot:#x}]"] and "fld dword ptr [eax]" in texts,
              f"slot {slot[which]} {which} reads a world frame's float time at +0 through this+{world:#x}")

    # -- 4. ReadDemoMessage ---------------------------------------------------
    rdm = dp.body(entries[SLOT_READ_DEMO_MESSAGE], 0x500)
    combined = list(rdm)
    for callee in dp.direct_calls(rdm):
        combined += dp.body(callee, 0x300)
    switch = next(i for i, (_, t) in enumerate(rdm) if t.startswith("jmp dword ptr ["))
    check(any(re.fullmatch(rf"mov e\w\w, dword ptr \[esi \+ {last_seq:#x}\]", t) for _, t in rdm[:switch]),
          f"ReadDemoMessage reads the last-sent mark this+{last_seq:#x} on entry")
    check(has(rdm, r"call dword ptr \[e\w\w \+ 0x50\]"),
          "and walks frames by sequence number (IWorld +0x50) to run their demo data")
    frame_reg = None
    for i, (_, t) in enumerate(combined):
        if t == f"call dword ptr [eax + {by_time:#x}]":
            nxt = next((u for _, u in combined[i + 1 : i + 4] if re.fullmatch(r"mov e\w\w, eax", u)), None)
            frame_reg = nxt.split()[1].rstrip(",") if nxt else None
            break
    check(frame_reg, f"WriteDatagram takes the frame at the clock from IWorld +{by_time:#x} (into {frame_reg})")
    if frame_reg:
        check(has(combined, rf"(cmp|mov) e\w\w, dword ptr \[{frame_reg} \+ {disp(seq_nr)}\]"),
              f"and reads its sequence number at frame+{seq_nr:#x}")
        check(has(combined, rf"(fld|movss) (dword ptr )?(xmm0, )?(dword ptr )?\[{frame_reg}\]"),
              "and its float time at frame+0")
    check(has(combined, rf"(fld|movsd) (xmm0, )?qword ptr \[esi \+ {last_time:#x}\]"),
          f"WriteCommands' start time is this+{last_time:#x}")
    check(has(combined, rf"(mov dword|movsd qword) ptr \[esi \+ {last_time:#x}\], (eax|xmm0)"),
          f"and the clock is stored back into this+{last_time:#x} after sending")

    # -- 5. type-3 frames -----------------------------------------------------
    message = dp.string(b"WARNING! DemoPlayer::ExecuteDemoFileCommands: unexpected demo file command %i\n")
    # The callee that starts nearest below the warning's `push`.
    users = sorted(f for f in dp.direct_calls(rdm) if any(0 < r - f < 0x400 for r in dp.refs(message)))[-1:]
    check(users, f"ReadDemoMessage calls ExecuteDemoFileCommands ({[hex(u) for u in users]})")
    wrapper_slots = None
    if users:
        edfc = dp.body(users[0], 0x400)
        texts = [t for _, t in edfc]
        at = next((i for i, t in enumerate(texts) if re.fullmatch(r"cmp e\w\w, 3", t)), None)
        window = texts[at : at + 24] if at is not None else []
        # The 25th Anniversary build loads the slot into a register first.
        valid = next((t for t in window if re.fullmatch(r"(call|mov e\w\w,) dword ptr \[e\w\w \+ 0x6c\]", t)), None)
        add = next((t for t in window if re.fullmatch(r"call dword ptr \[e\w\w \+ 0x70\]", t)), None)
        check("push 0x40" in window and valid and add,
              "a ConsoleCommand frame's 64 bytes go to IEngineWrapper +0x6c (check), then +0x70 (add)")
        wrapper_slots = (0x6C // 4, 0x70 // 4)

    hw = Image(game / "hw.dll")
    tables = hw.vftable("EngineWrapper", 29)
    check(len(tables) == 1, f"one 29-slot EngineWrapper vftable in hw.dll: {[hex(t) for t, _ in tables]}")
    if tables and wrapper_slots:
        thunk = {k: hw.direct_calls(hw.body(tables[0][1][k], 0x20)) for k in wrapper_slots}
        check(all(len(v) == 1 for v in thunk.values()), f"slots {wrapper_slots} are one-call thunks")
        valid, add = thunk[wrapper_slots[0]][0], thunk[wrapper_slots[1]][0]
        readers = []
        for site in hw.e8_calls_to(valid):
            window = [t for _, t in hw.body(site - 0x30, 0x60)]
            if "cmp al, 3" in window and "push 0x40" in window and \
                    window.count(f"call {hw.base + add:#x}") == 2:
                readers.append(site)
        check(len(readers) == 1,
              f"playdemo's type-3 reader ({[hex(r) for r in readers]}) calls the same check "
              f"({valid:#x}) and filtered add ({add:#x}) as viewdemo does")
    return ok


def main() -> int:
    games = [Path(a) for a in sys.argv[1:]] or [g for g in DEFAULT_GAMES if g.is_dir()]
    if not games:
        return print("no game folders found; pass one") or 2
    src = RUST.read_text(encoding="utf-8")
    ok = all([verify(g, src) for g in games])
    print("\nOFFSETS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
