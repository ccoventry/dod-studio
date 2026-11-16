#!/usr/bin/env python3
"""Checks `env_models.rs`'s patterns against real `client.dll` files.

`env_models.rs` calls DoD's own `CClientEnvModel::Think` on the object
`Parse_env_model` hands `AddEnvModel`. The unit tests prove the module reads
the call site the way it means to; only the binaries can prove:

  1. Each pattern matches exactly once in `.text`.
  2. The call site's `call` lands on `AddEnvModel`, and the object it passes
     (`mov ecx, imm32`) is inside the image.
  3. `AddEnvModel` keeps the count at `this + COUNT_OFFSET` and stops at 192;
     `Think` reads the carrier at `this + CARRIER_OFFSET` and stores a new one
     there at `Think + THINK_STORE_AT`.
  4. `StudioDrawModel` reads the count at `object + COUNT_OFFSET` and the
     entries at `object + 0x18`, so the object is the one the renderer
     draws from.

Constants and patterns are read out of `env_models.rs` rather than restated.

Usage:
    python -I goldsrc-hooks/tools/verify_env_models_offsets.py [client.dll ...]

Defaults to both movie installs. Needs `pip install pefile`.
"""

import re
import struct
import sys
from pathlib import Path

try:
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "env_models.rs"


def constants():
    text = RUST.read_text(encoding="utf-8")
    out = {}
    for name in ("OBJECT_AT", "CALL_REL_AT", "CALL_END", "THINK_STORE_AT", "CARRIER_OFFSET", "COUNT_OFFSET"):
        m = re.search(rf"const {name}: usize = (0x[0-9a-f_]+|\d+);", text)
        if not m:
            sys.exit(f"{name} not found in {RUST}")
        out[name] = int(m.group(1).replace("_", ""), 0)
    for name in ("ADD_CALL_PATTERN", "ADD_PATTERN", "THINK_PATTERN", "THINK_STORE_PATTERN"):
        m = re.search(rf'const {name}: &str =\s*"([0-9A-F? ]+)";', text)
        if not m:
            sys.exit(f"{name} not found in {RUST}")
        out[name] = m.group(1)
    return out


def regex(pattern):
    return re.compile(
        b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()),
        re.S,
    )


def check(path, c):
    pe = pefile.PE(str(path))
    base = pe.OPTIONAL_HEADER.ImageBase
    size = pe.OPTIONAL_HEADER.SizeOfImage
    img = pe.get_memory_mapped_image()
    text = next(s for s in pe.sections if s.Name.startswith(b".text"))
    t0 = text.VirtualAddress
    code = img[t0 : t0 + text.Misc_VirtualSize]
    problems = []
    found = {}
    for name in ("ADD_CALL_PATTERN", "ADD_PATTERN", "THINK_PATTERN", "THINK_STORE_PATTERN"):
        hits = [t0 + m.start() for m in regex(c[name]).finditer(code)]
        if len(hits) != 1:
            problems.append(f"{name} matches {len(hits)} times")
        else:
            found[name] = hits[0]
    if problems:
        return problems
    site, add, think = found["ADD_CALL_PATTERN"], found["ADD_PATTERN"], found["THINK_PATTERN"]
    rel = struct.unpack_from("<i", img, site + c["CALL_REL_AT"])[0]
    if site + c["CALL_END"] + rel != add:
        problems.append(f"the call at +{site:#x} goes to +{site + c['CALL_END'] + rel:#x}, not AddEnvModel +{add:#x}")
    obj = struct.unpack_from("<I", img, site + c["OBJECT_AT"])[0] - base
    if not 0 < obj < size - c["COUNT_OFFSET"] - 4:
        problems.append(f"object +{obj:#x} is outside the image")
    if found["THINK_STORE_PATTERN"] != think + c["THINK_STORE_AT"]:
        problems.append("Think's carrier store is not at THINK_STORE_AT")
    count = struct.pack("<I", c["COUNT_OFFSET"])
    if img[add + 8 : add + 12] != count:
        problems.append("AddEnvModel does not read the count at COUNT_OFFSET")
    # Think: `mov ecx, [esi + CARRIER_OFFSET]` and the store `mov [esi + CARRIER_OFFSET], eax`.
    if img[think + 6 : think + 9] != bytes([0x8B, 0x4E, c["CARRIER_OFFSET"]]):
        problems.append("Think does not read the carrier at CARRIER_OFFSET")
    if img[think + c["THINK_STORE_AT"] : think + c["THINK_STORE_AT"] + 3] != bytes([0x89, 0x46, c["CARRIER_OFFSET"]]):
        problems.append("Think does not store the carrier at CARRIER_OFFSET")
    # StudioDrawModel's loop (`client+0x4a3a9`): `mov eax, [object+COUNT]`,
    # then the entries' address, `object+0x18`, as an operand within 0x40.
    count_load = b"\xa1" + struct.pack("<I", base + obj + c["COUNT_OFFSET"])
    entries = struct.pack("<I", base + obj + 0x18)
    loads = [m.start() for m in re.finditer(re.escape(count_load), code)]
    loads = [at for at in loads if entries in code[at : at + 0x40]]
    if not loads:
        problems.append("no draw loop reading the count and the entries found")
    if not problems:
        print(f"  object +{obj:#x}, AddEnvModel +{add:#x}, Think +{think:#x}, call site +{site:#x}, draw loop at +{t0 + loads[0]:#x}")
    return problems


def main():
    c = constants()
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    bad = 0
    for dll in dlls:
        print(dll)
        if not dll.exists():
            print("  missing")
            bad += 1
            continue
        problems = check(dll, c)
        for p in problems:
            print("  PROBLEM:", p)
        print("  OK" if not problems else "  FAILED")
        bad += bool(problems)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
