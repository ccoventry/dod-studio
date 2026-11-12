import { readFileSync } from 'node:fs';
import { describe, it, expect } from 'vitest';
import { edlMs, jobFps, edlClips, pathsToProbe, vegasEdl, EDL_DEFAULT_FILE_NAME, EDL_SAVE_FILTER, edlDefaultPath } from './vegas_edl.js';

describe('edlDefaultPath', () => {
  const clip = (path) => ({ path });
  it('opens in the clips\' folder', () => {
    expect(edlDefaultPath([clip('D:\\out\\a.mp4'), clip('D:\\out\\b.mp4')])).toBe('D:\\out\\dod_highlights.txt');
    expect(edlDefaultPath([clip('/home/x/a.mp4')])).toBe('/home/x/dod_highlights.txt');
  });
  it('takes the folder holding most clips, a tie going to the first clip', () => {
    expect(edlDefaultPath([clip('D:\\a\\1.mp4'), clip('D:\\b\\2.mp4'), clip('D:\\b\\3.mp4')])).toBe('D:\\b\\dod_highlights.txt');
    expect(edlDefaultPath([clip('D:\\a\\1.mp4'), clip('D:\\b\\2.mp4')])).toBe('D:\\a\\dod_highlights.txt');
  });
  it('falls back to the bare name', () => {
    expect(edlDefaultPath([])).toBe(EDL_DEFAULT_FILE_NAME);
    expect(edlDefaultPath(undefined)).toBe(EDL_DEFAULT_FILE_NAME);
    expect(edlDefaultPath([clip('a.mp4')])).toBe(EDL_DEFAULT_FILE_NAME);
  });
});

describe('edlMs', () => {
  it('writes milliseconds with four decimals', () => {
    expect(edlMs(6)).toBe('6000.0000');
    expect(edlMs(1.5)).toBe('1500.0000');
    expect(edlMs(0)).toBe('0.0000');
    expect(edlMs(1 / 3)).toBe('333.3333');
    expect(edlMs(-1)).toBe('0.0000');
  });
});

describe('edlClips', () => {
  const job = (over) => ({
    name: 'a', status: 'Finished', output_path: 'D:\\r\\a.mov', frames: 600,
    settings_summary: 'ProRes @ 300fps (Render Settings)', ...over,
  });

  it('reads the frame rate out of the settings line', () => {
    expect(jobFps(job())).toBe(300);
    expect(jobFps({ settings_summary: 'ProRes @ 59.94fps (as captured)' })).toBe(59.94);
    expect(jobFps({ settings_summary: 'Source copy' })).toBeNull();
  });

  it('keeps finished renders with a file, and counts those whose length is unknown', () => {
    const { clips, skipped } = edlClips([
      job(), job({ name: 'b', status: 'Queued' }), job({ name: 'c', output_path: '' }),
      job({ name: 'd', settings_summary: 'Source copy' }),
    ]);
    expect(clips).toEqual([{ name: 'a', path: 'D:\\r\\a.mov', seconds: 2, take_key: '', hud: false, takeFolder: '' }]);
    expect(skipped).toBe(1);
  });

  it("carries each job's take key, for whose highlight it is (#429)", () => {
    const { clips } = edlClips([job({ take_key: 'session_1/chain_01_b0' })]);
    expect(clips[0].take_key).toBe('session_1/chain_01_b0');
  });

  it('names only the finished renders with no fps in their settings line to probe', () => {
    const jobs = [
      job(), job({ name: 'b', status: 'Queued', settings_summary: 'Source copy' }),
      job({ name: 'd', output_path: 'D:\\r\\d.mp4', settings_summary: 'Source copy' }),
    ];
    expect(pathsToProbe(jobs)).toEqual(['D:\\r\\d.mp4']);
  });

  it('places a copied or captured video at its probed length, in list order', () => {
    const jobs = [
      job({ name: 'd', output_path: 'D:\\r\\d.mp4', settings_summary: 'Source copy' }),
      job(),
      job({ name: 'e', output_path: 'D:\\r\\e.mp4', settings_summary: 'Source copy' }),
    ];
    const { clips, skipped } = edlClips(jobs, 'Finished', new Map([['D:\\r\\d.mp4', 4.25]]));
    expect(clips).toEqual([
      { name: 'd', path: 'D:\\r\\d.mp4', seconds: 4.25, take_key: '', hud: false, takeFolder: '' },
      { name: 'a', path: 'D:\\r\\a.mov', seconds: 2, take_key: '', hud: false, takeFolder: '' },
    ]);
    expect(skipped).toBe(1); // e: its file could not be read
  });
});

describe('vegasEdl', () => {
  const dir = 'C:\\Users\\someone\\Repos\\dod-studio\\local\\hud-overlay-test\\clips\\';
  const main1 = { name: 'm1', path: `${dir}main_1.mov`, seconds: 6, hud: false, takeFolder: 'T1' };
  const hud1 = { name: 'm1 hud', path: `${dir}main_1_hud.mov`, seconds: 6, hud: true, takeFolder: 'T1' };
  const main2 = { name: 'm2', path: `${dir}main_2.mov`, seconds: 6, hud: false, takeFolder: 'T2' };
  const lines = (text) => text.split('\r\n');
  const rowsOf = (text) => lines(text).slice(1, -1);

  // vegas_export.fixture.txt is a project VEGAS Pro 21 exported itself and
  // re-imported correctly; {DIR} stands for the clips' folder.
  it('matches VEGAS\'s own export for two clips and one HUD', () => {
    const golden = readFileSync(new URL('./vegas_export.fixture.txt', import.meta.url), 'utf8');
    expect(vegasEdl([main1, hud1, main2])).toBe(golden.replace(/\r?\n/g, '\r\n').replaceAll('{DIR}', dir));
  });

  it('writes the header line and CRLF line ends', () => {
    const out = vegasEdl([main1]);
    expect(lines(out)[0]).toMatch(/^"ID";"Track";"StartTime";"Length";"PlayRate".*;"FirstChannel";"Channels"$/);
    expect(out.endsWith('\r\n')).toBe(true);
    expect(out).not.toMatch(/[^\r]\n/);
  });

  it('puts the main video on Track 1 when there is no HUD render', () => {
    const rows = rowsOf(vegasEdl([main1, main2])).map((r) => r.split('; '));
    expect(rows.map((r) => [r[0], r[1], r[2], r[10]])).toEqual([
      ['1', '1', '0.0000', 'VIDEO'],
      ['2', '1', '6000.0000', 'VIDEO'],
      ['3', '0', '0.0000', 'AUDIO'],
      ['4', '0', '6000.0000', 'AUDIO'],
    ]);
  });

  it('keeps a HUD render over its own clip, picture only, whatever follows', () => {
    const rows = rowsOf(vegasEdl([main1, hud1, main2])).map((r) => r.split('; '));
    const hud = rows.find((r) => r[11].includes('main_1_hud'));
    expect(hud.slice(1, 4)).toEqual(['1', '0.0000', '6000.0000']);
    expect(rows.filter((r) => r[10] === 'AUDIO')).toHaveLength(2);
    expect(rows.filter((r) => r[1] === '2').map((r) => r[2])).toEqual(['0.0000', '6000.0000']);
  });

  it('places a HUD with no clip of its take after the rest, on the upper track', () => {
    const rows = rowsOf(vegasEdl([main2, hud1])).map((r) => r.split('; '));
    expect(rows[0].slice(1, 4)).toEqual(['1', '6000.0000', '6000.0000']);
  });

  it('puts each clip where the last one ended, without drift', () => {
    const many = Array.from({ length: 100 }, (_, i) => ({ name: `c${i}`, path: `D:\\c${i}.mov`, seconds: 1 / 3 }));
    const last = rowsOf(vegasEdl(many)).find((r) => r.includes('c99.mov') && r.includes('VIDEO'));
    expect(Number(last.split('; ')[2])).toBeCloseTo(99 * 333.3333, 3);
  });

  it('keeps a path on one line', () => {
    expect(vegasEdl([{ name: 'a', path: 'D:\\a\nb.mov', seconds: 1 }])).toContain('"D:\\a b.mov"');
  });
});

describe('save dialog defaults', () => {
  it('is a .txt, the only type the VEGAS EDL import dialog lists', () => {
    expect(EDL_DEFAULT_FILE_NAME).toMatch(/\.txt$/);
    expect(EDL_SAVE_FILTER.extensions).toEqual(['txt']);
  });
});
