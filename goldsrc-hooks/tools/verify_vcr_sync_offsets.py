#!/usr/bin/env python3
"""Checks `studio_panel/hook/vcr_sync.rs`'s GameUI table against real files.

`vcr_sync.rs` writes `CDemoPlayerDialog`'s slider and `m_iLastValue` while
the VCR bar is hidden (#663). For each GameUI.dll whose TimeDateStamp and
SizeOfImage match a row of its `BUILDS`, this proves that the dialog's
`OnThink` really is the code the fix mirrors:

  1. It loads the slider from `[this + slider]` and calls slot 0x220
     (SetRange), 0x21c (GetValue) and 0x218 (SetValue) on it.
  2. It compares GetValue's result with `[this + last_value]`, and on a
     difference calls slot 0x58 (SetWorldTime) on `[this + player]` (then
     pauses, directly or through a helper).
  3. It stores the new value back into `[this + last_value]`.

Values are read out of `vcr_sync.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_vcr_sync_offsets.py [GameUI.dll ...]

Defaults to both movie installs. Needs `pip install pefile capstone`.
"""

import re
import sys
from pathlib import Path

try:
    import pefile
    from capstone import Cs, CS_ARCH_X86, CS_MODE_32
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "valve" / "cl_dlls" / "GameUI.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "valve" / "cl_dlls" / "GameUI.dll",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "studio_panel" / "hook" / "vcr_sync.rs"


def builds():
    text = RUST.read_text(encoding="utf-8")
    out = []
    for m in re.finditer(r'name: "([^"]+)",\s*time_date_stamp: 0x([0-9a-f_]+),\s*size_of_image: 0x([0-9a-f_]+),'
                         r'\s*player: 0x([0-9a-f]+),\s*slider: 0x([0-9a-f]+),\s*last_value: 0x([0-9a-f]+)', text):
        name, *nums = m.groups()
        out.append((name, *[int(n.replace("_", ""), 16) for n in nums]))
    if len(out) != 2:
        sys.exit(f"expected 2 builds in {RUST}, found {len(out)}")
    return out


def check(path, rows):
    pe = pefile.PE(str(path), fast_load=True)
    stamp, size = pe.FILE_HEADER.TimeDateStamp, pe.OPTIONAL_HEADER.SizeOfImage
    row = next((r for r in rows if r[1] == stamp and r[2] == size), None)
    if row is None:
        print(f"  FAIL no row for TimeDateStamp {stamp:#x}, SizeOfImage {size:#x}")
        return False
    name, _, _, player, slider, last = row
    image = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    text = next(s for s in pe.sections if s.Characteristics & 0x20000000)
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    md.skipdata = True
    ins = list(md.disasm(image[text.VirtualAddress:text.VirtualAddress + text.Misc_VirtualSize],
                         base + text.VirtualAddress))
    for i, x in enumerate(ins):
        # The comparison with m_iLastValue right after GetValue.
        if not (x.mnemonic == "call" and x.op_str.endswith("+ 0x21c]")):
            continue
        window = ins[max(0, i - 80):i + 40]
        ops = [f"{w.mnemonic} {w.op_str}" for w in window]
        joined = "\n".join(ops)
        need = [
            f"+ {slider:#x}]",
            "+ 0x220]",
            "+ 0x218]",
            f"+ {last:#x}]",
            f"+ {player:#x}]",
            "+ 0x58]",
        ]
        if not all(n in joined for n in need):
            continue
        # `cmp reg, [this + last]` (25th Anniversary), or a load of it right
        # before a `cmp reg, reg` (pre-Anniversary).
        cmp_ok = any(o.startswith("cmp ") and f"+ {last:#x}]" in o for o in ops) or any(
            ops[k].startswith("mov e") and f"+ {last:#x}]" in ops[k] and ops[k + 1].startswith("cmp e")
            for k in range(len(ops) - 1))
        store_ok = any(o.startswith("mov dword ptr [") and f"+ {last:#x}]," in o for o in ops)
        if cmp_ok and store_ok:
            print(f"  ok   {name}: OnThink's slider check at +{x.address - base:#x} uses slider "
                  f"+{slider:#x}, last value +{last:#x}, player +{player:#x}")
            return True
    print(f"  FAIL {name}: no OnThink matching slider +{slider:#x} / last +{last:#x} / player +{player:#x}")
    return False


def main():
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    rows = builds()
    ok = True
    for path in dlls:
        print(path)
        ok &= check(path, rows)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
