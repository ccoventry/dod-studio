import { describe, it, expect } from 'vitest';
import { recordRender, rendersOf, projectRenderedVideos } from './take_renders.js';
import {
  clipsToFind, applyRelinkReport, relinkProgressView, relinkAppliedLine, relinkResultSummary,
} from './clip_relink_logic.js';
import { STRINGS } from './strings.js';

const KEY = 'session_b/chain_00_b0';
const render = (started, extra = {}) => ({
  started_unix_ms: started, stream: 'all', codec: 'prores', fps: 60, outcome: 'finished',
  output_path: `D:/exports/clip_${started}.mov`, ...extra,
});

function project() {
  const index = { [KEY]: ['uid'] };
  recordRender(index, KEY, render(1, { output_size_bytes: 10, content_hash: 'aa' }));
  recordRender(index, KEY, render(2));
  recordRender(index, KEY, render(3));
  return index;
}

describe('clipsToFind', () => {
  it('sends each render\'s tag parts and what the project knows of its file', () => {
    expect(clipsToFind(projectRenderedVideos(project()))).toEqual([
      { take_key: KEY, started_unix_ms: 1, stream: 'all', output_path: 'D:/exports/clip_1.mov', output_size_bytes: 10, content_hash: 'aa' },
      { take_key: KEY, started_unix_ms: 2, stream: 'all', output_path: 'D:/exports/clip_2.mov', output_size_bytes: null, content_hash: null },
      { take_key: KEY, started_unix_ms: 3, stream: 'all', output_path: 'D:/exports/clip_3.mov', output_size_bytes: null, content_hash: null },
    ]);
  });
});

describe('applyRelinkReport', () => {
  it('moves found videos, keeps in-place fingerprints and counts the ones not found', () => {
    const index = project();
    const videos = projectRenderedVideos(index);
    const counts = applyRelinkReport(index, videos, {
      clips: [
        { path: 'D:/exports/clip_1.mov', found_by: 'in_place', size_bytes: 10, content_hash: 'aa' },
        { path: 'E:/sorted/best kill.mov', found_by: 'stream_tag', size_bytes: 20, content_hash: 'bb' },
        { path: null, found_by: 'missing', size_bytes: null, content_hash: null },
      ],
    });
    expect(counts).toEqual({ moved: 1, notFound: 1, superseded: 0, changed: 1 });
    const [first, second, third] = rendersOf(index, KEY);
    expect(first.output_path).toBe('D:/exports/clip_1.mov');
    expect(second).toMatchObject({ output_path: 'E:/sorted/best kill.mov', output_size_bytes: 20, content_hash: 'bb' });
    expect(third.output_path).toBe('D:/exports/clip_3.mov');
  });

  it('counts a fingerprint taken in place as a change, so the project keeps it', () => {
    const index = project();
    const videos = projectRenderedVideos(index).slice(2);
    const counts = applyRelinkReport(index, videos, {
      clips: [{ path: 'D:/exports/clip_3.mov', found_by: 'in_place', size_bytes: 30, content_hash: 'cc' }],
    });
    expect(counts).toEqual({ moved: 0, notFound: 0, superseded: 0, changed: 1 });
    expect(rendersOf(index, KEY)[2].content_hash).toBe('cc');
  });

  it('treats a short report as not found rather than guessing', () => {
    const index = project();
    expect(applyRelinkReport(index, projectRenderedVideos(index), { clips: [] }))
      .toEqual({ moved: 0, notFound: 3, superseded: 0, changed: 0 });
  });
});

describe('relinkProgressView', () => {
  it('fills the bar while tagging and checking, and slides while listing', () => {
    expect(relinkProgressView({ step: 'tagging', done: 1, total: 4 }))
      .toEqual({ pct: 25, indeterminate: false, text: 'Looking at video 2 of 4 where it was rendered…' });
    expect(relinkProgressView({ step: 'listing', done: 12, total: 0 }))
      .toEqual({ pct: 0, indeterminate: true, text: 'Listing the folders: 12 video files so far…' });
    expect(relinkProgressView({ step: 'checking', done: 9, total: 10 }))
      .toEqual({ pct: 90, indeterminate: false, text: 'Reading the tag of file 10 of 10…' });
  });
});

describe('relinkAppliedLine', () => {
  it('gives the counts, no paths', () => {
    expect(relinkAppliedLine({ moved: 2, notFound: 1, superseded: 1, changed: 3 }, 5))
      .toBe('[clip-relink] project updated: 2 of 5 video(s) found in a new place, 1 not found, 1 replaced by a later render, 3 record(s) changed');
  });
});

describe('a render a later render replaced (#1442 review)', () => {
  it('is left as it is and not counted as not found', () => {
    const index = project();
    const videos = projectRenderedVideos(index);
    const counts = applyRelinkReport(index, videos, {
      clips: [
        { path: null, found_by: 'superseded', size_bytes: 10, content_hash: 'aa' },
        { path: 'D:/exports/clip_2.mov', found_by: 'in_place', size_bytes: 2, content_hash: 'bb' },
        { path: 'D:/exports/clip_3.mov', found_by: 'in_place', size_bytes: 3, content_hash: 'cc' },
      ],
    });
    expect(counts).toEqual({ moved: 0, notFound: 0, superseded: 1, changed: 2 });
    expect(rendersOf(index, KEY)[0]).toMatchObject({ output_path: 'D:/exports/clip_1.mov', content_hash: 'aa' });
    const summary = relinkResultSummary(counts, videos.length, { search_complete: true });
    expect(STRINGS.RENDER.findMovedDone(summary)).toBe('No moved videos found: all 2 are where they were rendered.');
  });
});

describe('the result toast', () => {
  it('says how many online-only files were left unread', () => {
    const summary = relinkResultSummary({ moved: 0, notFound: 1, superseded: 0 }, 2, { cloud_files_skipped: 3 });
    expect(STRINGS.RENDER.findMovedDone(summary)).toBe(
      'No moved videos found. 1 of 2 still not found. 3 files are only in the cloud, so they were left unread (reading would download them).',
    );
  });

  it('says a cancelled scan stopped, with what it found', () => {
    const summary = relinkResultSummary({ moved: 1, notFound: 4, superseded: 0 }, 5, { cancelled: true });
    expect(STRINGS.RENDER.findMovedDone(summary)).toBe('Stopped. Found 1 moved video and pointed the project at it.');
  });
});
