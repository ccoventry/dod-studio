import { describe, it, expect } from 'vitest';
import {
  selectionKey,
  wasRendered,
  bulkTargets,
  createRenderSelection,
} from './render_selection.js';

function job(overrides = {}) {
  return {
    id: '0',
    stream: 'all',
    status: 'Queued',
    take_folder: 'D:/captures/s1/chain_01_b0/take0000',
    history: [],
    ...overrides,
  };
}

const finished = { outcome: 'finished', output_exists: true, output_size_bytes: 10 };
const failed = { outcome: 'failed' };

describe('rendered before (#70)', () => {
  it('a finished attempt in the history counts, a failed one does not', () => {
    expect(wasRendered(job())).toBe(false);
    expect(wasRendered(job({ history: [failed] }))).toBe(false);
    expect(wasRendered(job({ history: [failed, finished] }))).toBe(true);
  });

  it('the key tells a new scan\'s job 0 from the old job 0', () => {
    const a = job({ id: '0', take_folder: 'D:/a/take0000' });
    const b = job({ id: '0', take_folder: 'D:/b/take0000' });
    expect(selectionKey(a)).not.toBe(selectionKey(b));
    expect(selectionKey(job({ stream: 'all' }))).not.toBe(selectionKey(job({ stream: 'HUD ONLY' })));
  });
});

describe('createRenderSelection', () => {
  function setup(initial) {
    let jobs = initial;
    let changes = 0;
    const sel = createRenderSelection({ getJobs: () => jobs, onChange: () => { changes += 1; } });
    sel.sync(jobs);
    return { sel, setJobs: (next) => { jobs = next; return sel.sync(next); }, changes: () => changes };
  }

  it('every row starts ticked, and counts exactly', () => {
    const { sel } = setup([
      job({ id: '0' }),
      job({ id: '1', take_folder: 'D:/x/take0001', history: [finished] }),
      job({ id: '2', take_folder: 'D:/x/take0002' }),
    ]);
    expect(sel.counts()).toEqual({ ticked: 3, total: 3 });
    expect(sel.tickedJobs().map((j) => j.id)).toEqual(['0', '1', '2']);
  });

  it('keeps existing ticks when a snapshot adds rows, and forgets rows that left', () => {
    const a = job({ id: '0' });
    const { sel, setJobs } = setup([a]);
    sel.setTicked(a, false);
    const b = job({ id: '1', take_folder: 'D:/x/take0001' });
    expect(setJobs([a, b])).toBe(true);
    expect(sel.isTicked(a)).toBe(false);
    expect(sel.isTicked(b)).toBe(true);
    setJobs([b]);
    expect(sel.counts()).toEqual({ ticked: 1, total: 1 });
    // The old row is gone from the map, so if it came back it would get
    // its default again.
    setJobs([a, b]);
    expect(sel.isTicked(a)).toBe(true);
  });

  it('a fresh scan reusing id 0 for another take does not inherit the tick', () => {
    const old = job({ id: '0', take_folder: 'D:/old/take0000' });
    const { sel, setJobs } = setup([old]);
    sel.setTicked(old, false);
    const fresh = job({ id: '0', take_folder: 'D:/new/take0000' });
    setJobs([fresh]);
    expect(sel.isTicked(fresh)).toBe(true);
  });

  it('select all, none and unrendered', () => {
    const rendered = job({ id: '1', take_folder: 'D:/x/take0001', history: [finished] });
    const { sel, changes } = setup([job({ id: '0' }), rendered]);
    expect(sel.counts().ticked).toBe(2);
    sel.selectNone();
    expect(sel.counts().ticked).toBe(0);
    sel.selectAll();
    expect(sel.counts().ticked).toBe(2);
    sel.selectUnrendered();
    expect(sel.tickedJobs().map((j) => j.id)).toEqual(['0']);
    expect(changes()).toBe(3);
    // No change, no callback.
    sel.selectUnrendered();
    expect(changes()).toBe(3);
  });

  it('setTicked only reports a real change', () => {
    const a = job();
    const { sel, changes } = setup([a]);
    sel.setTicked(a, true);
    expect(changes()).toBe(0);
    sel.setTicked(a, false);
    expect(changes()).toBe(1);
  });
});

describe('bulkTargets', () => {
  const TAKE_A = 'D:/x/take0000';
  const TAKE_B = 'D:/x/take0001';
  const all = [
    job({ id: '0', status: 'Queued', take_folder: TAKE_A }),
    job({ id: '1', status: 'Rendering', take_folder: TAKE_A, stream: 'HUD ONLY' }),
    job({ id: '2', status: 'Finished', take_folder: TAKE_B }),
    job({ id: '3', status: 'Error', take_folder: TAKE_B, stream: 'HUD ONLY' }),
    job({ id: '4', status: 'Cancelled', take_folder: 'D:/x/take0002' }),
  ];

  it('cancel, retry and remove follow each row\'s status', () => {
    const t = bulkTargets(all, all);
    expect(t.cancel).toEqual(['0', '1']);
    expect(t.retry).toEqual(['2', '3', '4']);
    expect(t.remove).toEqual(['0', '2', '3', '4']);
  });

  it('delete lists each ticked take once, skipping a take that is rendering', () => {
    const t = bulkTargets(all, all);
    expect(t.deleteFolders).toEqual([TAKE_B, 'D:/x/take0002']);
    // Only the Finished half of take B and the Error half ticked: one take.
    expect(bulkTargets([all[2], all[3]], all).deleteFolders).toEqual([TAKE_B]);
    // Take A's HUD half is still rendering, so ticking its Queued half
    // deletes nothing; once that half is only Queued, the take goes.
    expect(bulkTargets([all[0]], all).deleteFolders).toEqual([]);
    const queuedPair = [all[0], { ...all[1], status: 'Queued' }];
    expect(bulkTargets([all[0]], queuedPair).deleteFolders).toEqual([TAKE_A]);
  });

  it('nothing ticked, nothing to do', () => {
    expect(bulkTargets([], all)).toEqual({ cancel: [], retry: [], remove: [], deleteFolders: [] });
  });
});
