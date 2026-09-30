// Unit tests for www/export.js. Run with: node --test web-analyzer/tests/export.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  isDemoFileName,
  selectDemoFiles,
  buildExportEntry,
  toJson,
  jsonFileNameFor,
  batchFileName,
} from '../www/export.js';

const file = (name, size = 1024, lastModified = 1_700_000_000_500) => ({ name, size, lastModified });

test('isDemoFileName matches .dem case-insensitively', () => {
  assert.equal(isDemoFileName('a.dem'), true);
  assert.equal(isDemoFileName('A.DEM'), true);
  assert.equal(isDemoFileName('a.dem.txt'), false);
  assert.equal(isDemoFileName('notes.txt'), false);
  assert.equal(isDemoFileName(undefined), false);
});

test('selectDemoFiles passes a single file through untouched', () => {
  const only = file('notes.txt');
  assert.deepEqual(selectDemoFiles([only]), [only]);
  assert.deepEqual(selectDemoFiles([]), []);
  assert.deepEqual(selectDemoFiles(null), []);
});

test('selectDemoFiles drops non-demos from a multi-file pick, keeping order', () => {
  const a = file('b.dem');
  const b = file('readme.txt');
  const c = file('A.DEM');
  assert.deepEqual(selectDemoFiles([a, b, c]), [a, c]);
  assert.deepEqual(selectDemoFiles([b, file('x.cfg')]), []);
});

test('buildExportEntry carries file info plus the analysis', () => {
  const analysis = { demo_info: { map: 'dod_anzio' }, state: { players: [] }, extra: 1 };
  assert.deepEqual(buildExportEntry(file('x.dem', 2048), analysis), {
    file_name: 'x.dem',
    file_size_bytes: 2048,
    file_modified_unix_secs: 1_700_000_000,
    demo_info: { map: 'dod_anzio' },
    state: { players: [] },
  });
});

test('buildExportEntry records a failure as an error string', () => {
  const entry = buildExportEntry(file('bad.dem', 10, 0), null, new Error('truncated'));
  assert.deepEqual(entry, {
    file_name: 'bad.dem',
    file_size_bytes: 10,
    file_modified_unix_secs: 0,
    error: 'Error: truncated',
  });
});

test('toJson writes an array and survives BigInt values', () => {
  const out = toJson([{ a: 1n << 60n }, { b: 'x' }]);
  assert.deepEqual(JSON.parse(out), [{ a: '1152921504606846976' }, { b: 'x' }]);
});

test('jsonFileNameFor swaps .dem for .json', () => {
  assert.equal(jsonFileNameFor('dod_anzio_0001.dem'), 'dod_anzio_0001.json');
  assert.equal(jsonFileNameFor('UPPER.DEM'), 'UPPER.json');
  assert.equal(jsonFileNameFor('weird'), 'weird.json');
  assert.equal(jsonFileNameFor('.dem'), 'demo.json');
  assert.equal(jsonFileNameFor(''), 'demo.json');
});

test('batchFileName includes the count and local date', () => {
  const d = new Date(2026, 8, 29, 23, 59);
  assert.equal(batchFileName(3, d), 'demo-analysis-3-demos-2026-09-29.json');
  assert.equal(batchFileName(1, d), 'demo-analysis-1-demo-2026-09-29.json');
});
