#!/usr/bin/env python3
"""Checks that `engine.rs`'s `SLOT_HUD_ADD_ENTITY` really is `HUD_AddEntity`.

`engine.rs` already establishes its other three `cldll_func_t` slots
(`Initialize`, `HUD_Frame`, `HUD_GetStudioModelInterface`) by resolving all 43
addresses `F` writes back to their own exports and checking the result lines
up with Xash3D's identically-ordered `cldll_func_src_t`. This does the same
thing independently for slot 20, by disassembling `F` directly rather than
trusting the constant in `engine.rs` -- a wrong slot index would not fail to
compile or fail a unit test, it would swap a *different* function's pointer
for our trampoline and misbehave (or crash) only in the running game.

What this script does **not** establish: `HUD_AddEntity`'s *return-value*
contract (0 = suppress the entity). That's the standard Half-Life SDK
behaviour and is cross-checked against Xash3D's open-source engine in
`hide_sprite.rs`'s module doc, but confirming it against DoD's own `hw.dll`
(closed, no RTTI, heavily obfuscated -- see `docs/goldsrc_hw_dll_survey.md`)
is out of scope here.

Usage:
    python goldsrc-hooks/tools/verify_hide_sprite_slot.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install. Requires `pefile` and
`capstone`; both are analysis-only and are not build dependencies of anything
in the workspace.
"""

import re
import sys
from pathlib import Path

try:
    import pefile
    import capstone  # noqa: F401
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

sys.path.insert(0, str(Path(__file__).resolve().parent))
from survey_client_dll import DEFAULT_DLL, Image  # noqa: E402

RUST = Path(__file__).resolve().parent.parent / "src" / "engine.rs"

# `F`'s own RVA, from docs/goldsrc_client_dll_internals.md §2. Not derived
# here (finding "F" itself needs the export table, done separately below) --
# fixed, because this script's job is to check the *slot table* F builds, not
# to re-find F.
F_RVA = 0x2A700

EXPECTED_EXPORT = "HUD_AddEntity"


def rust_slot(src: str) -> int:
    match = re.search(r"const SLOT_HUD_ADD_ENTITY: usize = (\d+);", src)
    if not match:
        raise SystemExit("could not find `const SLOT_HUD_ADD_ENTITY` in engine.rs")
    return int(match.group(1))


def main(argv: list[str]) -> int:
    dll = Path(argv[0]) if argv else DEFAULT_DLL
    if not dll.is_file():
        print(f"no client.dll at {dll}")
        return 2

    img = Image(dll)
    wanted_slot = rust_slot(RUST.read_text())

    # F builds all 43 function pointers as immediates written to
    # [esp+8]..[esp+8+42*4] before one `rep movsd` copies them to the
    # caller's buffer -- see docs/goldsrc_client_dll_internals.md §2.
    slots: dict[int, int] = {}
    for ins in img.disasm(F_RVA, F_RVA + 0x400):
        m = re.fullmatch(r"dword ptr \[esp \+ (0x[0-9a-f]+)\], (0x[0-9a-f]+)", ins.op_str)
        if ins.mnemonic == "mov" and m:
            esp_off = int(m.group(1), 16)
            if esp_off >= 8:
                slots[(esp_off - 8) // 4] = int(m.group(2), 16)

    if wanted_slot not in slots:
        print(f"FAIL  slot {wanted_slot} was never written as an immediate by F -- disassembly didn't reach it")
        return 1

    target_rva = slots[wanted_slot] - img.base

    pe = pefile.PE(str(dll))
    pe.parse_data_directories()
    exports = {exp.address: exp.name.decode() for exp in pe.DIRECTORY_ENTRY_EXPORT.symbols if exp.name}
    name = exports.get(target_rva)

    print(f"engine.rs SLOT_HUD_ADD_ENTITY = {wanted_slot}")
    print(f"F writes slot {wanted_slot} = {target_rva:#x}, which exports as {name!r}")
    if name == EXPECTED_EXPORT:
        print(f"OK    slot {wanted_slot} really is {EXPECTED_EXPORT}")
        return 0
    print(f"FAIL  expected {EXPECTED_EXPORT!r}, got {name!r}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
