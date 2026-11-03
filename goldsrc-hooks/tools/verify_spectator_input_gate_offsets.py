#!/usr/bin/env python3
"""Checks `spectator_input_gate.rs` against a real DoD `client.dll` (#1296).

The module writes 0 into `CHudSpectator::m_flNextObserverInput` when it is
stale. A wrong address would write a float into some other variable:
compiled fine, wrong in game. Every constant is read out of the Rust.

  1. `OBJECT_PATTERN` matches once in `.text`, and its `call` reaches
     `HandleButtonsDown` (found with `spectator_buttons.rs`'s pattern).
  2. `GATE_PATTERN` matches once, inside that function (`GATE_WITHIN`), and
     is `fld dword [esi+disp32]` with esi = `this` (`mov esi, ecx` before it).
  3. The function stores the same field with `fstp dword [esi+disp32]` after
     adding 0.2 to the clock.

Usage:
    python goldsrc-hooks/tools/verify_spectator_input_gate_offsets.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install; `client.dll` is the same file
in the 25th Anniversary one (run it on both). Requires `pefile` and `capstone`.
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

DEFAULT_DLL = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\dod\cl_dlls\client.dll"
)
SRC = Path(__file__).resolve().parent.parent / "src"


def rust_string(src, decl):
    m = re.search(rf"{decl}\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find {decl}")
    body = re.sub(r"\\\s*\n\s*", "", m.group(1))
    return " ".join(p.strip() for p in re.findall(r'"([^"]*)"', body)).strip()


def rust_int(src, name):
    m = re.search(rf"const {name}:\s*usize\s*=\s*(\w+);", src)
    if not m:
        sys.exit(f"could not find {name}")
    return int(m.group(1), 0)


def find_all(code, pattern):
    want = [None if t == "??" else int(t, 16) for t in pattern.split()]
    return [i for i in range(len(code) - len(want) + 1)
            if code[i] == want[0] and all(w is None or code[i + k] == w for k, w in enumerate(want))]


def main():
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    gate_src = (SRC / "spectator_input_gate.rs").read_text(encoding="utf-8")
    buttons_src = (SRC / "spectator_buttons.rs").read_text(encoding="utf-8")
    pe = pefile.PE(str(dll))
    text = next(s for s in pe.sections if s.Name.rstrip(b"\0") == b".text")
    code = text.get_data()
    rva0 = text.VirtualAddress
    failures = []

    def unique(name, pattern):
        hits = find_all(code, pattern)
        if len(hits) != 1:
            failures.append(f"{name} matches {len(hits)} times")
            return None
        return hits[0]

    function = unique("spectator_buttons PATTERN", rust_string(buttons_src, "const PATTERN: &str"))
    call = unique("OBJECT_PATTERN", rust_string(gate_src, "const OBJECT_PATTERN: &str"))
    test = unique("GATE_PATTERN", rust_string(gate_src, "const GATE_PATTERN: &str"))
    if None in (function, call, test):
        print("\n".join(failures))
        return 1

    rel_at = rust_int(gate_src, "CALL_REL_AT")
    rel = struct.unpack_from("<i", code, call + rel_at)[0]
    if call + rel_at + 4 + rel != function:
        failures.append("OBJECT_PATTERN's call does not reach HandleButtonsDown")

    within = rust_int(gate_src, "GATE_WITHIN")
    if not function <= test < function + within:
        failures.append(f"GATE_PATTERN at +{rva0 + test:#x} is outside HandleButtonsDown")
    disp = struct.unpack_from("<I", code, test + rust_int(gate_src, "GATE_OFFSET_AT"))[0]

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    body = list(md.disasm(code[function:function + 0x170], rva0 + function))
    if not any(i.mnemonic == "mov" and i.op_str == "esi, ecx" for i in body[:8]):
        failures.append("HandleButtonsDown does not keep `this` in esi")
    load = f"dword ptr [esi + {disp:#x}]"
    stores = [i for i in body if i.mnemonic == "fstp" and i.op_str == load]
    if len(stores) != 1:
        failures.append(f"expected one `fstp {load}`, found {len(stores)}")
    fadds = [i for i in body if i.mnemonic == "fadd" and stores and i.address < stores[0].address]
    if not fadds:
        failures.append("no fadd before the store")
    else:
        const = int(re.search(r"0x[0-9a-f]+", fadds[-1].op_str).group(0), 16)
        value = struct.unpack("<d", pe.get_data(const - pe.OPTIONAL_HEADER.ImageBase, 8))[0]
        if abs(value - 0.2) > 1e-9:
            failures.append(f"the store adds {value}, not 0.2")

    if failures:
        print("FAIL\n" + "\n".join(failures))
        return 1
    print(f"OK: {dll}\n  HandleButtonsDown +{rva0 + function:#x}, +attack's call +{rva0 + call:#x}, "
          f"gate test +{rva0 + test:#x}, m_flNextObserverInput at this+{disp:#x}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
