import { describe, it, expect } from 'vitest';
import { edlFrames, edlTimecode, jobFps, edlClips, pathsToProbe, vegasEdl, planEdlParts, partPaths } from './vegas_edl.js';

describe('timecodes', () => {
  it('counts 30 frames a second, as VEGAS reads them', () => {
    expect(edlFrames(1.5)).toBe(45);
    expect(edlTimecode(45)).toBe('00:00:01:15');
    expect(edlTimecode(0)).toBe('00:00:00:00');
    expect(edlTimecode(30 * 3725 + 7)).toBe('01:02:05:07');
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
    expect(clips).toEqual([{ name: 'a', path: 'D:\\r\\a.mov', seconds: 2, hud: false, takeFolder: '' }]);
    expect(skipped).toBe(1);
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
      { name: 'd', path: 'D:\\r\\d.mp4', seconds: 4.25, hud: false, takeFolder: '' },
      { name: 'a', path: 'D:\\r\\a.mov', seconds: 2, hud: false, takeFolder: '' },
    ]);
    expect(skipped).toBe(1); // e: its file could not be read
  });
});

describe('vegasEdl', () => {
  // The shape VEGAS Pro 21.0 imported correctly: clips at 0 s, 2 s and 5 s,
  // the third 1.5 s long, in a 60 fps project.
  const clips = [
    { name: 'Garand 3 kills', path: 'D:\\dodstudio-110\\clips\\clip_01.mp4', seconds: 2 },
    { name: 'MP40 spree', path: 'D:\\dodstudio-110\\clips\\clip_02.mp4', seconds: 3 },
    { name: 'K98 headshot', path: 'D:\\dodstudio-110\\clips\\clip_03.mp4', seconds: 1.5 },
  ];
  const expected = [
    'TITLE:   dod_test',
    'FCM: NON-DROP FRAME',
    '',
    '001  DOD0001  V      C        00:00:00:00 00:00:02:00 00:00:00:00 00:00:02:00',
    '* FROM CLIP NAME: D:\\dodstudio-110\\clips\\clip_01.mp4',
    '* LOC: 00:00:00:00 RED     Garand 3 kills',
    '002  DOD0001  AA     C        00:00:00:00 00:00:02:00 00:00:00:00 00:00:02:00',
    '* FROM CLIP NAME: D:\\dodstudio-110\\clips\\clip_01.mp4',
    '003  DOD0002  V      C        00:00:00:00 00:00:03:00 00:00:02:00 00:00:05:00',
    '* FROM CLIP NAME: D:\\dodstudio-110\\clips\\clip_02.mp4',
    '* LOC: 00:00:02:00 RED     MP40 spree',
    '004  DOD0002  AA     C        00:00:00:00 00:00:03:00 00:00:02:00 00:00:05:00',
    '* FROM CLIP NAME: D:\\dodstudio-110\\clips\\clip_02.mp4',
    '005  DOD0003  V      C        00:00:00:00 00:00:01:15 00:00:05:00 00:00:06:15',
    '* FROM CLIP NAME: D:\\dodstudio-110\\clips\\clip_03.mp4',
    '* LOC: 00:00:05:00 RED     K98 headshot',
    '006  DOD0003  AA     C        00:00:00:00 00:00:01:15 00:00:05:00 00:00:06:15',
    '* FROM CLIP NAME: D:\\dodstudio-110\\clips\\clip_03.mp4',
    '',
  ].join('\r\n');

  it('writes the file shape VEGAS Pro 21 imported, clip by clip', () => {
    expect(vegasEdl(clips, 'dod_test')).toBe(expected);
  });

  it('puts each clip where the last one ended, without drift', () => {
    const many = Array.from({ length: 100 }, (_, i) => ({ name: `c${i}`, path: `D:\\c${i}.mov`, seconds: 1 / 3 }));
    const last = vegasEdl(many).split('\r\n').filter((l) => /^\d{3} /.test(l)).pop();
    // 100 clips of 10 frames each: the last starts at frame 990.
    expect(last).toContain('00:00:33:00 00:00:33:10');
  });

  it('keeps a label on one line', () => {
    expect(vegasEdl([{ name: 'a\nb', path: 'D:\\x.mov', seconds: 1 }])).toContain('RED     a b');
  });
});

describe('HUD renders', () => {
  const main = { name: 'a', path: 'D:/a.mov', seconds: 2, hud: false, takeFolder: 'T1' };
  const hud = { name: 'a hud', path: 'D:/a_hud.mov', seconds: 2, hud: true, takeFolder: 'T1' };
  const next = { name: 'b', path: 'D:/b.mov', seconds: 1, hud: false, takeFolder: 'T2' };

  it('lie over the clip of their take, picture only', () => {
    const events = vegasEdl([main, hud, next]).split('\r\n').filter((l) => /^\d{3} /.test(l));
    expect(events).toEqual([
      '001  DOD0001  V      C        00:00:00:00 00:00:02:00 00:00:00:00 00:00:02:00',
      '002  DOD0001  AA     C        00:00:00:00 00:00:02:00 00:00:00:00 00:00:02:00',
      '003  DOD0002  V      C        00:00:00:00 00:00:02:00 00:00:00:00 00:00:02:00',
      '004  DOD0003  V      C        00:00:00:00 00:00:01:00 00:00:02:00 00:00:03:00',
      '005  DOD0003  AA     C        00:00:00:00 00:00:01:00 00:00:02:00 00:00:03:00',
    ]);
  });

  it('with no clip of their take before them are placed after the rest, still without sound', () => {
    const events = vegasEdl([next, hud]).split('\r\n').filter((l) => /^\d{3} /.test(l));
    expect(events).toHaveLength(3);
    expect(events[2]).toContain('V      C        00:00:00:00 00:00:02:00 00:00:01:00 00:00:03:00');
  });

  it('are read from the job list', () => {
    const jobs = [{
      name: 'a', status: 'Finished', output_path: 'D:/a_hud.mov', frames: 600,
      stream: 'HUD ONLY', take_folder: 'T1', settings_summary: 'ProRes @ 300fps (Render Settings)',
    }];
    expect(edlClips(jobs).clips[0]).toMatchObject({ hud: true, takeFolder: 'T1' });
  });
});

describe('planEdlParts', () => {
  const clip = (i, hud = false) => ({ name: `c${i}`, path: `D:/c${i}.mov`, seconds: 1, hud, takeFolder: `T${i}` });

  it('keeps a short list in one file', () => {
    expect(planEdlParts([clip(1), clip(2)])).toHaveLength(1);
  });

  it('splits so no file passes 999 events, and no event number repeats within one', () => {
    const clips = Array.from({ length: 1200 }, (_, i) => clip(i));
    const parts = planEdlParts(clips);
    expect(parts.map((p) => p.length)).toEqual([499, 499, 202]);
    parts.forEach((p) => {
      const numbers = vegasEdl(p).split('\r\n').filter((l) => /^\d{3,} /.test(l)).map((l) => l.slice(0, 3));
      expect(numbers.length).toBeLessThanOrEqual(999);
    });
    expect(parts.flat()).toHaveLength(1200);
  });

  it('keeps a HUD render in the file of its clip, behind it', () => {
    const [a, b] = [clip(1), clip(2)];
    const parts = planEdlParts([a, b, { ...clip(1, true), takeFolder: 'T1' }]);
    expect(parts).toHaveLength(1);
    expect(parts[0].map((c) => c.name)).toEqual(['c1', 'c1', 'c2']);
    expect(parts[0][1].hud).toBe(true);
  });

  it('counts a HUD render as one event', () => {
    const clips = [];
    for (let i = 0; i < 400; i += 1) { clips.push(clip(i)); clips.push(clip(i, true)); }
    // 400 x 3 events = 1200: 333 pairs fit in 999.
    expect(planEdlParts(clips).map((p) => p.length)).toEqual([666, 134]);
  });
});


describe('partPaths', () => {
  it('leaves one file alone and numbers several', () => {
    expect(partPaths('D:\\x\\a.edl', 1)).toEqual(['D:\\x\\a.edl']);
    expect(partPaths('D:\\x\\a.edl', 2)).toEqual(['D:\\x\\a_part1.edl', 'D:\\x\\a_part2.edl']);
    expect(partPaths('D:\\x.y\\a', 2)[0]).toBe('D:\\x.y\\a_part1.edl');
  });
});
