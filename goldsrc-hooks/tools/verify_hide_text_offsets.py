#!/usr/bin/env python3
"""Checks #677's patches against a real DoD `client.dll`: `spectator_text.rs`,
`overview_marker.rs`'s camera-icon skip, `overview_speaker.rs`, and the
centre-print object `center_text.rs` relies on `clan_text.rs` to clear.

Every pattern, offset and byte string is read out of the Rust rather than
restated, and each patch is checked for what makes it safe, not only for
where it lands.

`spectator_text.rs`, the camera mode name:

  1. The signature matches exactly once, a few instructions after
     `push "#Spec_Mode%d"` (so it is `CHudSpectator::SetMode`).
  2. The patched span is one `call`, to the function the `TextMsg` message
     thunk (`msglog.rs`'s table) calls: `MsgFunc_TextMsg`.
  3. Every `ret` in that function is `ret 0xc`, and exactly three arguments
     are pushed for this call, so `add esp, 0xc` takes off what it would have.
  4. The replacement decodes to `add esp, 0xc` and `nop`s, the same width.
  5. Nothing after the call reads `eax` or the flags before the next call.

`spectator_text.rs`, "Press DUCK for Spectator Menu":

  6. The signature matches exactly once, at a function that names
     `Spec_Duck` in its `client_textmessage_t`.
  7. Every call to it pushes `"#Spec_Duck"` and pops its argument itself
     (`add esp, 4` or `pop ecx`), so a `ret` there leaves the stack as it was.

`overview_marker.rs`:

  8. The signature matches once; `JE_AT` and `SKIP_AT` are short `je`s; the
     skip follows `test esi, esi`, the draw after it starts with `fld`, and its
     target is the function's epilogue (no x87 instruction before the `ret`).
     `TEAM_OPERAND_AT` is the address operand of a 7-byte `cmp [imm32], 2`,
     the team test #907 points at the watched player's team.

`overview_speaker.rs`:

  9. The signature matches once; the span is `STOCK`, two whole instructions,
     5 bytes; the `jne` at `JNE_AT` targets the next-entry code.
 10. The block between the `jne` and that target draws the sprite
     `CHudSpectator::VidInit` loads from `speakerIcon.spr` (the instance in the
     pattern's `mov ecx, imm32` plus the member `VidInit` stores it in).

`center_text.rs` / `clan_text.rs`:

 11. `MsgFunc_TextMsg`'s destination-4 case posts to the same object whose
     hold `clan_text::end_center_line` clears.

All six patched spans:

 12. Nothing in `.text` branches into the middle of one.

Usage:
    python goldsrc-hooks/tools/verify_hide_text_offsets.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install. Requires `pefile` and
`capstone` (`pip install pefile capstone`).
"""

import hashlib
import re
import struct
import sys
from pathlib import Path

try:
    import capstone
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

DEFAULT_DLL = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\dod\cl_dlls\client.dll"
)
SRC = Path(__file__).resolve().parent.parent / "src"


def join_rust_string(raw: str) -> str:
    raw = re.sub(r"\\\s*\n\s*", " ", raw)
    return " ".join(" ".join(re.findall(r'"([^"]*)"', raw)).split())


def rust_str_const(src: str, name: str) -> str:
    m = re.search(rf"const {name}:\s*&str\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find const {name}")
    return join_rust_string(m.group(1))


def rust_usize(src: str, name: str) -> int:
    m = re.search(rf"const {name}:\s*usize\s*=\s*(\d+);", src)
    if not m:
        sys.exit(f"could not find const {name}")
    return int(m.group(1))


def rust_bytes(text: str) -> bytes:
    return bytes(int(x, 16) for x in re.findall(r"0x([0-9a-fA-F]{1,2})\b", text))


def rust_site(src: str, name: str):
    m = re.search(rf"const {name}: Site = Site \{{(.*?)\n\}};", src, re.S)
    if not m:
        sys.exit(f"could not find Site {name}")
    body = m.group(1)
    pattern = join_rust_string(re.search(r"pattern:\s*(.*?),\n\s*at:", body, re.S).group(1))
    at = int(re.search(r"\bat:\s*(\d+)", body).group(1))
    hidden = rust_bytes(re.search(r"hidden:\s*&\[(.*?)\]", body, re.S).group(1))
    return pattern, at, hidden


def parse_pattern(text):
    return [None if t == "??" else int(t, 16) for t in text.split()]


class Image:
    def __init__(self, path: Path):
        self.pe = pefile.PE(str(path), fast_load=True)
        self.img = self.pe.get_memory_mapped_image()
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        text = self.pe.sections[0]
        self.lo = text.VirtualAddress
        self.hi = text.VirtualAddress + text.Misc_VirtualSize
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self.md.detail = True
        self._sweep = None

    def find_all(self, pattern):
        pat = parse_pattern(pattern)
        code = self.img[self.lo : self.hi]
        out = []
        first = pat[0]
        i = code.find(bytes([first]))
        while i >= 0 and i + len(pat) <= len(code):
            if all(w is None or code[i + k] == w for k, w in enumerate(pat)):
                out.append(self.lo + i)
            i = code.find(bytes([first]), i + 1)
        return out

    def dis(self, rva, size):
        return list(self.md.disasm(self.img[rva : rva + size], self.base + rva))

    def one(self, rva):
        return next(self.md.disasm(self.img[rva : rva + 16], self.base + rva))

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

    def cstr_rva(self, text: bytes):
        i = self.img.find(text + b"\0")
        while i > 0 and self.img[i - 1] != 0:
            i = self.img.find(text + b"\0", i + 1)
        return i if i >= 0 else None

    def target(self, ins):
        """The RVA a direct call/jump goes to, or None."""
        op = ins.op_str
        if op.startswith("0x"):
            return int(op, 16) - self.base
        return None

    def sweep(self):
        """Every instruction in .text by linear sweep (data decodes as junk,
        which is why only direct branches are taken from it)."""
        if self._sweep is None:
            md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
            md.skipdata = True
            self._sweep = [
                (i.address - self.base, i.size, i.mnemonic, i.op_str)
                for i in md.disasm(self.img[self.lo : self.hi], self.base + self.lo)
            ]
        return self._sweep

    def branches_into(self, start, end):
        """Direct calls and jumps landing strictly inside (start, end)."""
        hits = []
        for addr, _size, mnem, op in self.sweep():
            if (mnem == "call" or mnem.startswith("j")) and op.startswith("0x"):
                t = int(op, 16) - self.base
                if start < t < end:
                    hits.append((addr, mnem, t))
        return hits

    def callers(self, rva):
        return [a for a, _s, m, op in self.sweep() if m == "call" and op.startswith("0x") and int(op, 16) - self.base == rva]

    def function_rets(self, start, limit=0x800):
        """The `ret`s of the function at `start`: walks forward, following the
        furthest forward branch target, and stops at a ret past all of them."""
        rets = []
        furthest = start
        for ins in self.dis(start, limit):
            rva = ins.address - self.base
            t = self.target(ins)
            if t is not None and ins.mnemonic.startswith("j") and t > furthest:
                furthest = t
            if ins.mnemonic == "ret":
                rets.append((rva, ins.op_str))
                if rva >= furthest:
                    break
        return rets


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    if not dll.is_file():
        sys.exit(f"no client.dll at {dll}")
    im = Image(dll)
    print(f"client.dll: {dll}")
    print(f"sha256: {hashlib.sha256(dll.read_bytes()).hexdigest()}\n")
    ok = True

    def check(cond, good, bad):
        nonlocal ok
        print(f"  {'OK  ' if cond else 'FAIL'} {good if cond else bad}")
        ok = ok and bool(cond)
        return cond

    spans = []  # (what, start, length)

    # msglog's TextMsg thunk -> MsgFunc_TextMsg
    msrc = (SRC / "msglog.rs").read_text(encoding="utf-8")
    thunk = int(re.search(r'\("TextMsg",\s*0x([0-9a-f_]+)\)', msrc).group(1).replace("_", ""), 16)
    call = next(i for i in im.dis(thunk, 0x40) if i.mnemonic == "call")
    textmsg = im.target(call)
    print(f"TextMsg thunk +{thunk:#x} calls MsgFunc_TextMsg at +{textmsg:#x}\n")

    # ── spectator_text.rs: the camera mode name ─────────────────────────────
    ssrc = (SRC / "spectator_text.rs").read_text(encoding="utf-8")
    pattern, at, hidden = rust_site(ssrc, "MODE_NAME")
    print(f"== spectator_text.rs MODE_NAME ==  at={at}  hidden={hidden.hex(' ')}")
    hits = im.find_all(pattern)
    if check(len(hits) == 1, f"the signature matches once, at +{hits[0]:#x}" if hits else "", f"the signature matches {len(hits)} time(s)"):
        start = hits[0]
        spec_mode = im.cstr_rva(b"#Spec_Mode%d")
        pushes = [i for i in im.dis(start - 0x40, 0x40) if i.mnemonic == "push" and i.op_str == hex(spec_mode + im.base)]
        check(pushes, 'it follows `push "#Spec_Mode%d"` (CHudSpectator::SetMode)', 'no `push "#Spec_Mode%d"` in the 0x40 bytes before it')
        site = start + at
        ins = im.one(site)
        check(ins.mnemonic == "call" and ins.size == len(hidden) and im.target(ins) == textmsg,
              f"+{site:#x} is a {ins.size}-byte `call` to MsgFunc_TextMsg (+{textmsg:#x})",
              f"+{site:#x} is `{ins.mnemonic} {ins.op_str}` ({ins.size} bytes)")
        rets = im.function_rets(textmsg)
        check(rets and all(op == "0xc" for _, op in rets),
              f"MsgFunc_TextMsg's {len(rets)} ret(s) are all `ret 0xc`",
              f"MsgFunc_TextMsg's rets: {rets}")
        body = [i for i in im.dis(start, at) if i.address - im.base < site]
        last_clean = max((k for k, i in enumerate(body) if i.mnemonic == "add" and i.op_str.startswith("esp")), default=-1)
        npush = sum(1 for i in body[last_clean + 1 :] if i.mnemonic == "push")
        check(npush == 3, "three arguments are pushed for the call", f"{npush} pushes before the call")
        rep = list(im.md.disasm(hidden, 0))
        check(rep[0].mnemonic == "add" and rep[0].op_str == "esp, 0xc" and all(r.mnemonic == "nop" for r in rep[1:]) and sum(r.size for r in rep) == len(hidden),
              "the replacement is `add esp, 0xc` and nops, as wide as the call",
              f"the replacement decodes to {[(r.mnemonic, r.op_str) for r in rep]}")
        after = []
        for i in im.dis(site + ins.size, 0x20):
            if i.mnemonic in ("call", "ret") or i.mnemonic.startswith("j"):
                after.append(i)
                break
            after.append(i)
        reads = [i for i in after[:-1] if "eax" in i.op_str and not i.op_str.startswith("eax,")]
        flags = after[-1].mnemonic.startswith("j") and after[-1].mnemonic != "jmp"
        check(not reads and not flags, "nothing after the call reads eax or the flags before the next call",
              f"after the call: {[(i.mnemonic, i.op_str) for i in after]}")
        spans.append(("the mode name call", site, len(hidden)))

    # ── spectator_text.rs: Press DUCK ───────────────────────────────────────
    pattern, at, hidden = rust_site(ssrc, "SPEC_DUCK")
    print(f"\n== spectator_text.rs SPEC_DUCK ==  at={at}  hidden={hidden.hex(' ')}")
    hits = im.find_all(pattern)
    if check(len(hits) == 1, f"the signature matches once, at +{hits[0]:#x}" if hits else "", f"the signature matches {len(hits)} time(s)"):
        helper = hits[0] + at
        name = im.cstr_rva(b"Spec_Duck")
        named = any(hex(name + im.base) in i.op_str for i in im.dis(helper, 0xc0))
        check(named, "the function stores the message name `Spec_Duck`", "no reference to `Spec_Duck` in the function")
        check(hidden == b"\xc3", "the replacement is a bare `ret`", f"the replacement is {hidden.hex(' ')}")
        duck = im.cstr_rva(b"#Spec_Duck")
        callers = im.callers(helper)
        check(callers, f"{len(callers)} caller(s): {', '.join(f'+{c:#x}' for c in callers)}", "no callers found")
        for c in callers:
            # Raw bytes rather than a disassembly from an arbitrary point
            # before the call, which can start mid-instruction.
            pushed = (b"\x68" + struct.pack("<I", duck + im.base)) in im.img[c - 16 : c]
            nxt = im.dis(c + 5, 0x20)
            cleaned = False
            for i in nxt[:4]:
                if (i.mnemonic == "add" and i.op_str == "esp, 4") or (i.mnemonic == "pop" and i.op_str == "ecx"):
                    cleaned = True
                    break
                if i.mnemonic in ("ret", "call") or i.mnemonic.startswith("j"):
                    break
            check(pushed and cleaned, f'+{c:#x} pushes "#Spec_Duck" and pops it itself',
                  f"+{c:#x}: pushed={pushed} cleaned={cleaned}")
        first = im.one(helper)
        spans.append(("the Spec_Duck helper's first instruction", helper, first.size))

    # ── overview_marker.rs ──────────────────────────────────────────────────
    osrc = (SRC / "overview_marker.rs").read_text(encoding="utf-8")
    pattern = rust_str_const(osrc, "PATTERN")
    je_at = rust_usize(osrc, "JE_AT")
    skip_at = rust_usize(osrc, "SKIP_AT")
    print(f"\n== overview_marker.rs ==  JE_AT={je_at}  SKIP_AT={skip_at}")
    hits = im.find_all(pattern)
    if check(len(hits) == 1, f"the signature matches once, at +{hits[0]:#x}" if hits else "", f"the signature matches {len(hits)} time(s)"):
        start = hits[0]
        je = im.one(start + je_at)
        check(je.mnemonic == "je" and je.size == 2, f"+{start + je_at:#x} is a short `je` (the sprite choice)", f"+{start + je_at:#x} is `{je.mnemonic} {je.op_str}`")
        skip = im.one(start + skip_at)
        check(skip.mnemonic == "je" and skip.size == 2, f"+{start + skip_at:#x} is a short `je` (the draw's no-sprite exit)", f"+{start + skip_at:#x} is `{skip.mnemonic} {skip.op_str}`")
        prev = im.one(start + skip_at - 2)
        check(prev.mnemonic == "test" and prev.op_str == "esi, esi", "it follows `test esi, esi`", f"it follows `{prev.mnemonic} {prev.op_str}`")
        draw = im.one(start + skip_at + 2)
        check(draw.mnemonic == "fld", "the draw it skips starts with `fld`, so the x87 stack is untouched on the skip", f"the draw starts `{draw.mnemonic} {draw.op_str}`")
        exit_ = im.target(skip)
        tail = []
        for i in im.dis(exit_, 0x20):
            tail.append(i)
            if i.mnemonic == "ret":
                break
        fpu = [i for i in tail if i.mnemonic.startswith("f")]
        check(tail and tail[-1].mnemonic == "ret" and not fpu and all(i.mnemonic in ("pop", "mov", "ret") for i in tail),
              f"its target +{exit_:#x} is the epilogue: {' / '.join(i.mnemonic + ' ' + i.op_str for i in tail)}",
              f"its target +{exit_:#x}: {[(i.mnemonic, i.op_str) for i in tail]}")
        team_at = rust_usize(osrc, "TEAM_OPERAND_AT")
        cmp = im.one(start + team_at - 2)
        check(cmp.mnemonic == "cmp" and cmp.op_str.endswith(", 2") and cmp.size == 7,
              f"+{start + team_at - 2:#x} is `cmp {cmp.op_str}` (the team test whose address #907 points at the watched player's team)",
              f"+{start + team_at - 2:#x} is `{cmp.mnemonic} {cmp.op_str}`")
        spans.append(("the camera team test's address", start + team_at, 4))
        spans.append(("the camera sprite je", start + je_at, 2))
        spans.append(("the camera draw je", start + skip_at, 2))

    # ── overview_speaker.rs ─────────────────────────────────────────────────
    vsrc = (SRC / "overview_speaker.rs").read_text(encoding="utf-8")
    pattern = rust_str_const(vsrc, "PATTERN")
    span_at = rust_usize(vsrc, "SPAN_AT")
    jne_at = rust_usize(vsrc, "JNE_AT")
    stock = rust_bytes(re.search(r"const STOCK:[^=]*=\s*\[(.*?)\];", vsrc, re.S).group(1))
    print(f"\n== overview_speaker.rs ==  SPAN_AT={span_at}  JNE_AT={jne_at}  STOCK={stock.hex(' ')}")
    hits = im.find_all(pattern)
    if check(len(hits) == 1, f"the signature matches once, at +{hits[0]:#x}" if hits else "", f"the signature matches {len(hits)} time(s)"):
        start = hits[0]
        span = start + span_at
        check(im.img[span : span + len(stock)] == stock, f"+{span:#x} holds STOCK", f"+{span:#x} holds {im.img[span:span + len(stock)].hex(' ')}")
        ins = im.dis(span, 16)
        consumed, n = 0, 0
        for i in ins:
            if consumed >= len(stock):
                break
            consumed += i.size
            n += 1
        check(consumed == len(stock), f"the span is {n} whole instruction(s): " + " / ".join(f"{i.mnemonic} {i.op_str}" for i in ins[:n]),
              f"the span ends mid-instruction ({consumed} bytes)")
        jne = im.one(start + jne_at)
        exit_ = im.target(jne)
        check(jne.mnemonic == "jne" and jne.size == 2, f"+{start + jne_at:#x} is `jne +{exit_:#x}`", f"+{start + jne_at:#x} is `{jne.mnemonic} {jne.op_str}`")
        land = im.one(exit_)
        check(land.mnemonic == "mov" and land.op_str.startswith("bl,"),
              f"its target +{exit_:#x} is the next-entry code (`{land.mnemonic} {land.op_str}`)",
              f"its target +{exit_:#x} is `{land.mnemonic} {land.op_str}`")
        rel = exit_ - (span + 5)
        print(f"       the hidden span is `jmp +{exit_:#x}` = e9 {struct.pack('<i', rel).hex(' ')}")
        # the instance the talking check uses, and VidInit's member for the sprite
        mov_ecx = next(i for i in im.dis(start, span_at) if i.mnemonic == "mov" and i.op_str.startswith("ecx, 0x"))
        instance = int(mov_ecx.op_str.split(",")[1], 16) - im.base
        spr = im.cstr_rva(b"sprites/mapsprites/speakerIcon.spr")
        refs = [a for a, _s, m, op in im.sweep() if m == "push" and op == hex(spr + im.base)]
        member = None
        for r in refs:
            seen_call = False
            for i in im.dis(r, 0x20):
                if i.mnemonic == "call":
                    seen_call = True
                elif seen_call and i.mnemonic == "mov" and i.op_str.endswith(", eax") and "[esi + " in i.op_str:
                    member = int(re.search(r"\+ (0x[0-9a-f]+)\]", i.op_str).group(1), 16)
                    break
        handle = instance + member if member is not None else None
        block = im.dis(start + jne_at + 2, exit_ - (start + jne_at + 2))
        uses = [i for i in block if handle is not None and hex(handle + im.base) in i.op_str]
        check(uses, f"the skipped block reads the speakerIcon.spr handle (CHudSpectator +{instance:#x} + member {member:#x})",
              f"the skipped block never reads the speakerIcon.spr handle (instance {instance:#x}, member {member})")
        spans.append(("the speaker decision", span, len(stock)))

    # ── center_text.rs / clan_text.rs ───────────────────────────────────────
    csrc = (SRC / "clan_text.rs").read_text(encoding="utf-8")
    post = rust_str_const(csrc, "POST_PATTERN")
    post_at = rust_usize(csrc, "POST_OBJECT_AT")
    print("\n== center_text.rs / clan_text.rs ==")
    hits = im.find_all(post)
    if check(len(hits) == 1, f"POST_PATTERN matches once, at +{hits[0]:#x}" if hits else "", f"POST_PATTERN matches {len(hits)} time(s)"):
        obj = im.u32(hits[0] + post_at) - im.base
        queue = im.target(im.one(hits[0] + post_at + 4))
        print(f"       DrawClanTimer posts to +{obj:#x} through +{queue:#x}")
        table = None
        for i in im.dis(textmsg, 0x200):
            m = re.match(r"dword ptr \[eax\*4 \+ (0x[0-9a-f]+)\]", i.op_str)
            if i.mnemonic == "jmp" and m:
                table = int(m.group(1), 16) - im.base
                break
        if check(table is not None, f"MsgFunc_TextMsg switches through a table at +{table:#x}" if table else "", "no jump table in MsgFunc_TextMsg"):
            case4 = im.u32(table + 3 * 4) - im.base
            posted = None
            for i in im.dis(case4, 0x100):
                if i.mnemonic == "mov" and i.op_str.startswith("ecx, 0x"):
                    posted = int(i.op_str.split(",")[1], 16) - im.base
                if i.mnemonic == "call" and im.target(i) == queue:
                    break
                if i.mnemonic == "ret":
                    posted = None
                    break
            check(posted == obj, f"destination 4 (+{case4:#x}) posts through +{queue:#x} to +{obj:#x}, the object end_center_line clears",
                  f"destination 4 posts to {posted and hex(posted)}, end_center_line clears +{obj:#x}")

    # ── every span ──────────────────────────────────────────────────────────
    print("\n== branches into the patched spans ==")
    for what, start, length in spans:
        into = im.branches_into(start, start + length)
        check(not into, f"nothing branches into {what} (+{start:#x}, {length} bytes)",
              f"{what}: {[(f'+{a:#x}', m, f'+{t:#x}') for a, m, t in into]}")

    print("\nSIGNATURES VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
