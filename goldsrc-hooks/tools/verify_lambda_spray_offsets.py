#!/usr/bin/env python3
"""Checks `spray_lambda.rs`'s detour against a real `hw.dll`.

`spray_lambda.rs` (#1322) gives a lambda, the world decal a server places when
it has no custom spray for a player, the spray the Players table says. It
patches one place in `CL_ParseTEnt`; the unit tests prove the stub assembles as
documented, and only the binary can prove what it relies on:

  0. Exactly one of the builds in `BUILDS` matches this `hw.dll`.
  1. The signature matches once, inside the switch case `TE_WORLDDECAL` (116)
     shares with `TE_DECAL`, `TE_WORLDDECALHIGH` and `TE_DECALHIGH`. It opens
     on a `jnp` (the `r_decals` test) to a plain way out of the function, and
     the site follows it.
  2. The stub pushes what the engine's own shot is about to take: the
     position, the flags (1 for `TE_BSPDECAL`), the entity and the index, the
     last being the one the high-index messages add 256 to. The engine's shot
     is two calls through the decal table.
  3. `eax`, `ecx` and `edx` are written before they are read from the site on,
     and nothing branches into the span.
  4. `spray_hltv`'s `UNKNOWN_SLOT`, one past the players, stays inside hw.dll:
     the `Draw_DecalTexture` stub reads that slot's `customdata` for it.
  5. `R_DecalShoot` itself gives `}lambda06` the custom flag
     `R_CustomDecalShoot` shoots with, so a replacement sits on the wall as
     the lambda did.

Every constant comes out of `spray_lambda.rs` rather than being restated here.
`spray_hltv`'s own checks, the call to `R_CustomDecalShoot` included, are
`verify_hltv_spray_offsets.py`'s.

Usage:
    python goldsrc-hooks/tools/verify_lambda_spray_offsets.py [path-to-hw.dll]
    python goldsrc-hooks/tools/verify_lambda_spray_offsets.py --anniversary

Defaults to the pre-Anniversary movies install; `--anniversary` is the POST-Anniversary
movies install. Needs `pip install pefile capstone`.
"""

import re
import struct
import sys
from pathlib import Path

import verify_hltv_spray_offsets as spray

import capstone
import pefile

RUST = Path(__file__).resolve().parent.parent / "src" / "spray_lambda.rs"
TE_DECAL_FAMILY = (104, 116, 117, 118)


def rust_builds(src):
    table = re.search(r"const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S)
    if not table:
        raise SystemExit("could not find `const BUILDS` in spray_lambda.rs")
    builds = []
    for block in re.findall(r"Build \{(.*?)\n    \}", table.group(1), re.S):
        def raw(field):
            text = re.sub(r"//[^\n]*", "", block)
            m = re.search(rf"{field}: &\[(.*?)\]", text, re.S)
            return bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", m.group(1)))

        def num(field):
            return int(re.search(rf"{field}: (\d+),", block).group(1))

        builds.append({
            "name": re.search(r'name: "([^"]+)"', block).group(1),
            "pattern": " ".join(re.search(r'pattern: "(.*?)",', block, re.S).group(1).split()),
            "site_at": num("site_at"),
            "exit_rel_at": num("exit_rel_at"),
            "stolen": raw("stolen"),
            "push_args": raw("push_args"),
        })
    if not builds:
        raise SystemExit("`BUILDS` in spray_lambda.rs has no entries this script can read")
    return builds


def pushed_values(instructions):
    """What each `push` in `instructions` pushes, with `eax`, `ecx` and `edx`
    followed back to the `mov` or `lea` that set them."""
    known, pushes = {}, []
    for ins in instructions:
        if ins.mnemonic in ("mov", "lea"):
            dest, source = (part.strip() for part in ins.op_str.split(",", 1))
            known[dest] = source
        elif ins.mnemonic == "push":
            pushes.append(known.get(ins.op_str, ins.op_str).replace("dword ptr ", ""))
    return pushes


def main():
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    dll = spray.ANNIVERSARY_DLL if arg == "--anniversary" else Path(arg) if arg else spray.DEFAULT_DLL
    if not dll.is_file():
        return print(f"no hw.dll at {dll}") or 2

    builds = rust_builds(RUST.read_text(encoding="utf-8"))
    spray_src = spray.RUST.read_text(encoding="utf-8")
    spray_builds = spray.rust_builds(spray_src)
    stride = int(spray.rust_const(spray_src, "PLAYER_STRIDE"), 0)
    players = int(spray.rust_const(spray_src, "MAX_PLAYERS"), 0)

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

    print(f"== {dll} ==")
    hits = {b["name"]: spray.signature_matches(image, b["pattern"]) for b in builds}
    for name, found in hits.items():
        print(f"     {name}: {len(found)} match(es) {[hex(m) for m in found]}")
    matching = [b for b in builds if hits[b["name"]]]
    if len(matching) != 1:
        print(f"FAIL {len(matching)} builds' signatures match; exactly one should")
        print("\nMISMATCH -- do not ship")
        return 1
    b = matching[0]
    print(f"     this is the {b['name']} hw.dll")

    # -- 1. the site -----------------------------------------------------------------
    print("\n== the site ==")
    if not check(len(hits[b["name"]]) == 1, "the signature matches once"):
        return 1
    found = hits[b["name"]][0]
    site = found + b["site_at"]
    stolen = b["stolen"]
    check(image[site:site + len(stolen)] == stolen, f"the site +{site:#x} holds the stolen bytes {stolen.hex(' ')}")
    fn = spray.prologue_before(image, site)
    cases = {te: spray.case_target(image, base, fn, site, te) if fn else None for te in TE_DECAL_FAMILY}
    case = cases[116]
    check(case is not None and len(set(cases.values())) == 1,
          f"it is in CL_ParseTEnt (+{fn or 0:#x})'s case for TE_DECAL, TE_WORLDDECAL and TE_*DECALHIGH, "
          f"at +{case or 0:#x}")
    jnp = dis(found, 6)[0]
    exit_rva = int(jnp.op_str, 16) - base
    check(jnp.mnemonic == "jnp" and exit_rva == site + struct.unpack_from("<i", image, found + b["exit_rel_at"])[0],
          f"it opens on the r_decals jnp, to +{exit_rva:#x}")
    tail = []
    for ins in dis(exit_rva, 0x20):
        tail.append(ins)
        if ins.mnemonic == "ret":
            break
    check(tail[-1].mnemonic == "ret" and [i.mnemonic for i in tail].count("pop") >= 3,
          "the jnp's target is a plain way out: " + "; ".join(spray.text_of(i) for i in tail))

    # -- 2. the arguments ------------------------------------------------------------
    print("\n== the arguments ==")
    shot = dis(site, 0x18)
    upto = next(k for k, i in enumerate(shot) if i.mnemonic in ("call", "jmp"))
    engine = pushed_values(shot[:upto])
    stub = pushed_values(list(md.disasm(b["push_args"], 0)))
    flags, pos, _model, entity, index = engine
    check(stub == [pos, flags, entity, index],
          f"the stub pushes position, flags, entity, index as the engine does: {stub}")
    body = dis(case, site - case)
    high = f"add {'dword ptr ' if index.startswith('[') else ''}{index}, 0x100"
    check(any(spray.text_of(i) == high for i in body), f"the index is what the high messages add 256 to ({high})")
    check(any(spray.text_of(i) == f"xor {entity}, {entity}" for i in body),
          f"{entity} is cleared when the message has no entity")
    start = shot[upto]
    if start.mnemonic == "jmp":
        start = dis(int(start.op_str, 16) - base, 6)[0]
    after = dis(start.address - base, 0x14)
    table_calls = [i for i in after if re.fullmatch(r"call dword ptr \[0x[0-9a-f]+\]", spray.text_of(i))]
    check(len(table_calls) == 2, "the engine's shot is two calls through the decal table")

    # -- 3. the span -----------------------------------------------------------------
    print("\n== the span ==")
    bad = spray.written_before_read(md, image, base, site)
    check(not bad, "eax/ecx/edx are written before they are read from the site on"
          + "".join(f"\n       +{a:#x} reads {r}: {t}" for a, r, t in bad))
    targets = spray.branch_targets(pe, image, base, md)
    pointers = spray.relocated_pointers(pe, image, base)
    interior = range(site + 1, site + len(stolen))
    inbound = [(s, t) for t in interior for s, _ in targets.get(t, ())]
    inbound += [(s, t) for t in interior for s in pointers.get(t, ())]
    check(not inbound, f"no branch or relocated dword points inside +{site:#x}..+{site + len(stolen):#x}"
          + "".join(f"\n       +{s:#x} -> +{t:#x}" for s, t in inbound))
    check(len(stolen) >= 5, f"the span is {len(stolen)} bytes; a near jump needs 5")

    # -- 4. the slot past the players ------------------------------------------------
    print("\n== the slot past the players ==")
    dt_build = next(s for s in spray_builds if s["name"] == b["name"])
    dts = spray.signature_matches(image, dt_build["decal_texture"])
    if not check(len(dts) == 1, "Draw_DecalTexture's signature matches once"):
        return 1
    customdata = struct.unpack_from("<I", image, dts[0] + dt_build["customdata_at"])[0] - base
    past = customdata + players * stride
    inside = [s for s in pe.sections if s.VirtualAddress <= past < s.VirtualAddress + s.Misc_VirtualSize]
    check(inside, f"slot {players}'s customdata, +{past:#x}, is mapped in {inside[0].Name if inside else 'nothing'}")

    # -- 5. the engine's own flag for the lambda ---------------------------------------
    print("\n== the lambda's flag ==")
    name = image.find(b"}lambda06\0")
    push = b"\x68" + struct.pack("<I", base + name)
    use = image.find(push)
    near = dis(use, 0x20) if name >= 0 and use >= 0 else []
    check(any(spray.text_of(i).startswith("or ") and i.op_str.endswith(", 4") for i in near),
          f"the code at +{use:#x} sets flag 4 for }}lambda06")

    print("\nDETOUR VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
