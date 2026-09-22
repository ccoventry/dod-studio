#!/usr/bin/env python3
"""Maps what HLAE patches in GoldSrc's `hw.dll`, and checks our own findings
in it. Produces the tables in `docs/goldsrc_hw_dll_survey.md`.

`hw.dll` is the engine. Unlike `client.dll` — which HLAE barely touches for DoD,
having no `dod_` patterns at all — HLAE patches `hw.dll` heavily, and two hooks
over one span destroy each other. So the first job of any engine-side work is
**subtraction**: what does HLAE already own?

## Nothing of HLAE's is copied into this repository

This reads `AfxHookGoldSrc.dll` **from the user's own HLAE install, at run
time**. Its pattern strings are never written out, never committed, and never
used as signatures of ours. What the reports carry is the *result* of applying
them: addresses in the **game's** binary, which are facts about `hw.dll` rather
than anything of HLAE's. That is the same line
`docs/goldsrc_death_notices.md` draws — ideas yes, code no — and it is drawn
here deliberately rather than by accident.

Sections:

    keys       HLAE's complete key database, read out of its static
               initialisers: every name it resolves, whether it detours it,
               and which of its own hooks it attaches
    collide    each of HLAE's patterns applied to hw.dll -- the map of where
               HLAE writes, expressed as game-binary addresses
    findings   our own findings, re-checked against the shipped hw.dll: the
               command buffer's real size, ex_interp's clamp, and the
               self-naming error strings that name engine functions
    parse      the engine's own svc dispatch table -- 60 records naming every
               handler -- CL_ParseServerMessage, and the functions hw.dll
               names in its own messages
    entities   CL_ParsePacketEntities, CL_FlushEntityPacket and the predicate
               that fires it
    decals     the decal ring: pool, count, unlink, and what is reachable
    pin        HLAE patterns that match in several places, resolved to the
               function each match sits in

Usage:
    python goldsrc-hooks/tools/survey_hw_dll.py [section...]
        [--hw PATH] [--afx PATH]

Defaults to the pre-Anniversary movies install and the pre-Anniversary HLAE.
`keys`, `collide` and `pin` need HLAE present; the rest do not. Requires `pefile`
and `capstone` (`pip install pefile capstone`); both are analysis-only and are
not build dependencies of anything in the workspace.
"""

import bisect
import re
import struct
import sys
from pathlib import Path

try:
    import pefile
    import capstone
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

DEFAULT_HW = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\hw.dll"
)
DEFAULT_AFX = Path(r"C:\Program Files (x86)\HLAE\HLAE (Pre-Anniversary)\AfxHookGoldSrc.dll")

# HLAE's key registrations are `push <name>; push <slot>; mov ecx, <map>; call`,
# one per key, emitted as C++ static initialisers.
KEY_REGISTRATION = re.compile(rb"\x68(....)\x68(....)\xb9(....)\xe8", re.S)

# A pattern in HLAE's own spelling, which `scan.rs` deliberately shares.
PATTERN_TEXT = re.compile(r"^(?:[0-9A-Fa-f]{2}|\?\?)(?: (?:[0-9A-Fa-f]{2}|\?\?))+$")

# `DetourAttach(&target, hook)` in AfxHookGoldSrc, identified by the three-call
# transaction shape around it (begin / update-thread / attach) and confirmed by
# the `.detourc`/`.detourd` sections the module carries.
DETOUR_ATTACH = 0x26070

# Our own findings, re-checked rather than restated. Each is (rva, expected
# bytes, what it is) -- a mismatch means this hw.dll is not the analysed build.
FINDINGS = [
    (
        0x272B0,
        bytes.fromhex("68004000"),
        "Cbuf_Init: SZ_Alloc(cmd_text, 0x4000) -- the command buffer is 16,384 bytes",
    ),
    (
        0x18EF8,
        bytes.fromhex("bb64000000"),
        "the ex_interp ceiling, 100 ms, as `mov ebx, 0x64`",
    ),
    (
        0x18F68,
        bytes.fromhex("bbc8000000"),
        "the raised ex_interp ceiling, 200 ms, as `mov ebx, 0xc8`",
    ),
]

# Error messages GoldSrc prints with the function's own name in them. This is
# the only naming this survey has for `hw.dll` -- it exports almost nothing
# useful and carries no RTTI, unlike `client.dll`.
SELF_NAMING = re.compile(
    r"^(Cbuf_|Cmd_|CL_|SV_|Host_|Mod_|R_|S_|SND_|Sys_|Con_|SCR_|V_|PM_|Netchan_|MSG_|COM_|Draw_|GL_|Key_)"
    r"[A-Za-z0-9_]*\s*:"
)


class Image:
    def __init__(self, path):
        self.path = Path(path)
        self.pe = pefile.PE(str(path), fast_load=True)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.data = bytes(self.pe.get_memory_mapped_image())
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self.md.detail = True
        self.sections = [
            (s.Name.rstrip(b"\0").decode(), s.VirtualAddress,
             max(s.Misc_VirtualSize, s.SizeOfRawData), s.Characteristics)
            for s in self.pe.sections
        ]

    def sect(self, rva):
        for name, va, size, _ in self.sections:
            if va <= rva < va + size:
                return name
        return "?"

    def code_ranges(self):
        return [(va, size) for _n, va, size, ch in self.sections if ch & 0x20000000]

    def cstr(self, rva, limit=512):
        end = self.data.find(b"\0", rva, rva + limit)
        if end < 0:
            return None
        try:
            return self.data[rva:end].decode("ascii")
        except UnicodeDecodeError:
            return None

    def strings(self, minlen=5):
        return {
            m.start(): m.group()[:-1].decode("ascii")
            for m in re.finditer(rb"[\x20-\x7e]{%d,}\x00" % minlen, self.data)
        }

    def find(self, needle):
        out, at = [], self.data.find(needle)
        while at >= 0:
            out.append(at)
            at = self.data.find(needle, at + 1)
        return out

    def xrefs_to(self, rva):
        return self.find(struct.pack("<I", self.base + rva))

    def disasm(self, start, end):
        return self.md.disasm(self.data[start:end], self.base + start)

    def match_pattern(self, text):
        rx = re.compile(
            b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in text.split()),
            re.S,
        )
        return [m.start() for m in rx.finditer(self.data) if self.sect(m.start()) == ".text"]

    def calls_to(self, rva):
        out = []
        for start, size in self.code_ranges():
            blob = self.data[start:start + size]
            at = blob.find(b"\xe8")
            while at >= 0 and at + 5 <= len(blob):
                if start + at + 5 + struct.unpack_from("<i", blob, at + 1)[0] == rva:
                    out.append(start + at)
                at = blob.find(b"\xe8", at + 1)
        return out


# ── HLAE's database ──────────────────────────────────────────────────────────

def hlae_keys(afx):
    """{result slot rva: key name}, from the static initialisers."""
    out = {}
    for m in KEY_REGISTRATION.finditer(afx.data):
        if afx.sect(m.start()) != ".text":
            continue
        name = struct.unpack("<I", m.group(1))[0] - afx.base
        slot = struct.unpack("<I", m.group(2))[0] - afx.base
        text = afx.cstr(name) if 0 <= name < len(afx.data) else None
        if text and afx.sect(slot) == ".data":
            out[slot] = text
    return out


def hlae_patterns(afx):
    return {rva: text for rva, text in sorted(afx.strings().items()) if PATTERN_TEXT.match(text)}


def pattern_owner(afx, keys):
    """{pattern rva: key name}, by taking the next key-slot store after each use.

    The scan writes its result to the key's slot a couple of dozen bytes later,
    and the two sequences run in lockstep, so "the next store" is exact rather
    than a guess. Where it is not — a pattern used twice — both are reported.
    """
    stores = []
    for slot, name in keys.items():
        needle = struct.pack("<I", afx.base + slot)
        for at in afx.find(needle):
            if afx.sect(at) != ".text":
                continue
            if afx.data[at - 1:at] == b"\xa3" or afx.data[at - 2:at - 1] == b"\x89":
                stores.append((at, name))
    stores.sort()
    addrs = [a for a, _n in stores]

    out = {}
    for prva in hlae_patterns(afx):
        for ref in afx.xrefs_to(prva):
            if afx.sect(ref) != ".text":
                continue
            i = bisect.bisect_left(addrs, ref)
            if i < len(stores):
                out.setdefault(prva, []).append(stores[i][1])
    return out


def hlae_detours(afx, keys):
    """{key name: hook rva} for every key HLAE attaches a Detours hook to."""
    def pushes_before(rva, back=0x20):
        for lo in range(rva - back, rva):
            got, reached = [], False
            for ins in afx.disasm(lo, rva + 5):
                if ins.address - afx.base == rva:
                    reached = True
                    break
                if ins.mnemonic == "push" and ins.op_str.startswith("0x"):
                    got.append(int(ins.op_str, 16) - afx.base)
                elif ins.mnemonic in ("call", "jmp", "ret"):
                    got = []
            if reached and len(got) >= 2:
                return got[-2], got[-1]
        return None, None

    def source_keys(target, back=0x60):
        """Which key slot the target variable was copied from."""
        found = set()
        for at in afx.find(b"\xa3" + struct.pack("<I", afx.base + target)):
            if afx.sect(at) != ".text":
                continue
            window = afx.data[max(0, at - back):at]
            for m in re.finditer(b"\xa1", window):
                src = struct.unpack_from("<I", window, m.start() + 1)[0] - afx.base
                if src in keys:
                    found.add(keys[src])
        return sorted(found)

    out = {}
    for call in sorted(afx.calls_to(DETOUR_ATTACH)):
        hook, target = pushes_before(call)
        if target is None:
            continue
        names = [keys[target]] if target in keys else source_keys(target)
        # The copy chain can pick up neighbours; the last one written before the
        # attach is the right one, and the sequences run in order.
        if names:
            out.setdefault(names[-1], hook)
    return out


# ── Reports ──────────────────────────────────────────────────────────────────

def report_keys(hw, afx):
    keys = hlae_keys(afx)
    detoured = hlae_detours(afx, keys)
    print(f"== HLAE's key database: {len(keys)} keys ==")
    print(f"read from {afx.path.name}, which is not modified and nothing from which is stored\n")
    engine = [n for n in keys.values() if not n.startswith(("cstrike_", "tfc_", "valve_"))]
    game = [n for n in keys.values() if n.startswith(("cstrike_", "tfc_", "valve_"))]
    print(f"engine-side / generic: {len(engine)}   game-client-side: {len(game)}")
    print("  game-client keys are for cstrike, tfc and valve only -- there is no `dod_` key.\n")
    for slot, name in sorted(keys.items()):
        hook = detoured.get(name)
        mark = f"DETOURED, hook at afx+{hook:#x}" if hook else "resolved only"
        print(f"  {name:<46} {mark}")


def report_collide(hw, afx):
    keys = hlae_keys(afx)
    owners = pattern_owner(afx, keys)
    detoured = hlae_detours(afx, keys)
    print("== where HLAE's patterns land in this hw.dll ==")
    print("Addresses are in the GAME binary. A unique match is a place HLAE writes or")
    print("reads; several matches means the pattern is used with a restricted search")
    print("range we do not reproduce, so the address is not pinned here.\n")
    for prva, text in sorted(hlae_patterns(afx).items()):
        names = owners.get(prva, [])
        name = names[0] if names else "?"
        if name.startswith(("cstrike_", "tfc_", "valve_")):
            continue
        hits = hw.match_pattern(text)
        where = f"hw+{hits[0]:#x}" if len(hits) == 1 else f"{len(hits)} matches"
        mark = " [detoured]" if name in detoured else ""
        print(f"  {name:<46} {len(text.split()):>4} bytes -> {where}{mark}")


def report_findings(hw, _afx):
    print("== our own findings, re-checked against this hw.dll ==\n")
    ok = True
    for rva, want, what in FINDINGS:
        got = hw.data[rva:rva + len(want)]
        verdict = "OK  " if got == want else "FAIL"
        if got != want:
            ok = False
        print(f"  {verdict} hw+{rva:#08x}  {what}")
        if got != want:
            print(f"         expected {want.hex(' ')}, found {got.hex(' ')}")

    print("\n== functions hw.dll names in its own error messages ==")
    print("The only naming available: hw.dll carries no RTTI and exports almost")
    print("nothing, unlike client.dll.\n")
    names = {}
    for rva, text in sorted(hw.strings().items()):
        m = SELF_NAMING.match(text)
        if not m:
            continue
        refs = [r for r in hw.xrefs_to(rva) if hw.sect(r) == ".text"]
        names.setdefault(m.group(0).rstrip(": ").strip(), []).extend(refs)
    for name, refs in sorted(names.items()):
        where = ", ".join(f"+{r:#x}" for r in sorted(set(refs))[:3]) or "(no direct reference)"
        print(f"  {name:<34} referenced from {where}")
    return ok



# ── The engine's own dispatch table ──────────────────────────────────────────
# `CL_ParseServerMessage` dispatches through a table of 12-byte records,
# `{int opcode; char *name; void (*func)(void);}`. It is the naming strategy
# #273 asked for: it names 58 handlers that the call graph cannot see at all,
# because none of them is ever the target of a direct `call`.
#
# Found by shape rather than by address: a run of records whose opcodes count
# from zero and whose name pointers are consecutive `svc_` strings. The
# dispatch code is then checked against it, so the two confirm each other.

SVC_RECORD = 12

# Above this many matches a pattern is a short prologue HLAE searches inside a
# range it already knows, and no naming on our side pins it.
MAX_PINNABLE = 8


def svc_table(hw):
    """(table rva, [(opcode, name, handler rva)]) or (None, []) if not found."""
    first = hw.data.find(b"svc_bad\0")
    if first < 0:
        return None, []
    for at in hw.find(struct.pack("<I", hw.base + first)):
        start = at - 4  # the opcode field precedes the name pointer
        rows = []
        rva = start
        while rva + SVC_RECORD <= len(hw.data):
            opcode = struct.unpack_from("<i", hw.data, rva)[0]
            name_va, func_va = struct.unpack_from("<II", hw.data, rva + 4)
            name = hw.cstr(name_va - hw.base) if hw.base < name_va else None
            if name is None or not name.startswith("svc_") or opcode != len(rows):
                break
            rows.append((opcode, name, func_va - hw.base if func_va else 0))
            rva += SVC_RECORD
        if len(rows) > 32:
            return start, rows
    return None, []


def dispatch_site(hw, table):
    """Where CL_ParseServerMessage reads the table's name and func fields.

    The opcode is scaled by 12 as `lea reg, [reg + reg*2]` then `*4`, so the
    displacements the code carries are `table+4` (name) and `table+8` (func).
    Finding both is what promotes the table from a plausible data shape to the
    thing the engine actually dispatches through.
    """
    out = {}
    for field, offset in (("name", 4), ("func", 8)):
        refs = [r for r in hw.find(struct.pack("<I", hw.base + table + offset))
                if hw.sect(r) == ".text"]
        out[field] = refs
    return out


def call_targets(hw):
    """Every address reached by a direct `call rel32`: the function entries the
    call graph can see."""
    out = set()
    for start, size in hw.code_ranges():
        blob = hw.data[start:start + size]
        at = blob.find(b"\xe8")
        while at >= 0 and at + 5 <= len(blob):
            target = start + at + 5 + struct.unpack_from("<i", blob, at + 1)[0]
            if start <= target < start + size:
                out.add(target)
            at = blob.find(b"\xe8", at + 1)
    return sorted(out)


def owner_of(entries, rva):
    """The function entry `rva` most likely belongs to."""
    i = bisect.bisect_right(entries, rva) - 1
    return entries[i] if i >= 0 else None


# An engine identifier appearing anywhere in a string, not only as a `Name:`
# prefix. `CL_FlushEntityPacket` is the case that forced this: the engine names
# it in `"WARNING:  CL_FlushEntityPacket"`, with the name at the end and no
# colon after it, so the prefix-only heuristic missed the one function #273
# said was not located.
NAMED_IN_MESSAGE = re.compile(
    r"\b((?:Cbuf|Cmd|CL|SV|Host|Mod|R|S|SND|Sys|Con|SCR|V|PM|Netchan|MSG|COM"
    r"|Draw|Key|EV|Delta|DELTA|NET|SZ|CRC|VID|IN|CDAudio|Voice)_[A-Za-z0-9_]{2,})"
)


def named_functions(hw, entries):
    """{name: {function rva}} from strings that mention an engine identifier.

    Only *messages* count -- a string that is nothing but the identifier is an
    OpenGL extension name or a cvar, not a function naming itself. Requiring a
    space or colon elsewhere in the string drops 41 such false positives.
    """
    names = {}
    for rva, text in hw.strings().items():
        found = list(NAMED_IN_MESSAGE.finditer(text))
        if not found:
            continue
        if not re.search(r"[ :]", text.replace(found[0].group(1), "", 1)):
            continue
        refs = [r for r in hw.xrefs_to(rva) if hw.sect(r) == ".text"]
        for match in found:
            for ref in refs:
                owner = owner_of(entries, ref)
                if owner is not None:
                    names.setdefault(match.group(1), set()).add(owner)
    return names


def report_parse(hw, afx):
    print("== the engine's svc dispatch table ==")
    print("`{int opcode; char *name; void (*func)(void);}`, 12 bytes per record.")
    print("This is the naming strategy #273 asked for.\n")
    table, rows = svc_table(hw)
    if table is None:
        print("  not found -- this is not the analysed build")
        return False
    sites = dispatch_site(hw, table)
    handled = [r for r in rows if r[2]]
    print(f"  table at hw+{table:#x}, {len(rows)} records, {len(rows) * SVC_RECORD} bytes")
    print(f"  {len(handled)} of them carry a handler; svc_bad and svc_nop do not")
    for field, refs in sites.items():
        where = ", ".join(f"hw+{r:#x}" for r in refs) or "(none)"
        print(f"  dispatch reads .{field} (table+{4 if field == 'name' else 8}) from {where}")
    if not sites["func"]:
        print("  FAIL no code reads the handler field -- the table shape is a coincidence")
        return False

    entries = call_targets(hw)
    parse_msg = owner_of(entries, sites["func"][0])
    print(f"\n  CL_ParseServerMessage = hw+{parse_msg:#x} (the function that dispatches)")

    unseen = [r for r in handled if r[2] not in set(entries)]
    print(f"  {len(unseen)} of the {len(handled)} handlers are reached ONLY through this")
    print("  table -- no direct call anywhere, so a call-graph walk cannot find them\n")
    for opcode, name, func in rows:
        print(f"  {opcode:3}  {name:<24} {('hw+%#x' % func) if func else '(handled inline)'}")

    print("\n== functions named by their own messages ==")
    print("Broadened from the `Name:` prefix the first pass used. See")
    print("NAMED_IN_MESSAGE for why, and which function forced it.\n")
    names = named_functions(hw, entries)
    single = {n: sorted(s)[0] for n, s in names.items() if len(s) == 1}
    ambiguous = sorted(n for n, s in names.items() if len(s) > 1)
    print(f"  {len(names)} identifiers named; {len(single)} resolve to one function\n")
    for name in sorted(single):
        print(f"  {name:<34} hw+{single[name]:#08x}")
    if ambiguous:
        print(f"\n  named in more than one function, so not pinned: {', '.join(ambiguous)}")
        print("  (a name in two places usually means one of them inlined the other)")
    return True


def report_entities(hw, afx):
    """CL_ParsePacketEntities and the flush, which #273 left unlocated."""
    print("== CL_ParsePacketEntities and CL_FlushEntityPacket ==\n")
    entries = call_targets(hw)
    ok = True

    def locate(needle, what):
        rva = hw.data.find(needle)
        if rva < 0:
            print(f"  FAIL {what}: the string is not in this build")
            return None, None
        refs = [r for r in hw.xrefs_to(rva) if hw.sect(r) == ".text"]
        if not refs:
            print(f"  FAIL {what}: the string is never referenced")
            return None, None
        return owner_of(entries, refs[0]), refs[0]

    parse, parse_ref = locate(
        b"CL_ParsePacketEntities: newindex == MAX_PACKET_ENTITIES\0",
        "CL_ParsePacketEntities",
    )
    flush, flush_ref = locate(b"WARNING:  CL_FlushEntityPacket\0", "CL_FlushEntityPacket")
    if parse is None or flush is None:
        return False
    print(f"  CL_ParsePacketEntities = hw+{parse:#x}   (names itself at hw+{parse_ref:#x})")
    print(f"  CL_FlushEntityPacket   = hw+{flush:#x}   (names itself at hw+{flush_ref:#x})")

    sites = hw.calls_to(flush)
    owners = {owner_of(entries, s) for s in sites}
    print(f"\n  called from {len(sites)} site(s): " + ", ".join(f"hw+{s:#x}" for s in sites))
    print("  all inside " + ", ".join(f"hw+{o:#x}" for o in sorted(owners)))
    if owners != {parse}:
        print("  NOTE the flush is reached from outside CL_ParsePacketEntities")
        ok = False

    # The predicate, as the engine spells it: the flush fires when the frame
    # being deltad against is further back than the history buffer keeps.
    backup = struct.unpack_from("<I", hw.data, 0x13AFC8)[0]
    mask = struct.unpack_from("<I", hw.data, 0x13AFCC)[0]
    print(f"\n  CL_UPDATE_BACKUP  hw+0x13afc8 = {backup}")
    print(f"  CL_UPDATE_MASK    hw+0x13afcc = {mask}")
    if backup != 64 or mask != backup - 1:
        print("  FAIL those are not the 64/63 pair the analysed build carries")
        ok = False
    else:
        print("  The engine reads both from .data rather than baking them as")
        print("  immediates, which is what makes the predicate checkable here:")
        print("  flush when ((incoming_sequence - oldpacket) & 0xff) >= CL_UPDATE_MASK.")
    return ok


def report_decals(hw, afx):
    """The decal ring, which #273 lists as located but not surveyed."""
    print("== the decal ring ==\n")
    remove = hw.match_pattern(
        "55 8B EC 56 57 8B 7D 08 BE ?? ?? ?? ?? 0F BF 46 16 85 C7 74 13 "
        "56 E8 ?? ?? ?? ?? 6A 1C 6A 00 56 E8 ?? ?? ?? ?? 83 C4 10 83 C6 1C "
        "81 FE ?? ?? ?? ?? 7C DA"
    )
    init = hw.match_pattern(
        "68 00 C0 01 00 6A 00 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 0C "
        "C7 05 ?? ?? ?? ?? 00 00 00 00"
    )
    if len(remove) != 1 or len(init) != 1:
        print(f"  remove-by-flag loop: {len(remove)} match(es); R_DecalInit: {len(init)} match(es)")
        print("  Not the pre-Anniversary engine -- the Anniversary build compiles")
        print("  the remove loop differently. See goldsrc-hooks/src/decals.rs.")
        return len(init) == 1

    def u32(rva):
        return struct.unpack_from("<I", hw.data, rva)[0]

    pool = u32(remove[0] + 9)
    end = u32(remove[0] + 45)
    unlink = remove[0] + 22 + 5 + struct.unpack_from("<i", hw.data, remove[0] + 23)[0]
    cleared = u32(init[0] + 1)
    count = u32(init[0] + 22)
    slots = cleared // 0x1C
    print(f"  R_DecalRemoveAll<by flag>  hw+{remove[0]:#x}")
    print(f"  R_DecalInit                hw+{init[0]:#x}")
    print(f"  R_DecalUnlink              hw+{unlink:#x}")
    print(f"  gDecalPool                 {pool:#x} .. {end:#x}")
    print(f"  gDecalCount                {count:#x}")
    print(f"  sizeof(decal_t)            0x1c, so {slots} slots (MAX_RENDER_DECALS)")
    print("\n  #273 asked whether the ring's SIZE is reachable. It is not a cvar:")
    print("  the pool is a fixed 0x1c000-byte array and `r_decals` only bounds how")
    print("  far the index travels before wrapping. What IS reachable is emptying")
    print("  it, which is what dodstudio_clear_decals does -- see")
    print("  goldsrc-hooks/src/decals.rs and docs/goldsrc_decals.md.")
    ok = pool + cleared == end and cleared % 0x1C == 0
    if not ok:
        print("\n  FAIL the pool span and R_DecalInit's memset length disagree")
    return ok


def landmarks(hw, entries):
    """{function rva: name} for the functions this survey derives rather than
    reads out of a string: the dispatcher, and the two the decal work names."""
    out = {}
    table, _rows = svc_table(hw)
    if table is not None:
        sites = dispatch_site(hw, table)
        if sites["func"]:
            owner = owner_of(entries, sites["func"][0])
            if owner is not None:
                out[owner] = "CL_ParseServerMessage"
    remove = hw.match_pattern(
        "55 8B EC 56 57 8B 7D 08 BE ?? ?? ?? ?? 0F BF 46 16 85 C7 74 13 "
        "56 E8 ?? ?? ?? ?? 6A 1C 6A 00 56 E8 ?? ?? ?? ?? 83 C4 10 83 C6 1C "
        "81 FE ?? ?? ?? ?? 7C DA"
    )
    if len(remove) == 1:
        out[remove[0]] = "R_DecalRemoveAllByFlag"
        unlink = remove[0] + 27 + struct.unpack_from("<i", hw.data, remove[0] + 23)[0]
        out[unlink] = "R_DecalUnlink"
    init = hw.match_pattern(
        "68 00 C0 01 00 6A 00 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 0C "
        "C7 05 ?? ?? ?? ?? 00 00 00 00"
    )
    if len(init) == 1:
        out[init[0]] = "R_DecalInit"
    return out


def report_hlae_pin(hw, afx):
    """Disambiguates HLAE's ambiguous patterns using the functions now named.

    Only patterns with a handful of matches are worth reporting: one that
    matches hundreds of times is a two- or three-byte prologue HLAE searches
    inside a range it already knows, and no amount of naming on our side turns
    that into an address.
    """
    keys = hlae_keys(afx)
    owners = pattern_owner(afx, keys)
    detoured = hlae_detours(afx, keys)
    entries = call_targets(hw)

    # Everything this survey can put a name to, keyed by function address.
    named = {}
    _table, rows = svc_table(hw)
    for _op, name, func in rows:
        if func:
            named[func] = name
    for name, where in named_functions(hw, entries).items():
        if len(where) == 1:
            named.setdefault(sorted(where)[0], name)
    named.update(landmarks(hw, entries))

    print("== HLAE patterns pinned to a named function ==")
    print("A pattern matching several places is not pinned by the match alone.")
    print(f"Naming functions first resolves some of them. {len(named)} names available.\n")
    for prva, text in sorted(hlae_patterns(afx).items()):
        names = owners.get(prva, [])
        key = names[0] if names else "?"
        if key.startswith(("cstrike_", "tfc_", "valve_")):
            continue
        hits = hw.match_pattern(text)
        if len(hits) < 2 or len(hits) > MAX_PINNABLE:
            continue
        inside = []
        for hit in hits:
            owner = owner_of(entries, hit)
            label = named.get(owner)
            inside.append(f"{label} (hw+{hit:#x})" if label else f"hw+{hit:#x}")
        mark = " [detoured]" if key in detoured else ""
        hit_names = [h for h in inside if "(" in h]
        verdict = "PINNED" if len(hit_names) == 1 else "still ambiguous"
        print(f"  {key}{mark}: {len(hits)} matches -- {verdict}")
        for entry in inside:
            print(f"      {entry}")
    print()
    print("  Patterns matching more than", MAX_PINNABLE, "places are left out: those are")
    print("  short prologues HLAE searches inside a range it already knows, and")
    print("  naming functions on our side cannot narrow them.")
    return True


REPORTS = {
    "keys": report_keys,
    "collide": report_collide,
    "findings": report_findings,
    "parse": report_parse,
    "entities": report_entities,
    "decals": report_decals,
    "pin": report_hlae_pin,
}


def main(argv):
    hw_path, afx_path = DEFAULT_HW, DEFAULT_AFX
    for flag, setter in (("--hw", "hw"), ("--afx", "afx")):
        if flag in argv:
            i = argv.index(flag)
            if setter == "hw":
                hw_path = Path(argv[i + 1])
            else:
                afx_path = Path(argv[i + 1])
            del argv[i:i + 2]
    wanted = [a for a in argv if not a.startswith("-")] or list(REPORTS)
    unknown = [w for w in wanted if w not in REPORTS]
    if unknown:
        return print(f"no section {unknown}; try {list(REPORTS)}") or 2
    if not hw_path.is_file():
        return print(f"no hw.dll at {hw_path}") or 2

    hw = Image(hw_path)
    afx = None
    if any(w in ("keys", "collide", "pin") for w in wanted):
        if not afx_path.is_file():
            return print(f"no AfxHookGoldSrc.dll at {afx_path} -- `findings` works without it") or 2
        afx = Image(afx_path)

    for i, name in enumerate(wanted):
        if i:
            print()
        REPORTS[name](hw, afx)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
