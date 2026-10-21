#!/usr/bin/env python3
"""Checks `voice_menus.rs` against a real DoD `client.dll`.

`voice_menus.rs` reads and writes fields of `gHUD.m_Menu` by offset, finds
`gHUD.m_flTime` and the `+commandmenu` handler by signature, and flips one
byte in that handler. Every constant is read out of the Rust sources
(`voice_menus.rs`, `msglog.rs`'s `ShowMenu` thunk RVA, `hudelement.rs`'s
`menu` vftable RVA) rather than restated.

Checks:

  1. The `ShowMenu` thunk at msglog's RVA starts with `THUNK_START`, and the
     object its `mov ecx` loads has the `CHudMenu` vftable, which RTTI names
     `.?AVCHudMenu@@`.
  2. `SHUTOFF_PATTERN` matches once, inside the handler that thunk calls;
     its operand at `HUD_TIME_AT` is the time `CHudMenu::Draw` compares the
     shut-off field against.
  3. The handler sets `HUD_ACTIVE` in `FLAGS` and sets `DISPLAYED`, and
     stores the shut-off time at `SHUTOFF`.
  4. `CHudMenu::Draw` (vftable slot 3) starts with the time-out branch: it
     reads `SHUTOFF`, clears `DISPLAYED` and bit 0 of `FLAGS`, and returns
     before its first call.
  5. `CHud::Redraw` calls slot 3 only after `test al, 1` on `[ecx+FLAGS]`.
  6. `COMMAND_MENU_PATTERN` matches once; it is the function registered for
     `+commandmenu` (not `-commandmenu`); `JE_AT` is a two-byte `je` whose
     target is the function's own `ret`.

Usage:
    python goldsrc-hooks/tools/verify_voice_menus_offsets.py [--anniversary] [path-to-client.dll]

Defaults to the pre-Anniversary movies install; `--anniversary` picks the
post-Anniversary one. Requires `pefile` and `capstone`.
"""

import re
import struct
import sys
from pathlib import Path

try:
    import capstone
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
PRE = STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll"
POST = STEAM / "Half-Life - POST-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll"
SRC = Path(__file__).resolve().parent.parent / "src"


def rust_str(src: str, name: str) -> str:
    m = re.search(rf'const {name}:\s*&str\s*=\s*"([^"]*)";', src)
    if not m:
        sys.exit(f"could not find {name}")
    return m.group(1)


def rust_int(src: str, name: str) -> int:
    m = re.search(rf"const {name}:\s*(?:usize|u8|u32)\s*=\s*(0[xX][0-9a-fA-F_]+|[0-9_]+);", src)
    if not m:
        sys.exit(f"could not find {name}")
    return int(m.group(1).replace("_", ""), 0)


def rust_bytes(src: str, name: str) -> bytes:
    m = re.search(rf"const {name}:\s*&\[u8\]\s*=\s*&\[(.*?)\];", src, re.S)
    if not m:
        sys.exit(f"could not find {name}")
    return bytes(int(t, 16) for t in re.findall(r"0x([0-9a-fA-F]{2})", m.group(1)))


def parse(text: str):
    return [None if t == "??" else int(t, 16) for t in text.split()]


def find_all(img: bytes, lo: int, hi: int, pattern) -> list[int]:
    rx = b"".join(b"." if b is None else re.escape(bytes([b])) for b in pattern)
    return [lo + m.start() for m in re.finditer(rx, img[lo:hi], re.S)]


def main() -> int:
    args = [a for a in sys.argv[1:] if a != "--anniversary"]
    dll = Path(args[0]) if args else (POST if "--anniversary" in sys.argv else PRE)
    if not dll.is_file():
        sys.exit(f"no client.dll at {dll}")

    vm = (SRC / "voice_menus.rs").read_text(encoding="utf-8")
    msglog = (SRC / "msglog.rs").read_text(encoding="utf-8")
    hud = (SRC / "hudelement.rs").read_text(encoding="utf-8")

    thunk_start = rust_bytes(vm, "THUNK_START")
    shutoff_pattern = parse(rust_str(vm, "SHUTOFF_PATTERN"))
    hud_time_at = rust_int(vm, "HUD_TIME_AT")
    cmd_pattern = parse(rust_str(vm, "COMMAND_MENU_PATTERN"))
    je_at = rust_int(vm, "JE_AT")
    flags_off = rust_int(vm, "FLAGS")
    displayed_off = rust_int(vm, "DISPLAYED")
    shutoff_off = rust_int(vm, "SHUTOFF")
    thunk_rva = int(re.search(r'\("ShowMenu",\s*(0x[0-9a-fA-F]+)\)', msglog).group(1), 16)
    menu_entry = re.search(r'name:\s*"menu",\s*class:\s*"([^"]+)",\s*vftable_rva:\s*(0x[0-9a-fA-F]+)', hud)
    menu_class, menu_vft_rva = menu_entry.group(1), int(menu_entry.group(2), 16)

    pe = pefile.PE(str(dll))
    base = pe.OPTIONAL_HEADER.ImageBase
    img = bytes(pe.get_memory_mapped_image())
    text = next(s for s in pe.sections if s.Name.startswith(b".text"))
    ts, te = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    u32 = lambda rva: struct.unpack_from("<I", img, rva)[0]

    print(f"client.dll: {dll}\n")
    ok = True

    def check(cond: bool, good: str, bad: str):
        nonlocal ok
        print(f"  {'OK  ' if cond else 'FAIL'} {good if cond else bad}")
        ok &= cond
        return cond

    # 1. thunk and instance
    if not check(img[thunk_rva : thunk_rva + len(thunk_start)] == thunk_start,
                 f"the ShowMenu thunk at +{thunk_rva:#x} starts with THUNK_START",
                 f"the ShowMenu thunk at +{thunk_rva:#x} does not start with THUNK_START"):
        return 1
    menu_rva = u32(thunk_rva + len(thunk_start)) - base
    # The object is static and zero on disk; CHud's constructor writes its
    # vftable at load. CHudMenu::Init (slot 1) hands `this` to gHUD's
    # AddHudElem with `mov ecx, &gHUD`, which gives gHUD's address.
    init = u32(menu_vft_rva + 4) - base
    mov = img.find(b"\xB9", init, init + 16)
    ghud = u32(mov + 1) - base
    store = b"\xC7\x86" + struct.pack("<I", menu_rva - ghud) + struct.pack("<I", base + menu_vft_rva)
    check(img.find(store, ts, te) != -1,
          f"its object +{menu_rva:#x} is gHUD (+{ghud:#x}) + {menu_rva - ghud:#x}, and CHud's "
          f"constructor gives it vftable +{menu_vft_rva:#x}",
          f"no constructor writes vftable +{menu_vft_rva:#x} into +{menu_rva:#x}")
    locator = u32(menu_vft_rva - 4) - base
    name_at = u32(locator + 12) - base + 8
    name = img[name_at : img.index(b"\0", name_at)].decode()
    check(name == menu_class, f"RTTI names it {name}", f"RTTI names it {name}, not {menu_class}")
    call_at = thunk_rva + len(thunk_start) + 4
    check(img[call_at] == 0xE8, "the thunk then calls the handler", "no call after the mov ecx")
    handler = call_at + 5 + struct.unpack_from("<i", img, call_at + 1)[0]

    # 2. shut-off pattern
    hits = find_all(img, ts, te, shutoff_pattern)
    if not check(len(hits) == 1, f"SHUTOFF_PATTERN matches once, at +{hits[0]:#x}" if hits else "",
                 f"SHUTOFF_PATTERN matches {len(hits)} time(s)"):
        return 1
    check(handler < hits[0] < handler + 0x140,
          f"inside MsgFunc_ShowMenu (+{handler:#x})",
          f"not inside MsgFunc_ShowMenu (+{handler:#x})")
    hud_time = u32(hits[0] + hud_time_at)

    # 3. the handler's stores
    body = img[handler : handler + 0x140]
    check(bytes([0xD9, 0x5E, shutoff_off]) in body,
          f"it stores the shut-off time at +{shutoff_off:#x}", "no fstp [esi+SHUTOFF]")
    tail = bytes([0x0B, 0xC8, 0x89, 0x46, displayed_off, 0x89, 0x4E, flags_off])
    check(tail in body and bytes([0x8B, 0x4E, flags_off, 0xB8, 1, 0, 0, 0]) in body,
          f"it ORs HUD_ACTIVE into +{flags_off:#x} and sets +{displayed_off:#x}",
          "the flags/displayed stores are not where FLAGS and DISPLAYED say")

    # 4. Draw's time-out branch
    draw = u32(menu_vft_rva + 12) - base
    ins = []
    for i in md.disasm(img[draw : draw + 0x60], base + draw):
        ins.append(i)
        if i.mnemonic in ("ret", "call"):
            break
    text_ins = [f"{i.mnemonic} {i.op_str}" for i in ins]
    check(text_ins[0] == f"fld dword ptr [ecx + {shutoff_off:#x}]",
          "Draw starts by reading the shut-off time", f"Draw starts with {text_ins[0]}")
    check(any(f"[ecx + {hex(hud_time)}]" in t or hex(hud_time) in t for t in text_ins),
          f"and compares it against the HUD time at {hud_time:#x} (the pattern's operand)",
          f"Draw never reads {hud_time:#x}")
    check(ins[-1].mnemonic == "ret"
          and f"mov dword ptr [ecx + {displayed_off:#x}], 0" in text_ins
          and "and al, 0xfe" in text_ins,
          "it clears both fields and returns before its first call",
          f"the time-out branch is not as expected: {text_ins}")

    # 5. Redraw's HUD_ACTIVE gate
    gate = parse(f"8A 41 {flags_off:02X} 75 ?? A8 01 74 ?? F6 46 ?? ?? 75 ?? 8B 11 55 FF 52 0C")
    hits = find_all(img, ts, te, gate)
    check(len(hits) == 1, f"CHud::Redraw tests bit 0 of +{flags_off:#x} before Draw (+{hits[0]:#x})" if hits else "",
          f"the Redraw gate matches {len(hits)} time(s)")

    # 6. +commandmenu
    hits = find_all(img, ts, te, cmd_pattern)
    if not check(len(hits) == 1, f"COMMAND_MENU_PATTERN matches once, at +{hits[0]:#x}" if hits else "",
                 f"COMMAND_MENU_PATTERN matches {len(hits)} time(s)"):
        return 1
    fn = hits[0]

    def registered(command: str):
        s = img.find(command.encode() + b"\0")
        push = b"\x68" + struct.pack("<I", base + s)
        at = img.find(push, ts, te)
        return u32(at - 4) - base if at > 5 and img[at - 5] == 0x68 else None

    check(registered("+commandmenu") == fn, "it is the function registered for +commandmenu",
          f"+commandmenu is registered to {registered('+commandmenu')}")
    minus = registered("-commandmenu")
    check(minus is not None and minus != fn, f"-commandmenu is another function (+{minus:#x})",
          "-commandmenu is the same function")
    je = next(md.disasm(img[fn + je_at : fn + je_at + 2], base + fn + je_at))
    target = int(je.op_str, 16) - base
    check(je.mnemonic == "je" and je.size == 2 and img[target] == 0xC3,
          f"+{fn + je_at:#x} is `je` to the function's own ret (+{target:#x})",
          f"+{fn + je_at:#x} is `{je.mnemonic} {je.op_str}`")

    print("\nall checks passed" if ok else "\nSOME CHECKS FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
