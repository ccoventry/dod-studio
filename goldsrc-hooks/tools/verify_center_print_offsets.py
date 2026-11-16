"""Checks the engine's `pfnCenterPrint` (cl_enginefunc_t slot 31) that
`src/demo_damage/tell.rs` calls for the damaged-demo notice (#1373).

For each build it checks, in that build's hw.dll:

- the wrapper at the table's offset starts `push ebp; mov ebp, esp; lea
  eax, [ebp+8]` and calls `SCR_CenterPrint` (whose offset
  `src/engine_center_print.rs` already names) with its one argument, the
  text, unformatted;
- one pointer to it sits in .data, and that pointer is slot 31 of a table:
  the dword 32 slots before it is not a code pointer (the table's start)
  while slots 0 to 31 are, and slot 30 is a function too (`pfnConsolePrint`).

Usage: python -I verify_center_print_offsets.py [--anniversary] [hw.dll]
"""

import os
import struct
import sys

import pefile

PRE = r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\hw.dll"
POST = r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - POST-Anniversary for Movies\hw.dll"

# (wrapper, SCR_CenterPrint) -- must match tell.rs's BUILDS and
# engine_center_print.rs's table.
BUILDS = {
    "pre-Anniversary": (0xB0E0, 0x4BA90),
    "25th Anniversary": (0x195480, 0x24CDE0),
}
SLOT = 31


def check(path, build):
    wrapper, centre = BUILDS[build]
    pe = pefile.PE(path, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = pe.get_memory_mapped_image()
    text = next(s for s in pe.sections if s.Name.startswith(b".text"))
    t0, t1 = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize

    def is_code(v):
        return t0 <= v - base < t1

    ok = True
    head = image[wrapper:wrapper + 6]
    if head != bytes.fromhex("558bec8d4508"):
        print(f"  FAIL wrapper +{wrapper:#x} starts {head.hex()}")
        ok = False
    calls = [i for i in range(wrapper, wrapper + 48)
             if image[i] == 0xE8 and i + 5 + struct.unpack_from("<i", image, i + 1)[0] == centre]
    if not calls:
        print(f"  FAIL wrapper +{wrapper:#x} never calls SCR_CenterPrint +{centre:#x}")
        ok = False
    needle = struct.pack("<I", base + wrapper)
    refs = []
    at = image.find(needle)
    while at != -1:
        refs.append(at)
        at = image.find(needle, at + 1)
    if len(refs) != 1:
        print(f"  FAIL {len(refs)} pointers to the wrapper, want 1: {[hex(r) for r in refs]}")
        return False
    start = refs[0] - SLOT * 4
    slots = struct.unpack_from(f"<{SLOT + 1}I", image, start)
    before = struct.unpack_from("<I", image, start - 4)[0]
    if not all(is_code(v) for v in slots):
        print(f"  FAIL not every slot 0..{SLOT} of the table at +{start:#x} is code")
        ok = False
    if is_code(before):
        print(f"  FAIL the dword before the table at +{start:#x} is code too: slot 31 is not where it seems")
        ok = False
    print(f"  table +{start:#x}, slot 31 at +{refs[0]:#x} -> +{wrapper:#x} (calls SCR_CenterPrint at +{calls[0]:#x} if found), "
          f"slot 30 -> +{slots[30] - base:#x}")
    return ok


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    anniversary = "--anniversary" in sys.argv
    build = "25th Anniversary" if anniversary else "pre-Anniversary"
    path = args[0] if args else (POST if anniversary else PRE)
    if not os.path.isfile(path):
        print(f"no hw.dll at {path}")
        return 2
    print(f"{build}: {path}")
    good = check(path, build)
    print("OK" if good else "FAILED")
    return 0 if good else 1


if __name__ == "__main__":
    sys.exit(main())
