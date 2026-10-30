import { describe, it, expect } from 'vitest';
import { edlFrames, edlTimecode, jobFps, edlClips, pathsToProbe, vegasEdl } from './vegas_edl.js';

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
    expect(clips).toEqual([{ name: 'a', path: 'D:\\r\\a.mov', seconds: 2, take_key: '' }]);
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
      { name: 'd', path: 'D:\\r\\d.mp4', seconds: 4.25, take_key: '' },
      { name: 'a', path: 'D:\\r\\a.mov', seconds: 2, take_key: '' },
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
