#!/usr/bin/env python3
"""Lists and searches the symbols of any 32-bit Linux `.so`, and says whether it is stripped.

Valve's Linux builds of GoldSrc ship unstripped: the engine (`engine_i486.so`,
`sw.so`), `demoplayer.so`, `client.so`, `hl.so` and DoD's own libraries carry
`.symtab` and most carry DWARF too. See `docs/goldsrc_linux_symbols.md` for
which files hold what and how to fetch the ones the game installs lack.

`linux_so.py` reads DoD's two libraries in depth (DWARF structs, callers);
this one is the quick look at any file: no pyelftools needed.

Commands:

    info PATH...                 per file: size, `.symtab` or not, which DWARF
                                 sections, function and object counts, the
                                 GNU build id, the compiler (`.comment`) and
                                 any `__DATE__`-style build dates in `.rodata`
    syms PATH [--grep RE] [--demangle] [--kind func|object|all]
                                 address, size, kind and name of each symbol,
                                 sorted by address; `--grep` filters on the
                                 (demangled, if asked) name, case-insensitive

Usage:
    python goldsrc-hooks/tools/elf_symbols.py info <a.so> [<b.so> ...]
    python goldsrc-hooks/tools/elf_symbols.py syms <engine_i486.so> --grep "^Cbuf_" --demangle
"""

import argparse
import re
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from linux_so import demangle  # noqa: E402

SHT_SYMTAB = 2
SHT_NOTE = 7
STT_OBJECT = 1
STT_FUNC = 2
NT_GNU_BUILD_ID = 3
# `__DATE__` as GCC expands it ("Oct  8 2024"): the engine prints its build
# date from it (`build_number()` counts days from it), so it dates a build.
BUILD_DATE = re.compile(rb"(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) [ 0-3]\d (?:19|20)\d\d")


class NotElf(ValueError):
    """The file is not a 32-bit little-endian ELF."""


def read_sections(data):
    """[(name, type, addr, offset, size, link)] for every section of an ELF32 image."""
    if data[:4] != b"\x7fELF":
        raise NotElf("no ELF magic")
    if data[4] != 1 or data[5] != 1:
        raise NotElf("not 32-bit little-endian")
    shoff = struct.unpack_from("<I", data, 0x20)[0]
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", data, 0x2E)
    if not shoff or shoff + shnum * shentsize > len(data):
        raise NotElf("section headers missing or truncated")
    raw = [struct.unpack_from("<IIIIIII", data, shoff + i * shentsize) for i in range(shnum)]
    strtab_off = raw[shstrndx][4]
    out = []
    for name, typ, _flags, addr, off, size, link in raw:
        out.append((_cstr(data, strtab_off + name), typ, addr, off, size, link))
    return out


def _cstr(data, at):
    end = data.find(b"\0", at)
    return data[at:end if end >= 0 else len(data)].decode("latin-1")


def read_symbols(data, sections):
    """[(addr, size, kind, name)] from `.symtab`, sorted, named and non-zero only.

    `kind` is "func", "object" or "other". Empty when the file is stripped.
    """
    out = set()
    for _name, typ, _addr, off, size, link in sections:
        if typ != SHT_SYMTAB:
            continue
        strtab_off = sections[link][3]
        for at in range(off, off + size - 15, 16):
            name, value, sym_size, info = struct.unpack_from("<IIIB", data, at)
            if not name or not value:
                continue
            kind = {STT_FUNC: "func", STT_OBJECT: "object"}.get(info & 0xF, "other")
            out.add((value, sym_size, kind, _cstr(data, strtab_off + name)))
    return sorted(out)


def build_id(data, sections):
    """The GNU build id as hex, or None."""
    for _name, typ, _addr, off, size, _link in sections:
        at = off
        while typ == SHT_NOTE and at + 12 <= off + size:
            namesz, descsz, ntype = struct.unpack_from("<III", data, at)
            name_end = at + 12 + ((namesz + 3) & ~3)
            if ntype == NT_GNU_BUILD_ID and data[at + 12:at + 15] == b"GNU":
                return data[name_end:name_end + descsz].hex()
            at = name_end + ((descsz + 3) & ~3)
    return None


def section_bytes(data, sections, name):
    for sec_name, _typ, _addr, off, size, _link in sections:
        if sec_name == name:
            return data[off:off + size]
    return b""


def summarize(data):
    """What `info` prints for one file, as a dict."""
    sections = read_sections(data)
    names = [s[0] for s in sections]
    syms = read_symbols(data, sections)
    comment = section_bytes(data, sections, ".comment")
    rodata = section_bytes(data, sections, ".rodata")
    return {
        "size": len(data),
        "symtab": ".symtab" in names,
        "dwarf": [n for n in names if n.startswith(".debug_")],
        "functions": sum(1 for s in syms if s[2] == "func"),
        "objects": sum(1 for s in syms if s[2] == "object"),
        "build_id": build_id(data, sections),
        "compilers": sorted({c.decode("latin-1") for c in comment.split(b"\0") if c}),
        "dates": sorted({d.decode() for d in BUILD_DATE.findall(rodata)}),
    }


def cmd_info(paths):
    for path in paths:
        data = Path(path).read_bytes()
        try:
            s = summarize(data)
        except NotElf as e:
            print(f"{path}: not read ({e})")
            continue
        print(f"{path}")
        print(f"  size {s['size']:,} bytes; .symtab {'yes' if s['symtab'] else 'no'}; "
              f"{s['functions']:,} functions, {s['objects']:,} objects")
        print(f"  DWARF: {', '.join(s['dwarf']) if s['dwarf'] else 'none'}")
        print(f"  build id: {s['build_id'] or 'none'}")
        print(f"  compiler: {'; '.join(s['compilers']) or 'unknown'}")
        print(f"  build dates: {', '.join(s['dates']) or 'none'}")


def filter_symbols(syms, pattern=None, kind="func", readable=False):
    """[(addr, size, kind, shown name, mangled)] matching `pattern` and `kind`."""
    regex = re.compile(pattern, re.IGNORECASE) if pattern else None
    out = []
    for addr, size, sym_kind, name in syms:
        if kind != "all" and sym_kind != kind:
            continue
        shown = demangle(name) if readable else name
        if regex and not regex.search(shown):
            continue
        out.append((addr, size, sym_kind, shown, name))
    return out


def cmd_syms(path, pattern, kind, readable):
    data = Path(path).read_bytes()
    syms = read_symbols(data, read_sections(data))
    if not syms:
        sys.exit(f"{path}: no .symtab (stripped)")
    for addr, size, sym_kind, shown, mangled in filter_symbols(syms, pattern, kind, readable):
        extra = f"\t{mangled}" if readable and shown != mangled else ""
        print(f"0x{addr:08x}\t{size}\t{sym_kind}\t{shown}{extra}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    sub = parser.add_subparsers(dest="command", required=True)
    info = sub.add_parser("info")
    info.add_argument("paths", nargs="+")
    syms = sub.add_parser("syms")
    syms.add_argument("path")
    syms.add_argument("--grep")
    syms.add_argument("--demangle", action="store_true")
    syms.add_argument("--kind", choices=("func", "object", "all"), default="func")
    args = parser.parse_args(argv)
    if args.command == "info":
        cmd_info(args.paths)
    else:
        cmd_syms(args.path, args.grep, args.kind, args.demangle)


if __name__ == "__main__":
    main()
