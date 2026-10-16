#!/usr/bin/env python3
"""Checks `spray_hltv.rs`'s two detours and its texture call against a real `hw.dll`.

`spray_hltv.rs` (#428) makes a demo with no customization for a player draw
the spray his `player` line names instead of the bullet hole. It patches two
places and calls a third; the unit tests prove the stubs assemble as
documented, and only the binary can prove what they rely on:

  0. Exactly one of the builds in `BUILDS` matches this `hw.dll` (by its
     `CL_ParseTEnt` signature), and that build's other two signatures match.
  1. `CL_ParseTEnt`: the signature matches once, inside the switch case for
     `TE_PLAYERDECAL` (112), and starts with the stolen bytes, which are the
     `customdata` test, its `je` to the wad-decal branch and the texture
     test. The case stores the player number at `[ebp+playernum_disp]` and
     loads `customdata` from the same table `Draw_DecalTexture` reads, into
     the register the test checks. `custom_at` is the custom branch, which
     passes the player number and the texture from where `tex_store` puts it
     to `R_CustomDecalShoot` (which shoots `~player` at scale 1.0);
     `else_at` is the branch calling through the decal table; the span is
     followed by the custom branch's `je`. Neither branch reads `eax`, `ecx`
     or `edx` before writing them, except the texture register itself.
  2. `Draw_DecalTexture`: the signature matches once and starts with the six
     stolen bytes; the `customdata` table is at `customdata_at` and players
     are `PLAYER_STRIDE` apart; every direct call lands on the entry and
     every `ret` is plain (cdecl).
  3. Nothing branches into either span's interior, and no relocated dword
     (a jump table entry, say) points there.
  4. `Draw_MiptexTexture`: the signature matches once, the function names
     itself in its `Bad cached wad` error, reads `cacheExtra` at `+0x18`,
     uploads the buffer it was given as a `GLT_DECAL` (1) texture, and stores
     the GL texture number at `+0x18`, where `spray_hltv.rs` reads it.

Every constant comes out of `spray_hltv.rs` rather than being restated here.

Usage:
    python goldsrc-hooks/tools/verify_hltv_spray_offsets.py [path-to-hw.dll]
    python goldsrc-hooks/tools/verify_hltv_spray_offsets.py --anniversary

Defaults to the pre-Anniversary movies install; `--anniversary` is the stock
25th Anniversary one. Needs `pip install pefile capstone`.
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

DEFAULT_DLL = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\hw.dll"
)
ANNIVERSARY_DLL = Path(r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life\hw.dll")
RUST = Path(__file__).resolve().parent.parent / "src" / "spray_hltv.rs"
TE_PLAYERDECAL = 112
SCRATCH = {"eax", "ecx", "edx"}


def rust_builds(src):
    table = re.search(r"const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S)
    if not table:
        raise SystemExit("could not find `const BUILDS` in spray_hltv.rs")
    builds = []
    for block in re.findall(r"Build \{(.*?)\n    \}", table.group(1), re.S):
        def text(field):
            m = re.search(rf'{field}: "(.*?)",', block, re.S)
            return " ".join(m.group(1).replace("\\", " ").split())

        def raw(field):
            m = re.search(rf"{field}: &\[([^\]]*)\]", block)
            return bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", m.group(1)))

        def num(field):
            return int(re.search(rf"{field}: (0x[0-9a-f]+|\d+),", block).group(1), 0)

        builds.append({
            "name": re.search(r'name: "([^"]+)"', block).group(1),
            "te_pattern": text("te_pattern"),
            "te_stolen": raw("te_stolen"),
            "cust_test": raw("cust_test"),
            "tex_test": raw("tex_test"),
            "tex_store": raw("tex_store"),
            "playernum_disp": num("playernum_disp"),
            "custom_at": num("custom_at"),
            "else_at": num("else_at"),
            "decal_texture": text("decal_texture"),
            "customdata_at": num("customdata_at"),
            "miptex_texture": text("miptex_texture"),
        })
    if not builds:
        raise SystemExit("`BUILDS` in spray_hltv.rs has no entries this script can read")
    return builds


def rust_const(src, name):
    m = re.search(rf"const {name}: [^=]+= (.*?);", src, re.S)
    if not m:
        raise SystemExit(f"could not find `const {name}` in spray_hltv.rs")
    return m.group(1)


def signature_matches(image, pattern):
    rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()), re.S)
    return [m.start() for m in rx.finditer(image)]


def branch_targets(pe, image, base, md):
    found = {}
    for section in pe.sections:
        if not section.Characteristics & 0x20000000:
            continue
        lo = section.VirtualAddress
        hi = lo + max(section.Misc_VirtualSize, section.SizeOfRawData)
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
                value = int.from_bytes(image[entry.rva:entry.rva + 4], "little")
                found.setdefault(value - base, []).append(entry.rva)
    return found


def text_of(ins):
    return f"{ins.mnemonic} {ins.op_str}"


def disp_text(disp):
    """`[ebp - 0x38]` for a disp8 of 0xc8."""
    signed = disp - 0x100 if disp >= 0x80 else disp
    return f"ebp - {-signed:#x}" if signed < 0 else f"ebp + {signed:#x}"


def prologue_before(image, rva, limit=0x6000):
    at = rva
    while at > rva - limit:
        if image[at:at + 3] == b"\x55\x8b\xec" and image[at - 1] in (0xCC, 0x90, 0xC3, 0x00):
            return at
        at -= 1
    return None


def case_target(image, base, fn, site):
    """The address the switch in the function at `fn` jumps to for 112."""
    at = fn
    while True:
        # jmp [reg*4 + table]: the SIB names any index register.
        at = image.find(b"\xff\x24", at, site)
        if at < 0:
            return None
        if image[at + 2] & 0xC7 != 0x85:
            at += 1
            continue
        table = struct.unpack_from("<I", image, at + 3)[0] - base
        blob = image[at - 24:at]
        index = None
        for i in range(len(blob) - 5):
            if blob[i] == 0x8A and (blob[i + 1] & 0xC0) == 0x80 and (blob[i + 1] & 7) != 4:
                index = struct.unpack_from("<I", blob, i + 2)[0] - base
            if blob[i:i + 2] == b"\x0f\xb6" and (blob[i + 2] & 0xC0) == 0x80 and (blob[i + 2] & 7) != 4:
                index = struct.unpack_from("<I", blob, i + 3)[0] - base
        case = image[index + TE_PLAYERDECAL] if index is not None else TE_PLAYERDECAL
        target = struct.unpack_from("<I", image, table + case * 4)[0] - base
        if target <= site < target + 0x200:
            return target
        at += 1


def written_before_read(md, image, base, start, stop_at_call=True, allowed=()):
    """Registers of SCRATCH read before written from `start` to the first call."""
    written, bad = set(), []
    for ins in md.disasm(image[start:start + 0x40], base + start):
        reads, writes = ins.regs_access()
        names_r = {ins.reg_name(r) for r in reads}
        names_w = {ins.reg_name(r) for r in writes}
        for r in (names_r & SCRATCH) - written - set(allowed):
            bad.append((ins.address - base, r, text_of(ins)))
        written |= names_w & SCRATCH
        if stop_at_call and ins.mnemonic == "call":
            break
    return bad


def main():
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    dll = ANNIVERSARY_DLL if arg == "--anniversary" else Path(arg) if arg else DEFAULT_DLL
    if not dll.is_file():
        return print(f"no hw.dll at {dll}") or 2

    src = RUST.read_text(encoding="utf-8")
    builds = rust_builds(src)
    dt_stolen = bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", rust_const(src, "DT_STOLEN")))
    stride = int(rust_const(src, "PLAYER_STRIDE"), 0)
    tex_prefix = int(rust_const(src, "TEX_PREFIX"), 0)

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
        return passed

    def dis(start, n):
        return list(md.disasm(image[start:start + n], base + start))

    # -- 0. which build --------------------------------------------------------
    print(f"== {dll} ==")
    hits = {b["name"]: signature_matches(image, b["te_pattern"]) for b in builds}
    for name, found in hits.items():
        print(f"     {name} CL_ParseTEnt signature: {len(found)} match(es) {[hex(m) for m in found]}")
    matching = [b for b in builds if hits[b["name"]]]
    if len(matching) != 1:
        print(f"FAIL {len(matching)} builds' signatures match; exactly one should")
        print("\nMISMATCH -- do not ship")
        return 1
    b = matching[0]
    print(f"     this is the {b['name']} hw.dll")

    # -- 1. CL_ParseTEnt ---------------------------------------------------------
    print("\n== CL_ParseTEnt, TE_PLAYERDECAL ==")
    if not check(len(hits[b["name"]]) == 1, f"the signature matches once, at +{hits[b['name']][0]:#x}"):
        return 1
    site = hits[b["name"]][0]
    stolen = b["te_stolen"]
    check(image[site:site + len(stolen)] == stolen, f"it starts with the stolen bytes {stolen.hex(' ')}")
    fn = prologue_before(image, site)
    case = case_target(image, base, fn, site) if fn else None
    if not check(case is not None, f"the site is in CL_ParseTEnt (+{fn or 0:#x})'s case {TE_PLAYERDECAL}, "
                                   f"which starts at +{case or 0:#x}"):
        return 1
    span = dis(site, len(stolen))
    check([i.bytes for i in span] == [b["cust_test"], stolen[len(b["cust_test"]):len(b["cust_test"]) + 2], b["tex_test"]],
          "the span is the customdata test, a short je and the texture test: "
          + "; ".join(text_of(i) for i in span))
    je = span[1] if len(span) > 1 else None
    check(je is not None and je.mnemonic == "je" and int(je.op_str, 16) - base == site + b["else_at"],
          f"the je goes to else_at (+{site + b['else_at']:#x})")

    case_body = dis(case, site - case)
    pn = disp_text(b["playernum_disp"])
    store = [i for i in case_body if text_of(i) == f"mov dword ptr [{pn}], eax"]
    read_byte_dec = any(i.mnemonic == "call" for i in case_body[:4]) and any(i.mnemonic == "dec" for i in case_body[:5])
    check(store and read_byte_dec, f"the case reads a byte, subtracts one and keeps it at [{pn}] (the player number)")
    cust_reg = span[0].op_str.split(",")[0].strip()
    loads = [i for i in case_body if re.fullmatch(rf"mov {cust_reg}, dword ptr \[e\w\w \+ (0x[0-9a-f]+)\]", text_of(i))]
    customdata_te = int(re.search(r"\+ (0x[0-9a-f]+)\]", loads[0].op_str).group(1), 16) if loads else None
    check(loads, f"customdata goes into {cust_reg}, the register the test checks"
          + (f", from table {customdata_te:#x}" if customdata_te else ""))
    stride_ok = any(re.fullmatch(rf"imul e\w\w, e\w\w, {stride:#x}", text_of(i)) for i in case_body)
    texts = [text_of(i) for i in case_body]
    for k in range(len(texts) - 2):
        a = re.fullmatch(r"lea (e\w\w), \[(e\w\w) \+ \2\*8\]", texts[k])
        if a and re.fullmatch(rf"lea {a.group(2)}, \[{a.group(2)} \+ {a.group(1)}\*4\]", texts[k + 1]) \
                and texts[k + 2] == f"shl {a.group(2)}, 4" and 37 * 16 == stride:
            stride_ok = True
    check(stride_ok, f"players are {stride:#x} bytes apart in that table")

    custom = site + b["custom_at"]
    after = dis(site + len(stolen), 6)
    check(after and after[0].mnemonic == "je" and site + len(stolen) + after[0].size == custom,
          f"the span is followed by the custom branch's je, then the custom branch at +{custom:#x}")
    branch = []
    for ins in dis(custom, 0x40):
        branch.append(ins)
        if ins.mnemonic == "call":
            break
    call = branch[-1]
    shoot = int(call.op_str, 16) - base if call.mnemonic == "call" and call.op_str.startswith("0x") else None
    tex_where = [text_of(i) for i in md.disasm(b["tex_store"], 0)][0]
    tex_place = re.search(r"mov (.*), eax", tex_where).group(1)
    tex_reg = tex_place if tex_place in SCRATCH else None
    reads_tex = any(tex_place.replace("dword ptr ", "") in text_of(i).replace("dword ptr ", "") for i in branch[:-1])
    reads_pn = any(f"[{pn}]" in text_of(i) for i in branch[:-1])
    check(shoot is not None and reads_tex and reads_pn,
          f"the custom branch passes the player number and the texture ({tex_place}) to +{shoot or 0:#x}")
    if shoot is not None:
        body = dis(shoot, 0x40)
        flips = any(i.mnemonic == "not" for i in body) or any(
            text_of(body[k]) == "or ecx, 0xffffffff" and body[k + 1].mnemonic == "sub" for k in range(len(body) - 1))
        scale = any("0x3f800000" in i.op_str for i in body)
        check(flips and scale, f"+{shoot:#x} is R_CustomDecalShoot: it shoots ~player at scale 1.0")
    bad = written_before_read(md, image, base, custom, allowed=[tex_reg] if tex_reg else [])
    check(not bad, "the custom branch writes eax/ecx/edx before reading them"
          + "".join(f"\n       +{a:#x} reads {r}: {t}" for a, r, t in bad))
    els = site + b["else_at"]
    else_body = dis(els, 0x20)
    table_calls = [i for i in else_body if re.fullmatch(r"call dword ptr \[0x[0-9a-f]+\]", text_of(i))]
    check(len(table_calls) >= 2, f"the wad-decal branch at +{els:#x} calls through the decal table twice")
    bad = written_before_read(md, image, base, els)
    check(not bad, "the wad-decal branch writes eax/ecx/edx before reading them"
          + "".join(f"\n       +{a:#x} reads {r}: {t}" for a, r, t in bad))

    # -- 2. Draw_DecalTexture ------------------------------------------------------
    print("\n== Draw_DecalTexture ==")
    dts = signature_matches(image, b["decal_texture"])
    if not check(len(dts) == 1, f"the signature matches once: {[hex(m) for m in dts]}"):
        return 1
    dt = dts[0]
    check(image[dt:dt + len(dt_stolen)] == dt_stolen, f"it starts with the stolen bytes {dt_stolen.hex(' ')}")
    customdata = struct.unpack_from("<I", image, dt + b["customdata_at"])[0]
    check(customdata == customdata_te, f"its customdata table {customdata:#x} is CL_ParseTEnt's")
    dt_body = []
    for ins in dis(dt, 0x100):
        dt_body.append(ins)
        if ins.mnemonic == "ret" and ins.address - base > dt + 0x80:
            break
    rets = [i for i in dt_body if i.mnemonic == "ret"]
    check(rets and all(not i.op_str for i in rets), f"its {len(rets)} ret(s) are plain (cdecl)")

    # -- 3. the spans ----------------------------------------------------------------
    print("\n== the spans ==")
    targets = branch_targets(pe, image, base, md)
    pointers = relocated_pointers(pe, image, base)
    for what, start, length in (("CL_ParseTEnt", site, len(stolen)), ("Draw_DecalTexture", dt, len(dt_stolen))):
        interior = range(start + 1, start + length)
        inbound = [(s, t) for t in interior for s, _ in targets.get(t, ())]
        inbound += [(s, t) for t in interior for s in pointers.get(t, ())]
        check(not inbound, f"{what}: no branch or relocated dword points inside +{start:#x}..+{start + length:#x}"
              + "".join(f"\n       +{s:#x} -> +{t:#x}" for s, t in inbound))
        check(length >= 5, f"{what}: the span is {length} bytes; a near jump needs 5")
    callers = targets.get(dt, [])
    check(callers and all(m == "call" for _, m in callers), f"Draw_DecalTexture: {len(callers)} direct calls, all `call`s")

    # -- 4. Draw_MiptexTexture --------------------------------------------------------
    print("\n== Draw_MiptexTexture ==")
    mts = signature_matches(image, b["miptex_texture"])
    if not check(len(mts) == 1, f"the signature matches once: {[hex(m) for m in mts]}"):
        return 1
    mt = mts[0]
    body = []
    for ins in dis(mt, 0x300):
        body.append(ins)
        if ins.mnemonic == "ret":
            break
    message = image.find(b"Draw_MiptexTexture: Bad cached wad")
    check(message >= 0 and any(f"{base + message:#x}" in i.op_str for i in body),
          "it names itself in its `Bad cached wad` error")
    check(any(re.fullmatch(rf"(cmp dword ptr \[e\w\w \+ 0x18\], {tex_prefix:#x}|mov eax, dword ptr \[esi \+ 0x18\])", text_of(i))
              for i in body), f"it checks cacheExtra (+0x18) against {tex_prefix:#x}")
    tex_reg = next((re.fullmatch(r"mov (e\w\w), dword ptr \[ebp \+ 0xc\]", text_of(i)).group(1)
                    for i in body if re.fullmatch(r"mov (e\w\w), dword ptr \[ebp \+ 0xc\]", text_of(i))), None)
    calls = [k for k, i in enumerate(body) if i.mnemonic == "call" and i.op_str.startswith("0x")]
    upload = None
    for k in calls:
        if k >= 2 and text_of(body[k - 1]) == f"push {tex_reg}" and text_of(body[k - 2]) == "push 1":
            upload = k
    check(upload is not None, f"it uploads the buffer it was given ({tex_reg}) as GLT_DECAL (push 1)")
    if upload is not None:
        nxt = [text_of(i) for i in body[upload + 1:upload + 4]]
        check(f"mov dword ptr [{tex_reg} + 0x18], eax" in nxt, "and keeps the GL texture number at +0x18")

    print("\nDETOURS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
