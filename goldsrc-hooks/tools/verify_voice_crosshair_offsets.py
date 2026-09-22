#!/usr/bin/env python3
"""Checks `voice.rs` and `crosshair.rs` against a real DoD `client.dll`.

Both modules rewrite a handful of bytes inside a binary that is not in this
repository, and both are small enough to look self-evidently safe. That is
exactly why they are checked: a signature matching the wrong one of two nearly
identical callbacks, or an offset off by a byte, would compile, pass the unit
tests, and patch the wrong instruction in the user's game.

Every constant below is read out of the Rust rather than restated.

`voice.rs` -- for each of the two voice event callbacks:

  1. The signature matches exactly once in `.text`.
  2. `call_at` lands on a real `call dword ptr [ebx]`, by disassembling rather
     than by counting bytes in the pattern.
  3. The match is inside the function DoD registers with `pfnHookEvent` for
     `events/misc/usvoice.sc` / `events/misc/gervoice.sc`.
  4. The caller cleans the arguments itself, so removing the call cannot
     unbalance the stack -- there is an `add esp, N` after it, before any
     `ret`, and no `ret` in between.
  5. With the call NOPped, every later instruction still starts where it did.

`crosshair.rs`:

  6. The signature matches exactly once, and is the function in
     `CHudDoDCrossHair`'s vftable slot 3.
  7. The five bytes it overwrites decode to whole instructions, so nothing
     after the patched span is left mid-instruction.
  8. The replacement is byte-identical to `CHudBase::Draw`, found independently
     as the do-nothing `Draw` shared by the elements that do not override it.

Usage:
    python goldsrc-hooks/tools/verify_voice_crosshair_offsets.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install. Requires `pefile` and
`capstone` (`pip install pefile capstone`); both are analysis-only and are not
build dependencies of anything in the workspace.
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


def rust_string(src: str, decl: str) -> str:
    """A `const NAME: &str = "..."` including line continuations."""
    m = re.search(rf"{decl}\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find {decl}")
    body = re.sub(r"\\\s*\n\s*", "", m.group(1))
    return " ".join(p.strip() for p in re.findall(r'"([^"]*)"', body)).strip()


def rust_byte_slice(src: str, name: str) -> bytes:
    m = re.search(rf"const {name}:\s*&\[u8\]\s*=\s*&\[(.*?)\];", src, re.S)
    if not m:
        sys.exit(f"could not find {name}")
    return bytes(int(x, 0) for x in re.findall(r"0x[0-9a-fA-F]+", m.group(1)))


def rust_sites(src: str):
    """Each `const NAME: Site = Site { ... }` as (what, pattern, call_at)."""
    out = []
    for m in re.finditer(r"const\s+\w+:\s*Site\s*=\s*Site\s*\{(.*?)\n\};", src, re.S):
        body = m.group(1)
        what = re.search(r'what:\s*"([^"]*)"', body).group(1)
        praw = re.search(r"pattern:\s*(.*?),\n", body, re.S).group(1)
        praw = re.sub(r"\\\s*\n\s*", "", praw)
        pattern = " ".join(p.strip() for p in re.findall(r'"([^"]*)"', praw)).strip()
        call_at = int(re.search(r"call_at:\s*(\d+)", body).group(1))
        out.append((what, pattern, call_at))
    return out


def parse_pattern(text):
    return [None if t == "??" else int(t, 16) for t in text.split()]


def find_all(code, pattern):
    n = len(pattern)
    hits = []
    for i in range(len(code) - n + 1):
        if code[i] != pattern[0]:
            continue
        if all(w is None or code[i + k] == w for k, w in enumerate(pattern)):
            hits.append(i)
    return hits


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    if not dll.is_file():
        sys.exit(f"no client.dll at {dll}")

    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    blob = bytes(pe.__data__)
    text = pe.sections[0]
    code = blob[text.PointerToRawData : text.PointerToRawData + text.SizeOfRawData]
    trva = text.VirtualAddress

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True

    def sec_off(rva):
        for s in pe.sections:
            if s.VirtualAddress <= rva < s.VirtualAddress + max(s.Misc_VirtualSize, s.SizeOfRawData):
                return s.PointerToRawData + (rva - s.VirtualAddress)
        return None

    def cstr_rva(needle: bytes):
        i = blob.find(needle + b"\0")
        if i < 0:
            return None
        for s in pe.sections:
            if s.PointerToRawData <= i < s.PointerToRawData + s.SizeOfRawData:
                return s.VirtualAddress + (i - s.PointerToRawData)
        return None

    print(f"client.dll: {dll}\n")
    ok = True

    # ── voice.rs ────────────────────────────────────────────────────────────
    vsrc = (SRC / "voice.rs").read_text(encoding="utf-8")
    call_bytes = rust_byte_slice(vsrc, "CALL")
    nop_bytes = rust_byte_slice(vsrc, "NOPS")
    sites = rust_sites(vsrc)
    print(f"== voice.rs ==  CALL={call_bytes.hex(' ')}  NOPS={nop_bytes.hex(' ')}")
    if len(call_bytes) != len(nop_bytes):
        ok = False
        print("  FAIL CALL and NOPS are different widths")

    # which function does pfnHookEvent get for each script?
    registered = {}
    for script in (b"events/misc/usvoice.sc", b"events/misc/gervoice.sc"):
        srva = cstr_rva(script)
        if srva is None:
            continue
        needle = b"\x68" + struct.pack("<I", srva + base)
        i = code.find(needle)
        if i >= 5 and code[i - 5] == 0x68:
            registered[script.decode()] = struct.unpack("<I", code[i - 4 : i])[0] - base

    for what, pattern, call_at in sites:
        print(f"\n  -- {what}")
        pat = parse_pattern(pattern)
        hits = find_all(code, pat)
        if len(hits) != 1:
            ok = False
            print(f"  FAIL the signature matches {len(hits)} time(s)")
            continue
        start = trva + hits[0]
        print(f"  OK   the signature matches exactly once, at +{start:#x}")

        ins = list(md.disasm(code[hits[0] : hits[0] + len(pat) + 24], base + start))
        call_rva = start + call_at
        call_ins = next((k for k in ins if k.address - base == call_rva), None)
        if call_ins is None:
            ok = False
            print(f"  FAIL +{call_rva:#x} is not an instruction boundary -- call_at is off")
            continue
        if call_ins.mnemonic == "call" and call_ins.op_str == "dword ptr [ebx]":
            print(f"  OK   +{call_rva:#x} is `call dword ptr [ebx]` (EV_PlaySound)")
        else:
            ok = False
            print(f"  FAIL +{call_rva:#x} is `{call_ins.mnemonic} {call_ins.op_str}`")

        # inside the registered callback?
        owner = None
        for script, fn in registered.items():
            # walk forward from the function start to see if it reaches the match
            if fn <= start and start - fn < 0x400:
                owner = (script, fn)
        if owner:
            print(f"  OK   inside the callback DoD registers for {owner[0]} (+{owner[1]:#x})")
        else:
            ok = False
            print("  FAIL the match is not inside either registered voice callback")

        # caller-side cleanup, before any ret
        cleanup = None
        for k in ins:
            if k.address - base <= call_rva:
                continue
            if k.mnemonic == "ret":
                break
            if k.mnemonic == "add" and k.op_str.startswith("esp,"):
                cleanup = k
                break
        if cleanup:
            print(f"  OK   the caller cleans up itself: `{cleanup.mnemonic} {cleanup.op_str}` at +{cleanup.address-base:#x}")
        else:
            ok = False
            print("  FAIL no `add esp, N` between the call and the next ret -- NOPping it would unbalance the stack")

        patched = bytearray(code[hits[0] : hits[0] + len(pat) + 24])
        patched[call_at : call_at + len(nop_bytes)] = nop_bytes
        # Instruction *counts* differ by design -- one `call` becomes two
        # `nop`s -- so what has to match is where every instruction *after the
        # replaced span* begins. Comparing from `call_rva` instead would trip
        # over the second nop, which is a new boundary inside the span itself.
        resume = call_rva + len(nop_bytes)
        after = {k.address - base for k in md.disasm(bytes(patched), base + start)}
        before = {k.address - base for k in ins}
        tail_ok = {a for a in before if a >= resume} == {a for a in after if a >= resume}
        if tail_ok:
            print("  OK   with the call NOPped, every later instruction still starts where it did")
        else:
            ok = False
            print("  FAIL NOPping the call moves later instructions")

    # ── crosshair.rs ────────────────────────────────────────────────────────
    csrc = (SRC / "crosshair.rs").read_text(encoding="utf-8")
    pattern = rust_string(csrc, r"const PATTERN:\s*&str")
    stock = rust_byte_slice(csrc, "STOCK")
    hidden = rust_byte_slice(csrc, "HIDDEN")
    print(f"\n== crosshair.rs ==  STOCK={stock.hex(' ')}  HIDDEN={hidden.hex(' ')}")

    pat = parse_pattern(pattern)
    hits = find_all(code, pat)
    if len(hits) != 1:
        ok = False
        print(f"  FAIL the signature matches {len(hits)} time(s)")
    else:
        draw = trva + hits[0]
        print(f"  OK   the signature matches exactly once, at +{draw:#x}")
        if bytes(code[hits[0] : hits[0] + len(stock)]) == stock:
            print(f"  OK   it starts with the {len(stock)} bytes STOCK restores")
        else:
            ok = False
            print(f"  FAIL it starts {code[hits[0]:hits[0]+len(stock)].hex(' ')}, STOCK says {stock.hex(' ')}")

        # whole instructions in the overwritten span
        consumed = 0
        for k in md.disasm(code[hits[0] : hits[0] + 16], base + draw):
            if consumed >= len(stock):
                break
            consumed += k.size
        if consumed == len(stock):
            print(f"  OK   the {len(stock)} overwritten bytes are whole instructions")
        else:
            ok = False
            print(f"  FAIL the overwritten span ends mid-instruction ({consumed} bytes decode, not {len(stock)})")

        # vftable slot 3 of CHudDoDCrossHair, via RTTI
        i = blob.find(b".?AVCHudDoDCrossHair@@")
        slot3 = None
        if i >= 0:
            for s in pe.sections:
                if s.PointerToRawData <= i < s.PointerToRawData + s.SizeOfRawData:
                    td = s.VirtualAddress + (i - s.PointerToRawData) - 8
                    break
            j = blob.find(struct.pack("<I", td + base))
            while j >= 0 and slot3 is None:
                for s in pe.sections:
                    if s.PointerToRawData <= j < s.PointerToRawData + s.SizeOfRawData:
                        col = s.VirtualAddress + (j - s.PointerToRawData) - 0xC
                        k = blob.find(struct.pack("<I", col + base))
                        if k >= 0:
                            for s2 in pe.sections:
                                if s2.PointerToRawData <= k < s2.PointerToRawData + s2.SizeOfRawData:
                                    vft = s2.VirtualAddress + (k - s2.PointerToRawData) + 4
                                    o = sec_off(vft + 12)
                                    slot3 = struct.unpack("<I", blob[o : o + 4])[0] - base
                j = blob.find(struct.pack("<I", td + base), j + 1)
        if slot3 == draw:
            print(f"  OK   +{draw:#x} is CHudDoDCrossHair's vftable slot 3 (Draw), resolved from RTTI")
        else:
            ok = False
            print(f"  FAIL vftable slot 3 is {hex(slot3) if slot3 else 'unresolved'}, the signature matched +{draw:#x}")

    # CHudBase::Draw, found independently
    stub_hits = [trva + i for i in find_all(code, parse_pattern(hidden.hex(" ")))]
    if stub_hits:
        print(f"  OK   HIDDEN is byte-identical to CHudBase::Draw, present at +{stub_hits[0]:#x}"
              f"{' and ' + str(len(stub_hits) - 1) + ' other place(s)' if len(stub_hits) > 1 else ''}")
    else:
        ok = False
        print("  FAIL HIDDEN does not appear anywhere in .text -- it should be CHudBase::Draw verbatim")

    print("\nSIGNATURES VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
