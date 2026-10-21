#!/usr/bin/env python3
"""Checks `pov_overview.rs`'s spans against real `hw.dll` files (#897).

`pov_overview.rs` detours the store of 0 into `ref_params_s.nextView` that
`V_RenderView` makes after the demo view override. The unit tests prove each
pattern's span ends on that store; only the binaries can prove:

  1. Each build's pattern matches exactly once in its own `hw.dll`, and not at
     all in the other build's.
  2. The span sits less than 0x80 bytes after the call to the demo view
     override (found by `pov_live_bob.rs`' own pattern), and the store's `ebp`
     displacement is that call's `ref_params_s` (its `lea reg, [ebp-x]`) plus
     0xe0, `nextView`'s offset.
  3. Nothing in `.text` branches into the span past its first byte (a raw scan
     of every `E8`/`E9`/`EB`/`7x`/`0F 8x` displacement, which over-reports
     rather than misses; any hit fails the check and wants a disassembly).

Patterns are read out of the Rust sources rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_pov_overview_offsets.py [hw.dll ...]

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
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "hw.dll",
]
SRC = Path(__file__).resolve().parent.parent / "src"
NEXT_VIEW_AT = 0xE0


def ours():
    text = (SRC / "pov_overview.rs").read_text(encoding="utf-8")
    out = []
    for m in re.finditer(r'build: "([^"]+)",.*?pattern: "([^"]+)",.*?span_at: (\d+),'
                         r'\s*span_len: (\d+),\s*keep: (\d+),\s*frame_disp: 0x([0-9A-Fa-f]+)',
                         text, re.S):
        out.append(dict(build=m.group(1), pattern=m.group(2), span_at=int(m.group(3)),
                        span_len=int(m.group(4)), keep=int(m.group(5)),
                        disp=int(m.group(6), 16)))
    if len(out) != 2:
        sys.exit(f"expected 2 sites in pov_overview.rs, found {len(out)}")
    return out


def overrides():
    text = (SRC / "pov_live_bob.rs").read_text(encoding="utf-8")
    out = {m.group(1): (m.group(2), int(m.group(3))) for m in re.finditer(
        r'build: "([^"]+)".*?pattern: "([^"]+)".*?call_at: (\d+)', text, re.S)}
    if len(out) != 2:
        sys.exit("expected 2 sites in pov_live_bob.rs")
    return out


def compile_pattern(pattern):
    return re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)]))
                               for t in pattern.split()), re.S)


def branches_into(image, lo, hi, first, last):
    """Raw displacement scan: sources whose target is in (first, last]."""
    hits = []
    for p in range(lo, hi - 6):
        b = image[p]
        if b in (0xE8, 0xE9):
            t = p + 5 + struct.unpack_from("<i", image, p + 1)[0]
        elif b == 0xEB or 0x70 <= b <= 0x7F:
            t = p + 2 + struct.unpack_from("<b", image, p + 1)[0]
        elif b == 0x0F and 0x80 <= image[p + 1] <= 0x8F:
            t = p + 6 + struct.unpack_from("<i", image, p + 2)[0]
        else:
            continue
        if first < t <= last:
            hits.append((p, t))
    return hits


def check(path, sites, override_sites):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    text = next(s for s in pe.sections if s.Name.rstrip(b"\0") == b".text")
    lo, hi = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize
    ok = True
    matched = []
    for site in sites:
        hits = [m.start() for m in compile_pattern(site["pattern"]).finditer(image)]
        if not hits:
            continue
        build = site["build"]
        if len(hits) > 1:
            print(f"  FAIL {build}: {len(hits)} matches, expected one")
            ok = False
            continue
        span = hits[0] + site["span_at"]
        end = span + site["span_len"]
        # 2. after the override's call, on its ref_params' nextView.
        opattern, call_at = override_sites[build]
        ohits = [m.start() for m in compile_pattern(opattern).finditer(image)]
        if len(ohits) != 1:
            print(f"  FAIL {build}: the override's call pattern matched {len(ohits)} times")
            ok = False
            continue
        call = ohits[0] + call_at
        lea_disp = struct.unpack_from("<i", image, ohits[0] + 7)[0]
        want = (lea_disp + NEXT_VIEW_AT) & 0xFF
        if not 0 < span - call < 0x80:
            print(f"  FAIL {build}: span +{span:#x} is not just after the override call +{call:#x}")
            ok = False
            continue
        if site["disp"] != want:
            print(f"  FAIL {build}: store displacement {site['disp']:#x}, "
                  f"ref_params' nextView is {want:#x}")
            ok = False
            continue
        # 3. no branch into the span.
        into = branches_into(image, lo, hi, span, end - 1)
        if into:
            for src, t in into:
                print(f"  CHECK {build}: +{src:#x} may branch to +{t:#x}, inside the span")
            ok = False
            continue
        print(f"  ok   {build}: span +{span:#x}..+{end:#x}, {span - call:#x} bytes after the "
              f"override call, store to [ebp{lea_disp + NEXT_VIEW_AT:+#x}]")
        matched.append(build)
    if len(matched) != 1:
        print(f"  FAIL: {len(matched)} builds matched, expected exactly one")
        ok = False
    return ok


def main():
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    sites = ours()
    override_sites = overrides()
    ok = True
    for path in dlls:
        print(path)
        ok &= check(path, sites, override_sites)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
