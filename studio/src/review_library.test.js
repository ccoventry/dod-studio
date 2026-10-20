import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ reviewLibraryGet: vi.fn(), reviewLibraryUpdate: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));
vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn(async () => true) }));

import {
  createReviewLibrary, demoKey, highlightId, libraryEntry, matchSavedMarks, workSignature,
} from './review_library.js';
import { applyReviewAnswer } from './review_queue.js';
import { streakUid } from './take_index.js';

const KEY = '123456:abcdef';
const hl = (kills, extra = {}) => ({
  player_index: 3,
  target_player: 'm00cat',
  steam_id: '76561197972576011',
  source_key: KEY,
  kills,
  victims: kills.map((k) => `shot with ${k[2]}`),
  ...extra,
});
const KILLS_A = [[100, 1, 'K98'], [200, 3, 'K98'], [300, 5, 'BAR']];
const KILLS_B = [[900, 20, 'Grenade'], [1000, 22, 'Colt']];
const demo = (path, streaks) => ({ path, streaks });

/** A library held in memory, the shape review_library.rs keeps. */
function memoryBridge() {
  const demos = {};
  return {
    demos,
    get: vi.fn(async (keys) => Object.fromEntries(keys.filter((k) => demos[k]).map((k) => [k, demos[k]]))),
    update: vi.fn(async (key, name, changes) => {
      const d = demos[key] || (demos[key] = { name, highlights: {} });
      Object.entries(changes).forEach(([id, e]) => { if (e) d.highlights[id] = e; else delete d.highlights[id]; });
      if (Object.keys(d.highlights).length === 0) delete demos[key];
    }),
  };
}

describe('what the library keeps of a highlight', () => {
  it('keys a demo on its contents and a highlight on its kills, not on the path', () => {
    const s = hl(KILLS_A);
    expect(demoKey(demo('C:/a.dem', [s]))).toBe(KEY);
    expect(demoKey(demo('C:/old.dem', [{ kills: [] }]))).toBeNull();
    expect(highlightId(s)).toBe('3#100#300');
  });

  it('keeps the mark, note and Kill Range, and nothing for a highlight with none', () => {
    expect(libraryEntry(hl(KILLS_A))).toBeNull();
    expect(libraryEntry(hl(KILLS_A, { selected: true, status: 'Captured' }))).toBeNull();
    const entry = libraryEntry(hl(KILLS_A, { curation: 'Keep', notes: 'nice', start_index: 1 }));
    expect(entry).toMatchObject({ curation: 'Keep', notes: 'nice', start_index: 1, end_index: 2, kills: KILLS_A, steam_id: '76561197972576011' });
    expect(entry.status).toBeUndefined();
    expect(workSignature(hl(KILLS_A))).toBe('');
  });
});

describe('finding saved marks on a demo scanned again', () => {
  it('finds them on a copy under another name, by its kills', () => {
    const saved = { highlights: { '3#100#300': libraryEntry(hl(KILLS_A, { curation: 'Maybe', notes: 'look again' })) } };
    const target = hl(KILLS_A);
    const { previous, carried } = matchSavedMarks(saved, demo('D:/renamed.dem', [target, hl(KILLS_B)]));
    expect(carried).toHaveLength(1);
    expect(carried[0].to).toBe(target);
    expect(previous.path).toBe('D:/renamed.dem');
  });

  it("never covers the demo's own work", () => {
    const saved = { highlights: { '3#100#300': libraryEntry(hl(KILLS_A, { curation: 'Skip' })) } };
    const { carried } = matchSavedMarks(saved, demo('C:/a.dem', [hl(KILLS_A, { curation: 'Keep' })]));
    expect(carried).toEqual([]);
  });
});

describe('createReviewLibrary', () => {
  const setup = ({ demos, project = false, ask = vi.fn(async () => true), bridge = memoryBridge() } = {}) => {
    const queue = { demos: demos || [] };
    const onBroughtIn = vi.fn();
    const library = createReviewLibrary({
      getDemos: () => queue.demos,
      hasProjectFile: () => project,
      onBroughtIn,
      bridge,
      ask,
      saveDelayMs: 0,
    });
    return { queue, library, bridge, onBroughtIn, ask };
  };

  it('saves a review answer given with no project, and only what changed', async () => {
    const a = hl(KILLS_A);
    const b = hl(KILLS_B);
    const d = demo('C:/demos/a.dem', [a, b]);
    const { library, bridge } = setup({ demos: [d] });
    await library.sync(); // the scan that added it
    expect(bridge.update).not.toHaveBeenCalled();

    applyReviewAnswer([d], { demo: d.path, key: streakUid(d.path, a), verdict: 'yes', from: 2, to: 3, note: 'triple' });
    library.sync();
    await library.flush();
    expect(bridge.update).toHaveBeenCalledTimes(1);
    expect(bridge.update).toHaveBeenCalledWith(KEY, 'a.dem', { '3#100#300': expect.objectContaining({ curation: 'Keep', notes: 'triple', start_index: 1 }) });

    library.sync();
    await library.flush();
    expect(bridge.update).toHaveBeenCalledTimes(1);
  });

  it('removes a highlight whose mark was cleared', async () => {
    const a = hl(KILLS_A);
    const d = demo('C:/demos/a.dem', [a]);
    const { library, bridge } = setup({ demos: [d] });
    await library.sync();
    a.curation = 'Keep';
    library.sync();
    await library.flush();
    delete a.curation;
    library.sync();
    await library.flush();
    expect(bridge.demos).toEqual({});
  });

  it('brings saved marks onto a copy a later scan adds, with no project, without asking', async () => {
    const bridge = memoryBridge();
    bridge.demos[KEY] = { name: 'a.dem', highlights: { '3#100#300': libraryEntry(hl(KILLS_A, { curation: 'Skip', notes: 'whiff' })) } };
    const target = hl(KILLS_A);
    const { library, onBroughtIn, ask } = setup({ demos: [demo('E:/copy.dem', [target])], bridge });
    const outcome = await library.sync();
    expect(ask).not.toHaveBeenCalled();
    expect(outcome.brought).toBe(1);
    expect(target).toMatchObject({ curation: 'Skip', notes: 'whiff' });
    expect(onBroughtIn).toHaveBeenCalledWith([target]);
    // Not written back: it came from the library.
    library.sync();
    await library.flush();
    expect(bridge.update).not.toHaveBeenCalled();
  });

  it('asks first in an open project, and "Not now" changes nothing', async () => {
    const bridge = memoryBridge();
    bridge.demos[KEY] = { name: 'a.dem', highlights: { '3#100#300': libraryEntry(hl(KILLS_A, { curation: 'Keep' })) } };
    const target = hl(KILLS_A);
    const ask = vi.fn(async () => false);
    const { library, onBroughtIn } = setup({ demos: [demo('C:/a.dem', [target])], bridge, project: true, ask });
    const outcome = await library.sync();
    expect(ask).toHaveBeenCalledWith([expect.objectContaining({ carried: [expect.anything()] })], 1);
    expect(outcome.brought).toBe(0);
    expect(target.curation).toBeUndefined();
    expect(onBroughtIn).not.toHaveBeenCalled();
  });

  it("doesn't offer anything for a loaded project's demos", async () => {
    const bridge = memoryBridge();
    bridge.demos[KEY] = { name: 'a.dem', highlights: { '3#100#300': libraryEntry(hl(KILLS_A, { curation: 'Keep' })) } };
    const { library, ask } = setup({ demos: [demo('C:/a.dem', [hl(KILLS_A)])], bridge, project: true });
    library.noteLoaded();
    expect(await library.sync()).toBeNull();
    expect(bridge.get).not.toHaveBeenCalled();
    expect(ask).not.toHaveBeenCalled();
  });

  it('a demo with no contents key (an older scan) is neither saved nor looked up', async () => {
    const s = { player_index: 0, kills: KILLS_A };
    const d = demo('C:/old.dem', [s]);
    const { library, bridge } = setup({ demos: [d] });
    await library.sync();
    s.curation = 'Keep';
    library.sync();
    await library.flush();
    expect(bridge.get).not.toHaveBeenCalled();
    expect(bridge.update).not.toHaveBeenCalled();
  });
});
