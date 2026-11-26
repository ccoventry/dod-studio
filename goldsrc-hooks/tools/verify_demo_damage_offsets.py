#!/usr/bin/env python3
"""Checks `demo_damage.rs`'s two detours against a real `hw.dll`.

`demo_damage.rs` writes a jump over the first nine bytes of `Host_EndGame`
and `Host_Error`, and its stub hands the format string to Rust before the
function runs (issue #1373). The unit tests prove the stub assembles as
documented; only the binary can prove the facts it relies on:

  0. Exactly one of the builds in `BUILDS` matches this `hw.dll` (both of its
     signatures), and the other build's match nothing.
  1. Each signature matches exactly once and starts with the stolen bytes,
     which are `push ebp; mov ebp, esp; sub esp, imm32`: nothing relative,
     nothing that reads the flags.
  2. The function the `Host_EndGame` signature finds is the one that prints
     "Host_EndGame: %s", and the `Host_Error` one prints "Host_Error: %s"
     and "Host_Error: recursively entered".
  3. Both read their arguments as `void (const char *fmt, ...)`: `fmt` is
     `[ebp+8]` and `va_start` is `lea r, [ebp+0xc]` -- what the stub passes.
  4. Nothing branches into the interior of either span, and no relocated
     dword points into it.
  5. Every damage reason in `REASONS`, and every entity-limit end in
     `LIMITS`, is a format string in this `hw.dll`, passed to the function
     the table names, and "End of demo." -- a healthy `playdemo` end -- goes
     to `Host_EndGame` and is in neither.

Every constant comes out of `demo_damage.rs` rather than being restated here,
for the reason in `verify_deathmsg_offsets.py`.

Usage:
    python goldsrc-hooks/tools/verify_demo_damage_offsets.py [path-to-hw.dll]
    python goldsrc-hooks/tools/verify_demo_damage_offsets.py --anniversary

Defaults to the pre-Anniversary movies install; `--anniversary` is the
POST-Anniversary movies install. Needs `pip install pefile capstone`.
"""

import re
import struct
import sys
from pathlib import Path

try:
    import pefile
    import capstone
    from capstone import x86
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLL = STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll"
ANNIVERSARY_DLL = STEAM / "Half-Life - POST-Anniversary for Movies" / "hw.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "demo_damage.rs"

FUNCTIONS = {"EndGame": "Host_EndGame", "Error": "Host_Error"}
PRINTS = {
    "EndGame": [b"Host_EndGame: %s\n"],
    "Error": [b"Host_Error: %s\n", b"Host_Error: recursively entered"],
}


def rust_builds(src: str):
    """[(name, end_game pattern, error pattern, stolen bytes)] from `BUILDS`."""
    table = re.search(r"const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S)
    if not table:
        raise SystemExit("could not find `const BUILDS` in demo_damage.rs")
    builds = []
    for name, end_game, error, stolen in re.findall(
        r'name: "([^"]+)",\s*end_game: "(.*?)",\s*error: "(.*?)",\s*stolen: \[([^\]]*)\]',
        table.group(1),
        re.S,
    ):
        tidy = lambda p: " ".join(p.replace("\\", " ").split())
        builds.append(
            (name, tidy(end_game), tidy(error), bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", stolen)))
        )
    if not builds:
        raise SystemExit("`BUILDS` in demo_damage.rs has no entries this script can read")
    return builds


def rust_reasons(src: str, name: str = "REASONS"):
    """[(function key, format start, reason key)] from `REASONS` (or `LIMITS`)."""
    table = re.search(rf"const {name}: \[\(Which, &str, &str\); \d+\] = \[(.*?)\];\n", src, re.S)
    if not table:
        raise SystemExit(f"could not find `const {name}` in demo_damage.rs")
    found = re.findall(r'\(\s*Which::(\w+),\s*"([^"]+)",\s*"([^"]+)",?\s*\)', table.group(1))
    if not found:
        raise SystemExit(f"`{name}` in demo_damage.rs has no entries this script can read")
    return found


def rust_const(src: str, name: str) -> str:
    match = re.search(rf"const {name}: [^=]+= (.*?);", src, re.S)
    if not match:
        raise SystemExit(f"could not find `const {name}` in demo_damage.rs")
    return match.group(1)


def signature_matches(image, pattern):
    rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()), re.S)
    return [m.start() for m in rx.finditer(image)]


def code_ranges(pe):
    for section in pe.sections:
        if section.Characteristics & 0x20000000:  # IMAGE_SCN_MEM_EXECUTE
            yield section.VirtualAddress, section.VirtualAddress + max(section.Misc_VirtualSize, section.SizeOfRawData)


def branch_targets(pe, image, base, md):
    """{target rva: [(source rva, mnemonic)]} for every direct branch in code."""
    found = {}
    for lo, hi in code_ranges(pe):
        for ins in md.disasm(image[lo:hi], base + lo):
            if ins.id == 0 or not ({x86.X86_GRP_JUMP, x86.X86_GRP_CALL} & set(ins.groups)):
                continue
            for op in ins.operands:
                if op.type == x86.X86_OP_IMM:
                    found.setdefault(op.imm - base, []).append((ins.address - base, ins.mnemonic))
    return found


def relocated_pointers(pe, image, base):
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_BASERELOC"]])
    found = {}
    for block in getattr(pe, "DIRECTORY_ENTRY_BASERELOC", []):
        for entry in block.entries:
            if entry.type == 3:
                value = int.from_bytes(image[entry.rva : entry.rva + 4], "little")
                found.setdefault(value - base, []).append(entry.rva)
    return found


def function_body(image, base, md, start, limit=0x200):
    """Instructions from `start` for `limit` bytes (enough for both functions'
    printing part; neither returns, so there is no `ret` to stop at)."""
    return list(md.disasm(image[start : start + limit], base + start))


def string_rva(image, text: bytes):
    """Every rva where `text` starts right after a NUL (a whole string)."""
    out, i = [], image.find(text)
    while i >= 0:
        if image[i - 1] == 0:
            out.append(i)
        i = image.find(text, i + 1)
    return out


def callers_with_push(pe, image, base, md, string_rvas):
    """[(push rva, call target rva)] for every `push <string>` followed within
    a few instructions by a direct call."""
    found = []
    for lo, hi in code_ranges(pe):
        for s in string_rvas:
            needle = b"\x68" + struct.pack("<I", base + s)
            i = image.find(needle, lo, hi)
            while i >= 0:
                for ins in md.disasm(image[i : i + 40], base + i):
                    if ins.mnemonic == "call" and ins.operands and ins.operands[0].type == x86.X86_OP_IMM:
                        found.append((i, ins.operands[0].imm - base))
                        break
                i = image.find(needle, i + 1, hi)
    return found


def main() -> int:
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    dll = ANNIVERSARY_DLL if arg == "--anniversary" else Path(arg) if arg else DEFAULT_DLL
    if not dll.is_file():
        return print(f"no hw.dll at {dll}") or 2

    src = RUST.read_text(encoding="utf-8")
    builds = rust_builds(src)
    reasons = rust_reasons(src)
    limits = rust_reasons(src, "LIMITS")
    stolen_len = int(rust_const(src, "STOLEN_LEN"), 0)

    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = bytes(pe.get_memory_mapped_image())
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    md.skipdata = True
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"== {dll} ==")
    # -- 0. which build ------------------------------------------------------
    hits = {}
    for name, end_game, error, _ in builds:
        hits[name] = {"EndGame": signature_matches(image, end_game), "Error": signature_matches(image, error)}
        print(f"     {name}: Host_EndGame {[hex(m) for m in hits[name]['EndGame']]}, "
              f"Host_Error {[hex(m) for m in hits[name]['Error']]}")
    matching = [b for b in builds if hits[b[0]]["EndGame"] or hits[b[0]]["Error"]]
    check(len(matching) == 1, f"{len(matching)} build(s) match; exactly one should")
    if len(matching) != 1:
        return 1
    name, _, _, stolen = matching[0]
    print(f"     build: {name}")
    check(len(stolen) == stolen_len, f"stolen bytes are STOLEN_LEN ({stolen_len})")

    targets = None
    pointers = relocated_pointers(pe, image, base)
    found = {}
    for key, fn in FUNCTIONS.items():
        matches = hits[name][key]
        # -- 1. unique, starts with the stolen bytes --------------------------
        check(len(matches) == 1, f"{fn}: signature matches once ({len(matches)})")
        if len(matches) != 1:
            continue
        at = matches[0]
        found[key] = at
        check(image[at : at + stolen_len] == stolen, f"{fn} at +{at:#x}: starts with the stolen bytes")
        head = list(md.disasm(image[at : at + stolen_len], base + at))
        shapes = [(i.mnemonic, i.op_str) for i in head]
        check(
            len(shapes) == 3
            and shapes[0] == ("push", "ebp")
            and shapes[1] == ("mov", "ebp, esp")
            and shapes[2][0] == "sub"
            and shapes[2][1].startswith("esp, "),
            f"{fn}: stolen span is push ebp; mov ebp, esp; sub esp, imm ({shapes})",
        )
        # -- 2. it is the function that prints its own name ------------------
        body = function_body(image, base, md, at)
        pushed = {op.imm - base for i in body if i.mnemonic == "push" for op in i.operands if op.type == x86.X86_OP_IMM}
        for text in PRINTS[key]:
            rvas = string_rva(image, text)
            check(any(r in pushed for r in rvas), f"{fn}: pushes {text!r}")
        # -- 3. (fmt, ...) ---------------------------------------------------
        ops = [(i.mnemonic, i.op_str) for i in body[:40]]
        check(
            any(m in ("mov", "push") and "dword ptr [ebp + 8]" in o for m, o in ops),
            f"{fn}: reads fmt from [ebp+8]",
        )
        check(any(m == "lea" and o.endswith("[ebp + 0xc]") for m, o in ops), f"{fn}: va_start is [ebp+0xc]")
        # -- 4. nothing lands inside the span --------------------------------
        if targets is None:
            targets = branch_targets(pe, image, base, md)
        inside = {t: s for t, s in targets.items() if at < t < at + stolen_len}
        check(not inside, f"{fn}: no branch into +{at + 1:#x}..+{at + stolen_len:#x} {inside or ''}")
        relocs = {t: s for t, s in pointers.items() if at < t < at + stolen_len}
        check(not relocs, f"{fn}: no relocated pointer into the span {relocs or ''}")
        entry_calls = len([s for s in targets.get(at, []) if s[1] == "call"])
        print(f"     {fn}: {entry_calls} direct call(s) to its entry")

    # -- 5. the reasons --------------------------------------------------------
    for key, start, reason in reasons + limits:
        rvas = [r for r in string_rva(image, start.encode())]
        calls = callers_with_push(pe, image, base, md, rvas)
        to = {t for _, t in calls}
        check(
            rvas and key in found and to == {found[key]},
            f"{reason}: {start!r} is passed to {FUNCTIONS[key]} "
            f"(sites {[hex(p) for p, _ in calls]})",
        )
    end = string_rva(image, b"End of demo.\n")
    end_calls = callers_with_push(pe, image, base, md, end)
    check(
        end_calls and {t for _, t in end_calls} == {found.get("EndGame")},
        f"'End of demo.' goes to Host_EndGame (sites {[hex(p) for p, _ in end_calls]})",
    )
    check(not any("End of demo".startswith(s) or s.startswith("End of demo") for _, s, _ in reasons + limits),
          "'End of demo.' is not a damage reason or an entity-limit end")

    print("all checks passed" if ok else "SOME CHECKS FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
