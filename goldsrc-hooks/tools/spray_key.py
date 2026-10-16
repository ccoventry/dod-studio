#!/usr/bin/env python3
"""The key the hook maps a player's spray by (#428), from spray WADs on disk.

    python -I goldsrc-hooks/tools/spray_key.py <tempdecal.wad | custom.hpk | logos folder> ...

For every one-lump spray WAD (a file, every `logos/*.wad`, or every entry of
a `custom.hpk`) prints one line:

    <key>  <WxH>  <lump name>  <resource md5>  <where>

`<key>` is what a `dodstudio_sprays.txt` line starts with: the MD5 of the
spray as the game loads it -- width (LE32), height (LE32), the mip-0 indices,
the palette count (LE16) and the palette (3 bytes a colour) -- the same bytes
`goldsrc-hooks/src/spray.rs::spray_key` hashes at upload. `<resource md5>` is
the MD5 of the whole WAD file, which is what demos and `custom.hpk` name a
spray by; the two differ and only the first is the key.

Add `--check` to also run the engine's own validation rules
(`Draw_ValidateCustomLogo` and the container check) over each WAD and say
what fails.
"""
import hashlib
import struct
import sys
from pathlib import Path

MAX_SIDE = 256


def lump_key(wad):
    """(key hex, w, h, lump name, problems) for a one-lump WAD3, or None if it
    isn't one."""
    problems = []
    if wad[:4] != b"WAD3" or len(wad) < 12:
        return None
    count, dir_at = struct.unpack_from("<ii", wad, 4)
    if count != 1:
        problems.append(f"{count} lumps, the engine wants exactly one")
    if dir_at + 32 * count != len(wad):
        problems.append("the directory isn't at the end of the file")
    if dir_at < 12 or dir_at + 32 > len(wad):
        return None
    filepos, disksize, size, kind, compression = struct.unpack_from("<iiiBB", wad, dir_at)
    name = wad[dir_at + 16:dir_at + 32].split(b"\0")[0].decode("latin1")
    if disksize != size:
        problems.append(f"disksize {disksize} != size {size}")
    if filepos < 12 or filepos + disksize > dir_at:
        problems.append("the lump runs outside the file")
    if compression:
        problems.append("compressed")
    lump = wad[filepos:filepos + size]
    if len(lump) < 40:
        return None
    w, h = struct.unpack_from("<II", lump, 16)
    offsets = struct.unpack_from("<4I", lump, 24)
    if not (1 <= w <= MAX_SIDE and 1 <= h <= MAX_SIDE):
        problems.append(f"{w}x{h} is outside 1..{MAX_SIDE}")
    pix = w * h
    mips = [pix, pix >> 2, pix >> 4, pix >> 6]
    for i in range(3):
        if offsets[i] + mips[i] != offsets[i + 1]:
            problems.append(f"mip {i + 1} offset {offsets[i + 1]} != {offsets[i]} + {mips[i]}")
    pal_count_at = offsets[0] + sum(mips)
    if pal_count_at + 2 > len(lump):
        problems.append("truncated before the palette")
        return None
    (pal_count,) = struct.unpack_from("<H", lump, pal_count_at)
    if pal_count == 0:
        # Passes the engine's check, and GL_Upload8 reads 768 bytes anyway:
        # the hook keys it as a full palette, so this does too.
        pal_count = 256
        problems.append("palette count 0 (read as 256, as the engine does)")
    if pal_count > 256:
        problems.append(f"{pal_count} palette entries")
    if pal_count_at + 2 + 3 * pal_count > size:
        problems.append("the palette runs past the lump")
    indices = lump[offsets[0]:offsets[0] + pix]
    palette = lump[pal_count_at + 2:pal_count_at + 2 + 3 * pal_count]
    key = hashlib.md5(struct.pack("<II", w, h) + indices + struct.pack("<H", pal_count) + palette).hexdigest()
    return key, w, h, name, problems


def hpak_entries(data):
    """[(name, md5 hex, wad bytes)] from a custom.hpk: `HPAK`, version, a
    directory offset; the directory is a count and 144-byte entries
    (resource_t with its MD5 at +77, then offset and length)."""
    if data[:4] != b"HPAK":
        return []
    _version, dir_at = struct.unpack_from("<ii", data, 4)
    (count,) = struct.unpack_from("<i", data, dir_at)
    out = []
    for i in range(count):
        at = dir_at + 4 + 144 * i
        name = data[at:at + 64].split(b"\0")[0].decode("latin1")
        md5 = data[at + 77:at + 93].hex()
        offset, length = struct.unpack_from("<ii", data, at + 136)
        out.append((name, md5, data[offset:offset + length]))
    return out


def report(wad, where, check):
    got = lump_key(wad)
    if got is None:
        print(f"-- not a spray WAD: {where}")
        return
    key, w, h, name, problems = got
    resource = hashlib.md5(wad).hexdigest()
    print(f"{key}  {w}x{h}  {name!r}  {resource}  {where}")
    if check and problems:
        for p in problems:
            print(f"    !! {p}")


def main():
    args = [a for a in sys.argv[1:] if a != "--check"]
    check = "--check" in sys.argv
    if not args:
        sys.exit(__doc__)
    for arg in args:
        path = Path(arg)
        if path.is_dir():
            for wad in sorted(path.glob("*.wad")):
                report(wad.read_bytes(), str(wad), check)
            continue
        data = path.read_bytes()
        if data[:4] == b"HPAK":
            for name, md5, wad in hpak_entries(data):
                note = "" if hashlib.md5(wad).hexdigest() == md5 else "  (stored md5 differs from the entry's bytes)"
                report(wad, f"{path}:{name}{note}", check)
        else:
            report(data, str(path), check)


if __name__ == "__main__":
    main()
