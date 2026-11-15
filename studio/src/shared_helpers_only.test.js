import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';

// The third copy of a helper is where copies start to drift (docs/coding_standards.md,
// section 6). #1107 found the file name of a path worked out in fourteen places and
// the bytes in a gigabyte typed out in a dozen; each now has one home. A new copy
// should import it instead.
const dir = new URL('./', import.meta.url);
const sources = readdirSync(dir)
  .filter((name) => name.endsWith('.js') && !name.endsWith('.test.js'))
  .map((name) => ({ name, text: readFileSync(new URL(name, dir), 'utf8') }));

const SPLIT_POP = /split\(\/\[\\\\\/\]\/\)\.pop\(\)/;
const GB_LITERAL = /1024 \* 1024 \* 1024|1024 \*\* 3/;

describe('shared helpers', () => {
  it('a file name comes from fileNameOf in path_display.js', () => {
    const copies = sources.filter(({ name, text }) => name !== 'path_display.js' && SPLIT_POP.test(text));
    expect(copies.map(({ name }) => name)).toEqual([]);
  });

  it('a gigabyte is BYTES_PER_GB from byte_size.js', () => {
    const copies = sources.filter(({ name, text }) => name !== 'byte_size.js' && GB_LITERAL.test(text));
    expect(copies.map(({ name }) => name)).toEqual([]);
  });
});
