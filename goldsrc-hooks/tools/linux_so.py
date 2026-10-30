#!/usr/bin/env python3
"""Reads DoD's Linux builds, `client.so` and `dod.so`, and their DWARF debug info.

Both movie installs ship the Linux builds next to the Windows DLLs, unstripped:
`dod/cl_dlls/client.so` and `dod/dlls/dod.so` carry `.symtab` (every function
and global by its mangled name) and full DWARF (struct members, compile units).
They were built from the same source as `client.dll` / `dod.dll`, so names and
member order carry over to the Windows build; offsets do not (GCC and MSVC lay
out some members differently). See `docs/goldsrc_linux_debug_symbols.md`.

Sections:

    syms              every function and object in `.symtab`: kind, address,
                      size, readable name, mangled name
    cus               the compile units (source files) and the compiler
    struct NAME...    member offsets, types and names of classes or structs
    callers NAME...   functions that call any function whose readable name
                      contains NAME (direct calls and calls through relocations)

Usage:
    python goldsrc-hooks/tools/linux_so.py client|dod SECTION [args] [--install PATH]

`client` reads `dod/cl_dlls/client.so`, `dod` reads `dod/dlls/dod.so`, under
the install (default: the pre-Anniversary movies install; both installs ship
byte-identical copies). Requires `pyelftools` (`pip install pyelftools`); it is
analysis-only and not a build dependency of anything in the workspace.
"""

import bisect
import io
import struct
import sys
from pathlib import Path

try:
    from elftools.elf.elffile import ELFFile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pyelftools`")

DEFAULT_INSTALL = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies"
)
LIBRARIES = {"client": Path("dod/cl_dlls/client.so"), "dod": Path("dod/dlls/dod.so")}

# i386 relocation types that `client.so` and `dod.so` carry. Both are built
# without -fPIC, so every absolute address in `.text` is patched by the loader
# through one of these (DT_TEXTREL), which is what lets a reader find them
# without disassembling.
R_386_32 = 1  # S + A: an absolute address of a symbol (plus the addend in place)
R_386_PC32 = 2  # S + A - P: a call to a global function
R_386_RELATIVE = 8  # B + A: an absolute address inside the library itself


def library_path(which, install=DEFAULT_INSTALL):
    """The `.so` for `client` or `dod` under an install folder."""
    return Path(install) / LIBRARIES[which]


# ── Names ────────────────────────────────────────────────────────────────────

_OPERATORS = {
    "nw": "new", "na": "new[]", "dl": "delete", "da": "delete[]", "aS": "=", "eq": "==",
    "ne": "!=", "lt": "<", "gt": ">", "le": "<=", "ge": ">=", "pl": "+", "mi": "-",
    "ml": "*", "dv": "/", "ix": "[]", "cl": "()", "pL": "+=", "mI": "-=", "mL": "*=",
    "dV": "/=", "ng": "-", "nt": "!", "co": "~", "an": "&", "or": "|", "aa": "&&",
    "oo": "||", "rm": "%", "eO": "^=", "aN": "&=", "oR": "|=", "ls": "<<", "rs": ">>",
    "cv": "cast", "pt": "->",
}


def demangle(mangled):
    """`Class::method` for an Itanium-mangled name, without its parameters.

    Covers what DoD's libraries use: nested names, constructors, destructors,
    operators and skipped template arguments. Anything else comes back as the
    mangled name unchanged, so a caller never gets a wrong name, only an
    unreadable one. Overloads share a readable name; the mangled one tells them
    apart.
    """
    if not mangled.startswith("_Z"):
        return mangled
    rest = mangled[2:]
    if rest[:2] in ("Th", "Tv"):  # this-adjusting thunk: `_ZThn268_<name>`
        cut = rest.find("_", 2)
        return "thunk to " + demangle("_Z" + rest[cut + 1:]) if cut > 0 else mangled
    for prefix, label in (("TV", "vtable for "), ("TI", "typeinfo for "), ("TS", "typeinfo name for ")):
        if rest.startswith(prefix):
            inner = rest[2:]
            if inner[:1].isdigit():
                inner = demangle("_ZN" + inner + "E")
            elif inner[:1] == "N":
                inner = demangle("_Z" + inner)
            return label + inner
    nested = rest.startswith("N")
    if nested:
        rest = rest[1:].lstrip("KVr")
    parts, at = [], 0
    while at < len(rest):
        ch = rest[at]
        if ch.isdigit():
            end = at
            while end < len(rest) and rest[end].isdigit():
                end += 1
            length = int(rest[at:end])
            parts.append(rest[end:end + length])
            at = end + length
        elif ch == "I":
            depth, at = 1, at + 1
            while at < len(rest) and depth:
                depth += {"I": 1, "E": -1}.get(rest[at], 0)
                at += 1
        elif ch == "C" and rest[at + 1:at + 2] in "123" and parts:
            parts.append(parts[-1])
            at += 2
        elif ch == "D" and rest[at + 1:at + 2] in "012" and parts:
            parts.append("~" + parts[-1])
            at += 2
        elif rest[at:at + 2] in _OPERATORS:
            parts.append("operator" + _OPERATORS[rest[at:at + 2]])
            at += 2
        else:
            break
        if not nested:
            break
        if at < len(rest) and rest[at] == "E":
            break
    if not parts:
        return mangled
    return "::".join(parts)


# ── The library ──────────────────────────────────────────────────────────────

class SharedObject:
    """A loaded `.so`: its sections, symbols and relocated references.

    Addresses are the library's own virtual addresses (it links at 0).
    """

    def __init__(self, path):
        self.path = Path(path)
        with open(self.path, "rb") as f:
            raw = f.read()
        self.elf = ELFFile(io.BytesIO(raw))
        self.sections = {}
        size = 0
        for sec in self.elf.iter_sections():
            if sec["sh_flags"] & 2 and sec["sh_addr"]:  # SHF_ALLOC
                self.sections[sec.name] = (sec["sh_addr"], sec["sh_size"])
                size = max(size, sec["sh_addr"] + sec["sh_size"])
        self.image = bytearray(size)
        for sec in self.elf.iter_sections():
            if sec.name in self.sections and sec["sh_type"] != "SHT_NOBITS":
                self.image[sec["sh_addr"]:sec["sh_addr"] + sec["sh_size"]] = sec.data()
        self._read_symbols()
        self._read_relocations()

    def _read_symbols(self):
        self.functions = []  # (address, size, mangled), sorted
        self.objects = {}  # mangled -> (address, size)
        for sym in self.elf.get_section_by_name(".symtab").iter_symbols():
            kind, addr = sym["st_info"]["type"], sym["st_value"]
            if not addr or not sym.name:
                continue
            if kind == "STT_FUNC":
                self.functions.append((addr, sym["st_size"], sym.name))
            elif kind == "STT_OBJECT":
                self.objects[sym.name] = (addr, sym["st_size"])
        self.functions = sorted(set(self.functions))
        self._starts = [f[0] for f in self.functions]
        self.function_by_name = {f[2]: f for f in self.functions}

    def _read_relocations(self):
        """Every relocated dword: `abs_refs` {place: target} and `ext_refs` {place: name}.

        `call_refs` holds the `R_386_PC32` call targets {place of the rel32: target}.
        A reference to a symbol the library doesn't define (libc, the engine)
        has no target, only a name, in `ext_refs`.
        """
        dynsym = self.elf.get_section_by_name(".dynsym")
        self.abs_refs, self.call_refs, self.ext_refs = {}, {}, {}
        for rel in self.elf.get_section_by_name(".rel.dyn").iter_relocations():
            place, kind = rel["r_offset"], rel["r_info_type"]
            if place + 4 > len(self.image):
                continue
            addend = struct.unpack_from("<i", self.image, place)[0]
            if kind == R_386_RELATIVE:
                self.abs_refs[place] = addend & 0xFFFFFFFF
                continue
            if kind not in (R_386_32, R_386_PC32):
                continue
            sym = dynsym.get_symbol(rel["r_info_sym"])
            if not sym["st_value"]:
                self.ext_refs[place] = sym.name
            elif kind == R_386_32:
                self.abs_refs[place] = (sym["st_value"] + addend) & 0xFFFFFFFF
            else:
                self.call_refs[place] = sym["st_value"]

    def section_of(self, addr):
        for name, (start, size) in self.sections.items():
            if start <= addr < start + size:
                return name
        return None

    def u32(self, addr):
        return struct.unpack_from("<I", self.image, addr)[0]

    def cstring(self, addr, limit=256):
        """The printable ASCII string at `addr`, or None."""
        if not 0 <= addr < len(self.image):
            return None
        end = self.image.find(b"\0", addr, addr + limit)
        if end <= addr:
            return None
        text = bytes(self.image[addr:end])
        if all(32 <= c < 127 or c in (9, 10, 13) for c in text):
            return text.decode("ascii")
        return None

    def function_at(self, addr):
        """(start, size, mangled) of the function containing `addr`, or None."""
        i = bisect.bisect_right(self._starts, addr) - 1
        if i < 0:
            return None
        fn = self.functions[i]
        return fn if fn[0] <= addr < fn[0] + max(fn[1], 1) else None

    def call_targets(self, start, size):
        """Direct call targets inside a function, in order: [(place, target)]."""
        out = []
        for at in range(start, start + size - 4):
            if self.image[at] != 0xE8:
                continue
            place = at + 1
            if place in self.call_refs:
                out.append((at, self.call_refs[place]))
            elif place not in self.abs_refs and place not in self.ext_refs:
                target = (at + 5 + struct.unpack_from("<i", self.image, place)[0]) & 0xFFFFFFFF
                if self.function_at(target) and self.function_at(target)[0] == target:
                    out.append((at, target))
        return out


# ── DWARF ────────────────────────────────────────────────────────────────────

def _type_name(die, depth=0):
    if die is None or depth > 8:
        return "?"
    attrs = die.attributes
    if die.tag == "DW_TAG_array_type":
        count = ""
        for sub in die.iter_children():
            if sub.tag == "DW_TAG_subrange_type" and "DW_AT_upper_bound" in sub.attributes:
                count += f"[{sub.attributes['DW_AT_upper_bound'].value + 1}]"
        inner = die.get_DIE_from_attribute("DW_AT_type") if "DW_AT_type" in attrs else None
        return _type_name(inner, depth + 1) + (count or "[]")
    if "DW_AT_name" in attrs and die.tag != "DW_TAG_pointer_type":
        return attrs["DW_AT_name"].value.decode(errors="replace")
    suffix = {"DW_TAG_pointer_type": "*", "DW_TAG_reference_type": "&"}.get(die.tag, "")
    prefix = {"DW_TAG_const_type": "const ", "DW_TAG_volatile_type": "volatile "}.get(die.tag, "")
    if die.tag in ("DW_TAG_pointer_type", "DW_TAG_reference_type", "DW_TAG_const_type",
                   "DW_TAG_volatile_type", "DW_TAG_typedef"):
        inner = die.get_DIE_from_attribute("DW_AT_type") if "DW_AT_type" in attrs else None
        return prefix + (_type_name(inner, depth + 1) if inner else "void") + suffix
    if die.tag == "DW_TAG_subroutine_type":
        return "fn"
    return die.tag.replace("DW_TAG_", "")


def _member_offset(attr):
    """DW_AT_data_member_location as an int: GCC 4.6 writes `DW_OP_plus_uconst <uleb>`."""
    value = attr.value
    if isinstance(value, int):
        return value
    result, shift = 0, 0
    for byte in value[1:]:
        result |= (byte & 0x7F) << shift
        shift += 7
        if not byte & 0x80:
            break
    return result


def struct_layouts(path, names):
    """{name: (size, [(kind, offset, type, member)])} for each named class/struct.

    `kind` is "base" for a base class, "member" for a data member, "static" for
    a static member (no offset). The first complete definition found wins.
    """
    want, out = set(names), {}
    with open(path, "rb") as f:
        dwarf = ELFFile(f).get_dwarf_info()
        for cu in dwarf.iter_CUs():
            for die in cu.iter_DIEs():
                if die.tag not in ("DW_TAG_class_type", "DW_TAG_structure_type", "DW_TAG_union_type"):
                    continue
                attrs = die.attributes
                if "DW_AT_name" not in attrs or "DW_AT_declaration" in attrs:
                    continue
                name = attrs["DW_AT_name"].value.decode(errors="replace")
                if name not in want or name in out:
                    continue
                rows = []
                for child in die.iter_children():
                    cattrs = child.attributes
                    if child.tag == "DW_TAG_inheritance":
                        rows.append(("base", _member_offset(cattrs["DW_AT_data_member_location"]),
                                     _type_name(child.get_DIE_from_attribute("DW_AT_type")), ""))
                    elif child.tag == "DW_TAG_member":
                        member = cattrs["DW_AT_name"].value.decode(errors="replace") if "DW_AT_name" in cattrs else "?"
                        type_name = _type_name(child.get_DIE_from_attribute("DW_AT_type")) if "DW_AT_type" in cattrs else "?"
                        if "DW_AT_data_member_location" in cattrs:
                            rows.append(("member", _member_offset(cattrs["DW_AT_data_member_location"]), type_name, member))
                        elif die.tag == "DW_TAG_union_type":
                            rows.append(("member", 0, type_name, member))
                        else:
                            rows.append(("static", None, type_name, member))
                size = attrs["DW_AT_byte_size"].value if "DW_AT_byte_size" in attrs else None
                out[name] = (size, rows)
                if len(out) == len(want):
                    return out
    return out


def compile_units(path):
    """[(source file, compiler)] for every compile unit."""
    out = []
    with open(path, "rb") as f:
        for cu in ELFFile(f).get_dwarf_info().iter_CUs():
            top = cu.get_top_DIE().attributes
            name = top["DW_AT_name"].value.decode(errors="replace") if "DW_AT_name" in top else "?"
            producer = top["DW_AT_producer"].value.decode(errors="replace") if "DW_AT_producer" in top else "?"
            out.append((name, producer))
    return out


# ── Reports ──────────────────────────────────────────────────────────────────

def report_syms(lib):
    for addr, size, mangled in lib.functions:
        print(f"FUNC\t{addr:#x}\t{size}\t{demangle(mangled)}\t{mangled}")
    for mangled, (addr, size) in sorted(lib.objects.items(), key=lambda kv: kv[1]):
        print(f"OBJECT\t{addr:#x}\t{size}\t{demangle(mangled)}\t{mangled}")


def report_cus(path):
    units = compile_units(path)
    for name, _ in units:
        print(name)
    print(f"# {len(units)} compile units; compiler: {sorted({p for _, p in units})}")


def report_structs(path, names):
    layouts = struct_layouts(path, names)
    for name in names:
        if name not in layouts:
            print(f"== {name}: not found")
            continue
        size, rows = layouts[name]
        print(f"== {name} size {size}")
        for kind, offset, type_name, member in rows:
            where = "static" if offset is None else f"+{offset:#x}"
            print(f"   {where}\t{'base ' if kind == 'base' else ''}{type_name}\t{member}")


def report_callers(lib, needles):
    callers = {}
    for start, size, mangled in lib.functions:
        for _, target in lib.call_targets(start, size):
            fn = lib.function_at(target)
            if fn:
                callers.setdefault(fn[2], set()).add(mangled)
    for start, size, mangled in lib.functions:
        readable = demangle(mangled)
        if any(n in readable for n in needles):
            names = sorted(demangle(c) for c in callers.get(mangled, ()))
            print(f"{readable} @ {start:#x} <- {names}")


def main(argv):
    install = DEFAULT_INSTALL
    if "--install" in argv:
        i = argv.index("--install")
        install = Path(argv[i + 1])
        argv = argv[:i] + argv[i + 2:]
    if len(argv) < 2 or argv[0] not in LIBRARIES:
        sys.exit(__doc__)
    path = library_path(argv[0], install)
    section, args = argv[1], argv[2:]
    if section == "syms":
        report_syms(SharedObject(path))
    elif section == "cus":
        report_cus(path)
    elif section == "struct":
        report_structs(path, args)
    elif section == "callers":
        report_callers(SharedObject(path), args)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
