// The chip wording; the repainting is covered by tests/e2e/capture-output-status.spec.js.
import { describe, it, expect } from 'vitest';
import { rowStatusFor, createCaptureOutputStatus } from './capture_output_status.js';

describe('rowStatusFor', () => {
  it('says checking until there is an answer', () => {
    expect(rowStatusFor(undefined).text).toBe('Checking…');
  });
  it('maps each status to a tone', () => {
    expect(rowStatusFor({ status: 'ok', usable: true }).tone).toBe('ok');
    expect(rowStatusFor({ status: 'not_found', usable: true })).toMatchObject({ tone: 'note', text: 'Will be created' });
    expect(rowStatusFor({ status: 'not_found', usable: false })).toMatchObject({ tone: 'bad', text: 'Drive not found' });
    expect(rowStatusFor({ status: 'not_absolute', usable: false }).tone).toBe('bad');
    expect(rowStatusFor({ status: 'not_a_directory', usable: false }).text).toBe('That is a file');
    expect(rowStatusFor({ status: 'malformed', usable: false }).text).toBe('Not a valid path');
  });
});

describe('createCaptureOutputStatus', () => {
  const fakeEl = () => ({ hidden: false, className: '', textContent: '', title: '' });
  function setup(paths, diagnose) {
    const chips = paths.map(fakeEl);
    const rows = chips.map((chip) => ({ querySelector: () => chip, appendChild() {} }));
    globalThis.document = { createElement: fakeEl };
    const status = createCaptureOutputStatus({
      getPaths: () => paths,
      diagnose,
      container: { querySelectorAll: () => rows },
    });
    return { chips, status };
  }

  it('hides the chip of a blank row instead of leaving it on Checking', async () => {
    const { chips, status } = setup(['E:/a', ''], async (p) => p.map((path) => ({ path, status: 'ok', usable: true })));
    await status.refresh();
    expect(chips[0].textContent).toBe('Found');
    expect(chips[1].hidden).toBe(true);
  });

  it('only the newest check draws', async () => {
    const paths = ['E:/a'];
    let release;
    const slow = new Promise((r) => { release = r; });
    let n = 0;
    const { chips, status } = setup(paths, (p) => (++n === 1
      ? slow
      : Promise.resolve(p.map((path) => ({ path, status: 'not_found', usable: true })))));
    const first = status.refresh();
    await status.refresh();
    release([{ path: 'E:/a', status: 'ok', usable: true }]);
    await first;
    expect(chips[0].textContent).toBe('Will be created');
  });

  it('hides the chips when the check fails', async () => {
    const { chips, status } = setup(['E:/a'], async () => { throw new Error('x'); });
    await status.refresh();
    expect(chips[0].hidden).toBe(true);
  });
});
