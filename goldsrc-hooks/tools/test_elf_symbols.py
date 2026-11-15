"""elf_symbols.py on a tiny synthetic ELF32: sections, `.symtab`, a build id, a build date.

usage: python -m unittest test_elf_symbols   (from this folder)
"""
import struct
import unittest

from elf_symbols import NotElf, filter_symbols, read_sections, read_symbols, summarize

SHT_PROGBITS, SHT_SYMTAB, SHT_STRTAB, SHT_NOTE = 1, 2, 3, 7


def build_elf(with_symtab=True):
    """An ELF32 image with `.rodata`, `.comment`, a GNU build-id note and, optionally, `.symtab`."""
    symstr = b"\0Cbuf_Init\0_ZN5CGame4InitEv\0g_count\0"
    symbols = b"".join([
        bytes(16),  # the null symbol
        struct.pack("<IIIBBH", 1, 0x1000, 67, 0x12, 0, 1),  # Cbuf_Init, global func
        struct.pack("<IIIBBH", 11, 0x1100, 20, 0x12, 0, 1),  # CGame::Init
        struct.pack("<IIIBBH", 28, 0x2000, 4, 0x11, 0, 2),  # g_count, global object
        struct.pack("<IIIBBH", 0, 0x3000, 4, 0x12, 0, 1),  # unnamed: skipped
    ])
    note = struct.pack("<III", 4, 4, 3) + b"GNU\0" + bytes.fromhex("deadbeef")
    sections = [
        (".rodata", SHT_PROGBITS, b"Exe build: 12:00:00 Oct  8 2024\0"),
        (".comment", SHT_PROGBITS, b"GCC: (GNU) 5.4.1\0"),
        (".note.gnu.build-id", SHT_NOTE, note),
        (".debug_info", SHT_PROGBITS, b"\0" * 8),
    ]
    if with_symtab:
        sections += [(".symtab", SHT_SYMTAB, symbols), (".strtab", SHT_STRTAB, symstr)]
    names = b"\0" + b"".join(n.encode() + b"\0" for n, _t, _d in sections) + b".shstrtab\0"
    sections.append((".shstrtab", SHT_STRTAB, names))

    out = bytearray(52)
    placed = []
    for name, typ, data in sections:
        placed.append((name, typ, len(out), len(data)))
        out += data
    while len(out) % 4:
        out.append(0)
    shoff = len(out)
    out += bytes(40)  # the null section
    strtab_index = len(sections) - 1 if not with_symtab else [n for n, *_ in sections].index(".strtab") + 1
    for name, typ, off, size in placed:
        link = strtab_index if typ == SHT_SYMTAB else 0
        out += struct.pack("<IIIIIIIIII", names.index(name.encode() + b"\0"), typ, 0, 0, off, size, link, 0, 1, 0)
    out[:16] = b"\x7fELF" + bytes([1, 1, 1]) + bytes(9)
    struct.pack_into("<HHIIIIIHHHHHH", out, 16, 3, 3, 1, 0, 0, shoff, 0, 52, 0, 0, 40,
                     len(sections) + 1, len(sections))
    return bytes(out)


class ElfSymbolsTest(unittest.TestCase):
    def test_sections_are_named(self):
        names = [s[0] for s in read_sections(build_elf())]
        self.assertEqual(names[0], "")
        self.assertIn(".symtab", names)
        self.assertIn(".debug_info", names)

    def test_symbols_skip_unnamed_and_sort(self):
        data = build_elf()
        syms = read_symbols(data, read_sections(data))
        self.assertEqual(syms, [
            (0x1000, 67, "func", "Cbuf_Init"),
            (0x1100, 20, "func", "_ZN5CGame4InitEv"),
            (0x2000, 4, "object", "g_count"),
        ])

    def test_summary(self):
        s = summarize(build_elf())
        self.assertTrue(s["symtab"])
        self.assertEqual(s["dwarf"], [".debug_info"])
        self.assertEqual((s["functions"], s["objects"]), (2, 1))
        self.assertEqual(s["build_id"], "deadbeef")
        self.assertEqual(s["compilers"], ["GCC: (GNU) 5.4.1"])
        self.assertEqual(s["dates"], ["Oct  8 2024"])

    def test_stripped_file_has_no_symbols(self):
        s = summarize(build_elf(with_symtab=False))
        self.assertFalse(s["symtab"])
        self.assertEqual(s["functions"], 0)

    def test_filter_demangles_before_grep(self):
        data = build_elf()
        syms = read_symbols(data, read_sections(data))
        hits = filter_symbols(syms, pattern="^CGame::", readable=True)
        self.assertEqual([h[3] for h in hits], ["CGame::Init"])
        self.assertEqual(filter_symbols(syms, pattern="^CGame::"), [])
        self.assertEqual(len(filter_symbols(syms, kind="all")), 3)

    def test_not_elf(self):
        with self.assertRaises(NotElf):
            read_sections(b"MZ" + bytes(100))


if __name__ == "__main__":
    unittest.main()
