#!/usr/bin/env python3
"""Names `client.dll`'s functions from the symbols in DoD's Linux `client.so`.

`client.so` (GCC 4.6, unstripped) and `client.dll` (MSVC, no symbols) were
built from the same source, so most functions exist in both. This matches them
up from what survives a change of compiler, strongest evidence first:

    export     the same exported name (`HUD_Redraw`, `F`, the weapon entities)
    vtable     the same slot of the same class's vftable (`client.dll` ships
               RTTI, so its vftables carry class names)
    callback   a function pointer passed next to the same string, which is how
               commands, user messages and events are registered:
               `pfnAddCommand("+attack", IN_AttackDown)`
    features   the same rare strings, constants and engine-table calls
               (`cl_enginefunc_t` is the same table in both builds)
    callgraph  between two matched functions, the calls in between line up:
               when both have the same number of unmatched calls between the
               same two matched ones, those pair up in order

Each pair is checked before it is kept: sizes within a factor of
`MAX_SIZE_RATIO`, and functions that both reference strings must share some.
Names come with the method that found them, so a reader can weigh them; see
`docs/goldsrc_linux_debug_symbols.md` for how reliable each method measured.

MSVC folds identical functions into one (`return 0;` bodies, empty
destructors); such an address gets the first name found for it.

Usage:
    python goldsrc-hooks/tools/name_client_dll.py [--install PATH] [--out FILE] [--no METHOD ...]

Writes a tab-separated map (rva, name, method, client.so address, mangled
name) to FILE (default: stdout) and a summary to stderr. `--no`
leaves a method out, to measure the others against it. Requires `pyelftools`,
`pefile` and `capstone`.
"""

import bisect
import math
import re
import struct
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import linux_so  # noqa: E402
import survey_client_dll  # noqa: E402
from survey_client_dll import GENGFUNCS, GENGFUNCS_FIELDS, type_descriptors  # noqa: E402

try:
    import capstone
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone pyelftools`")

METHODS = ("export", "vtable", "callback", "features", "callgraph")
# A pair whose sizes differ by more than this is rejected: inlining moves code
# between functions, but not by this much in a function both builds kept.
MAX_SIZE_RATIO = 3.0
# Below this many bytes sizes say nothing (thunks, `return x;`).
SMALL_FUNCTION_BYTES = 32
# `features`: a pair needs this much shared evidence (sum of IDF weights) and
# must beat the runner-up by `FEATURE_MARGIN` on both sides.
FEATURE_MIN_SCORE = 6.0
FEATURE_MARGIN = 1.5
# `callgraph`: the most unmatched calls a gap may hold and still pair in order.
CALLGRAPH_MAX_GAP = 3
# Immediates below this are counts, flags and small offsets: everywhere, and
# laid out differently by the two compilers.
MIN_CONSTANT = 0x100
HEX = re.compile(r"0x[0-9a-f]+")


# ── The two binaries, as functions with features ─────────────────────────────

class Function:
    """One function's evidence: what it references, in instruction order."""

    def __init__(self, start, end):
        self.start, self.end = start, end
        self.strings = []  # string literals referenced
        self.constants = []  # rare immediates and rodata constants
        self.engine = []  # cl_enginefunc_t slots called or read
        self.calls = []  # direct call targets, in order
        self.events = []  # ("str", s) / ("code", target) / ("call", None), in order

    @property
    def size(self):
        return self.end - self.start

    def features(self):
        return set(self.strings) | set(self.constants) | set(self.engine)


def _immediates(ins):
    """Immediate operands of an instruction (hex outside any `[...]`)."""
    if ins.mnemonic in ("lea", "call", "ret") or ins.mnemonic.startswith("j"):
        return []
    if ins.mnemonic in ("add", "sub") and ins.op_str.startswith("esp"):
        return []
    outside = re.sub(r"\[[^\]]*\]", "", ins.op_str)
    return [int(h, 16) for h in HEX.findall(outside)]


def _constant_feature(value):
    signed = value - (1 << 32) if value & 0x80000000 else value
    return f"k:{value:#x}" if abs(signed) >= MIN_CONSTANT else None


class Binary:
    """Shared reading for both sides: functions, references, features."""

    def __init__(self):
        self.functions = {}  # start -> Function
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)

    # Subclasses provide: image, refs_at(place), is_function_start(addr),
    # engine_slot(target), call_target(ins), string_at(addr), rodata_value(addr).

    def _scan(self, fn):
        for ins in self.md.disasm(bytes(self.image[fn.start:fn.end]), fn.start):
            refs = [t for p in range(ins.address, ins.address + ins.size - 3)
                    for t in self.refs_at(p)]
            target = self.call_target(ins)
            if target is not None:
                fn.calls.append(target)
                fn.events.append(("call", None))
            elif ins.mnemonic == "call":
                fn.events.append(("call", None))
            for ref in refs:
                self._note_ref(fn, ref, target)
            if not refs:
                for value in _immediates(ins):
                    feature = _constant_feature(value)
                    if feature:
                        fn.constants.append(feature)

    def _note_ref(self, fn, ref, call_target):
        """Turn one referenced address into a feature or an event of `fn`."""
        if isinstance(ref, str):  # an external symbol (client.so only)
            return
        slot = self.engine_slot(ref)
        if slot is not None:
            fn.engine.append(f"e:{slot}")
            return
        if self.is_function_start(ref):
            if ref != call_target:
                fn.events.append(("code", ref))
            return
        text = self.string_at(ref)
        if text is not None:
            fn.strings.append("s:" + text)
            fn.events.append(("str", text))
            return
        value = self.rodata_value(ref)
        if value is not None:
            fn.constants.append(f"d:{value:#x}")

    def scan_all(self):
        for fn in self.functions.values():
            self._scan(fn)


class LinuxLibrary(Binary):
    """`client.so`: functions and their extents come straight from `.symtab`."""

    def __init__(self, path):
        super().__init__()
        self.lib = linux_so.SharedObject(path)
        self.image = self.lib.image
        self.names = {}  # start -> [mangled]
        for start, size, mangled in self.lib.functions:
            if start not in self.functions:
                self.functions[start] = Function(start, start + max(size, 1))
            self.names.setdefault(start, []).append(mangled)
        self.engine_base = self.lib.objects["gEngfuncs"][0]
        self._rodata = self.lib.sections[".rodata"]

    def name(self, start):
        """The best readable name of a function, and its mangled name."""
        mangled = sorted(self.names[start], key=lambda m: (not m.startswith("_Z"), len(m)))[0]
        return linux_so.demangle(mangled), mangled

    def refs_at(self, place):
        if place in self.lib.abs_refs:
            return [self.lib.abs_refs[place]]
        if place in self.lib.ext_refs:
            return [self.lib.ext_refs[place]]
        return []

    def is_function_start(self, addr):
        return addr in self.functions

    def engine_slot(self, addr):
        offset = addr - self.engine_base
        return offset // 4 if 0 <= offset < GENGFUNCS_FIELDS * 4 else None

    def call_target(self, ins):
        if ins.mnemonic not in ("call", "jmp"):
            return None
        place = ins.address + 1
        if place in self.lib.call_refs:
            target = self.lib.call_refs[place]
        elif ins.op_str.startswith("0x"):
            target = int(ins.op_str, 16)
        else:
            return None
        return target if target in self.functions and target != ins.address else None

    def string_at(self, addr):
        return self.lib.cstring(addr) if self.lib.section_of(addr) in (".rodata", ".data") else None

    def rodata_value(self, addr):
        start, size = self._rodata
        return self.lib.u32(addr) if start <= addr < start + size - 3 else None


class WindowsDll(Binary):
    """`client.dll`: function starts are found, not given.

    Starts are exports, code pointers stored outside `.text` (vftables,
    callback tables), and direct call targets, found by decoding forward from
    the starts already known until no new ones appear. Each function runs to
    the next start. MSVC keeps switch jump tables inside `.text` right after
    their function; decoding stops at one, and the case labels inside it are
    never taken as starts.
    """

    def __init__(self, path):
        super().__init__()
        pe = pefile.PE(str(path))
        self.base = pe.OPTIONAL_HEADER.ImageBase
        self.image = bytes(pe.get_memory_mapped_image())
        self.sections = {s.Name.rstrip(b"\0").decode(): (s.VirtualAddress, max(s.Misc_VirtualSize, s.SizeOfRawData))
                         for s in pe.sections}
        text_start, text_size = self.sections[".text"]
        self.text = (text_start, text_start + text_size)
        self.relocs = {}
        for block in pe.DIRECTORY_ENTRY_BASERELOC:
            for entry in block.entries:
                if entry.type == 3:  # IMAGE_REL_BASED_HIGHLOW
                    self.relocs[entry.rva] = self.u32(entry.rva) - self.base
        self.exports = {}
        for sym in pe.DIRECTORY_ENTRY_EXPORT.symbols:
            if sym.name:
                self.exports[sym.address] = sym.name.decode()
        self.jump_tables = set()
        self._find_functions()

    def u32(self, rva):
        return struct.unpack_from("<I", self.image, rva)[0]

    def in_text(self, rva):
        return self.text[0] <= rva < self.text[1]

    def _find_functions(self):
        starts = set(self.exports)
        starts |= {t for p, t in self.relocs.items() if self.in_text(t) and not self.in_text(p)}
        pending = set(starts)
        while pending:
            self._starts = sorted(starts)
            found = set()
            for start in pending:
                for ins in self._decode(start):
                    if ins.mnemonic in ("call", "jmp") and ins.op_str.startswith("0x"):
                        target = int(ins.op_str, 16)
                        if self.in_text(target) and target not in starts and                                 (ins.mnemonic == "call" or not self._inside(start, target)):
                            found.add(target)
                    found |= self._pushed_code_pointers(ins) - starts
                    if ins.mnemonic == "jmp" and "*4 +" in ins.op_str:
                        for h in HEX.findall(ins.op_str):
                            if self.in_text(int(h, 16)):
                                self.jump_tables.add(int(h, 16))
            starts |= found
            pending = found
        self._starts = sorted(starts)
        for i, start in enumerate(self._starts):
            end = self._starts[i + 1] if i + 1 < len(self._starts) else self.text[1]
            tables = [t for t in self.jump_tables if start < t < end]
            self.functions[start] = Function(start, min(tables) if tables else end)

    def _inside(self, start, target):
        """Whether `target` lies in the function at `start`, as far as the starts known so far say.

        A `jmp` that leaves its function is a tail call, so its target is a
        function start (`HUD_Init` reaches `CHud::Init` that way).
        """
        i = bisect.bisect_right(self._starts, start)
        end = self._starts[i] if i < len(self._starts) else self.text[1]
        return start <= target < end

    def _pushed_code_pointers(self, ins):
        """Function pointers an instruction passes as an immediate (`push offset fn`).

        These are the callbacks handed to the engine (commands, user messages,
        events), which nothing calls directly.
        """
        if ins.mnemonic not in ("push", "mov"):
            return set()
        out = set()
        for place in range(ins.address, ins.address + ins.size - 3):
            target = self.relocs.get(place)
            if target is not None and self.in_text(target) and f"{target + self.base:#x}" in re.sub(r"\[[^\]]*\]", "", ins.op_str):
                out.add(target)
        return out

    def _decode(self, start):
        i = bisect.bisect_right(self._starts, start)
        end = self._starts[i] if i < len(self._starts) else self.text[1]
        for ins in self.md.disasm(self.image[start:end], start):
            if ins.address in self.jump_tables:
                return
            yield ins

    def refs_at(self, place):
        return [self.relocs[place]] if place in self.relocs else []

    def is_function_start(self, rva):
        return rva in self.functions

    def engine_slot(self, rva):
        offset = rva - GENGFUNCS
        return offset // 4 if 0 <= offset < GENGFUNCS_FIELDS * 4 else None

    def call_target(self, ins):
        if ins.mnemonic not in ("call", "jmp") or not ins.op_str.startswith("0x"):
            return None
        target = int(ins.op_str, 16)
        return target if target in self.functions and target != ins.address else None

    def section_of(self, rva):
        for name, (start, size) in self.sections.items():
            if start <= rva < start + size:
                return name
        return None

    def string_at(self, rva):
        if self.section_of(rva) not in (".rdata", ".data"):
            return None
        end = self.image.find(b"\0", rva, rva + 256)
        if end <= rva:
            return None
        text = self.image[rva:end]
        if all(32 <= c < 127 or c in (9, 10, 13) for c in text):
            return text.decode("ascii")
        return None

    def rodata_value(self, rva):
        return self.u32(rva) if self.section_of(rva) == ".rdata" else None


# ── The map ──────────────────────────────────────────────────────────────────

class NameMap:
    """Pairs of (client.dll rva, client.so address), each with the method that found it."""

    def __init__(self, dll, so):
        self.dll, self.so = dll, so
        self.by_dll = {}  # rva -> (so start, method)
        self.by_so = {}  # so start -> rva
        self.rejected = Counter()

    def plausible(self, rva, so_start):
        """Sizes and strings agree well enough for the two to be one function."""
        a, b = self.dll.functions[rva], self.so.functions[so_start]
        if max(a.size, b.size) > SMALL_FUNCTION_BYTES:
            ratio = max(a.size, b.size) / max(min(a.size, b.size), 1)
            if ratio > MAX_SIZE_RATIO:
                return False
        sa, sb = set(a.strings), set(b.strings)
        if sa and sb and not sa & sb:
            return False
        return True

    def add(self, rva, so_start, method, check=True):
        """Record a pair; False when either side is taken or the pair fails `plausible`."""
        if rva not in self.dll.functions or so_start not in self.so.functions:
            return False
        if rva in self.by_dll or so_start in self.by_so:
            return False
        if check and not self.plausible(rva, so_start):
            self.rejected[method] += 1
            return False
        self.by_dll[rva] = (so_start, method)
        self.by_so[so_start] = rva
        return True


def msvc_readable(decorated):
    """`Class::Name` from an MSVC-decorated export name (`?Name@Class@@...`)."""
    if not decorated.startswith("?"):
        return decorated
    parts = decorated[1:].split("@@", 1)[0].split("@")
    return "::".join(reversed(parts))


def msvc_class_name(decorated):
    """`vgui2::Panel` from an RTTI type name (`.?AVPanel@vgui2@@`)."""
    return "::".join(reversed(decorated[4:].rstrip("@").split("@")))


def match_exports(names):
    so_by_name = defaultdict(list)
    for start in names.so.functions:
        so_by_name[names.so.name(start)[0]].append(start)
    for rva, decorated in names.dll.exports.items():
        candidates = so_by_name.get(msvc_readable(decorated), [])
        if len(candidates) == 1:
            names.add(rva, candidates[0], "export", check=False)


def _so_vtables(so):
    """{class: [slot targets]} for each `_ZTV` primary vtable, destructors collapsed."""
    out = {}
    lib = so.lib
    for mangled, (addr, size) in lib.objects.items():
        if not mangled.startswith("_ZTV"):
            continue
        cls = linux_so.demangle(mangled)[len("vtable for "):]
        slots, place = [], addr + 8
        while place < addr + size:
            if place in lib.abs_refs and lib.abs_refs[place] in so.functions:
                slots.append(lib.abs_refs[place])
            elif lib.ext_refs.get(place) == "__cxa_pure_virtual":
                slots.append(None)
            else:
                break
            place += 4
        out[cls] = _collapse_destructors(so, slots)
    return out


def _collapse_destructors(so, slots):
    """GCC gives a virtual destructor two slots (D1 complete, D0 deleting); MSVC one."""
    out, i = [], 0
    while i < len(slots):
        here = so.names.get(slots[i], [""])[0] if slots[i] else ""
        nxt = so.names.get(slots[i + 1], [""])[0] if i + 1 < len(slots) and slots[i + 1] else ""
        if "D1Ev" in here and "D0Ev" in nxt:
            out.append(slots[i + 1])
            i += 2
        else:
            out.append(slots[i])
            i += 1
    return out


def _dll_vtables(dll):
    """{class: [slot rvas]} for each primary (offset 0) vftable, via RTTI."""
    image = survey_client_dll.Image.__new__(survey_client_dll.Image)
    image.data, image.base = dll.image, dll.base
    image.sections = [(n, va, size, 0) for n, (va, size) in dll.sections.items()]
    out = {}
    packed = {}
    for place in dll.relocs:
        packed.setdefault(dll.relocs[place], []).append(place)
    for td, decorated in type_descriptors(image).items():
        cls = msvc_class_name(decorated)
        for ref in packed.get(td, []):
            col = ref - 0xC
            if col < 0 or dll.u32(col) != 0 or dll.u32(col + 4) != 0:
                continue
            for slot_place in packed.get(col, []):
                vft, slots = slot_place + 4, []
                while vft + 4 * len(slots) in dll.relocs and dll.in_text(dll.relocs[vft + 4 * len(slots)]):
                    slots.append(dll.relocs[vft + 4 * len(slots)])
                out[cls] = slots
    return out


def match_vtables(names):
    so_tables = _so_vtables(names.so)
    for cls, dll_slots in _dll_vtables(names.dll).items():
        so_slots = so_tables.get(cls)
        if not so_slots or len(so_slots) != len(dll_slots):
            continue
        readable = [names.so.name(s)[0] if s else None for s in so_slots]
        overloaded = {n for n, c in Counter(readable).items() if c > 1}
        for so_start, rva, name in zip(so_slots, dll_slots, readable):
            if so_start is None or name in overloaded:
                continue
            names.add(rva, so_start, "vtable", check=False)


def _callbacks(binary):
    """{string: code pointer} for strings passed beside exactly one function pointer."""
    pairs = defaultdict(set)
    for fn in binary.functions.values():
        segment = []
        for kind, value in fn.events + [("call", None)]:
            if kind != "call":
                segment.append((kind, value))
                continue
            strings = [v for k, v in segment if k == "str"]
            codes = [v for k, v in segment if k == "code"]
            if len(strings) == 1 and len(codes) == 1:
                pairs[strings[0]].add(codes[0])
            segment = []
    return {s: next(iter(c)) for s, c in pairs.items() if len(c) == 1}


def match_callbacks(names):
    so_pairs = _callbacks(names.so)
    for text, rva in _callbacks(names.dll).items():
        if text in so_pairs:
            # No size check: a unique string beside a unique pointer is strong
            # evidence, and GCC often inlines the handler into the `__MsgFunc_`
            # thunk that MSVC keeps as a five-instruction stub.
            names.add(rva, so_pairs[text], "callback", check=False)


def match_features(names):
    """Mutual best matches on shared rare features, weighted by rarity."""
    df = Counter()
    for side in (names.dll, names.so):
        for fn in side.functions.values():
            df.update(fn.features())
    total = len(names.dll.functions) + len(names.so.functions)
    weight = {f: math.log(total / c) for f, c in df.items()}
    index = defaultdict(list)
    for start, fn in names.so.functions.items():
        if start not in names.by_so:
            for f in fn.features():
                if df[f] <= 40:
                    index[f].append(start)

    def best_two(scores):
        ranked = sorted(scores.items(), key=lambda kv: -kv[1])
        top = ranked[0] if ranked else (None, 0.0)
        second = ranked[1][1] if len(ranked) > 1 else 0.0
        return top, second

    forward = {}
    reverse = defaultdict(dict)
    for rva, fn in names.dll.functions.items():
        if rva in names.by_dll:
            continue
        scores = Counter()
        for f in fn.features():
            for so_start in index.get(f, ()):
                scores[so_start] += weight[f]
        if not scores:
            continue
        for so_start, s in scores.items():
            reverse[so_start][rva] = s
        forward[rva] = best_two(scores)
    for rva, ((so_start, score), second) in forward.items():
        if score < FEATURE_MIN_SCORE or score < second * FEATURE_MARGIN:
            continue
        (back, back_score), back_second = best_two(reverse[so_start])
        if back != rva or back_score < back_second * FEATURE_MARGIN:
            continue
        names.add(rva, so_start, "features")


def match_callgraph(names):
    """Pair the unmatched calls lying between the same two matched calls."""
    changed = True
    while changed:
        changed = False
        for rva, (so_start, _) in list(names.by_dll.items()):
            a_calls = names.dll.functions[rva].calls
            b_calls = names.so.functions[so_start].calls
            for a_gap, b_gap in _aligned_gaps(names, a_calls, b_calls):
                a_new = [c for c in dict.fromkeys(a_gap) if c not in names.by_dll]
                b_new = [c for c in dict.fromkeys(b_gap) if c not in names.by_so]
                if len(a_new) == len(b_new) <= CALLGRAPH_MAX_GAP:
                    for a, b in zip(a_new, b_new):
                        changed |= names.add(a, b, "callgraph")


def _aligned_gaps(names, a_calls, b_calls):
    """Split two call lists at the matched calls they share, in order; yield the gaps."""
    b_index = defaultdict(list)
    for j, b in enumerate(b_calls):
        b_index[b].append(j)
    anchors, last_j = [], -1
    for i, a in enumerate(a_calls):
        partner = names.by_dll.get(a, (None,))[0]
        later = [j for j in b_index.get(partner, ()) if j > last_j]
        if later:
            anchors.append((i, later[0]))
            last_j = later[0]
    prev_i, prev_j = -1, -1
    for i, j in anchors + [(len(a_calls), len(b_calls))]:
        yield a_calls[prev_i + 1:i], b_calls[prev_j + 1:j]
        prev_i, prev_j = i, j


def build_map(dll, so, skip=()):
    names = NameMap(dll, so)
    steps = {"export": match_exports, "vtable": match_vtables, "callback": match_callbacks,
             "features": match_features, "callgraph": match_callgraph}
    for method in METHODS:
        if method not in skip:
            steps[method](names)
    if "features" not in skip and "callgraph" not in skip:
        # Each pass removes candidates and adds anchors for the other: repeat until neither finds more.
        while True:
            before = len(names.by_dll)
            match_features(names)
            match_callgraph(names)
            if len(names.by_dll) == before:
                break
    return names


# ── Output ───────────────────────────────────────────────────────────────────

def write_map(names, out):
    for rva in sorted(names.by_dll):
        so_start, method = names.by_dll[rva]
        readable, mangled = names.so.name(so_start)
        out.write(f"{rva:#x}\t{readable}\t{method}\t{so_start:#x}\t{mangled}\n")


def summary(names):
    dll_total = len(names.dll.functions)
    so_total = len(names.so.functions)
    by_method = Counter(m for _, m in names.by_dll.values())
    named_bytes = sum(names.dll.functions[r].size for r in names.by_dll)
    text_bytes = names.dll.text[1] - names.dll.text[0]
    lines = [
        f"client.dll functions found: {dll_total}; client.so functions: {so_total}",
        f"named: {len(names.by_dll)} ({100 * len(names.by_dll) / dll_total:.1f}% of functions, "
        f"{100 * named_bytes / text_bytes:.1f}% of .text bytes)",
        "by method: " + ", ".join(f"{m} {by_method[m]}" for m in METHODS),
        "rejected by the size/string check: " + ", ".join(f"{m} {c}" for m, c in names.rejected.items()),
    ]
    return "\n".join(lines)


def main(argv):
    install, out_path, skip = linux_so.DEFAULT_INSTALL, None, set()
    i = 0
    while i < len(argv):
        if argv[i] == "--install":
            install, i = Path(argv[i + 1]), i + 2
        elif argv[i] == "--out":
            out_path, i = Path(argv[i + 1]), i + 2
        elif argv[i] == "--no":
            skip.add(argv[i + 1])
            i += 2
        else:
            sys.exit(__doc__)
    dll = WindowsDll(install / "dod/cl_dlls/client.dll")
    so = LinuxLibrary(linux_so.library_path("client", install))
    dll.scan_all()
    so.scan_all()
    names = build_map(dll, so, skip)
    if out_path:
        with open(out_path, "w", encoding="utf-8") as f:
            write_map(names, f)
    else:
        write_map(names, sys.stdout)
    print(summary(names), file=sys.stderr)


if __name__ == "__main__":
    main(sys.argv[1:])
