import { describe, it, expect, vi } from 'vitest';
import { recordCapture } from './take_record.js';
import { rendersOf } from './take_renders.js';
import { createProjectRenders, knownTakeFolders } from './project_renders.js';

const finished = (started) => ({
  started_unix_ms: started, stream: 'all', codec: 'prores', fps: 60, outcome: 'finished',
  output_path: `D:/exports/clip_${started}.mov`, output_exists: true,
});

function setUp(states) {
  const index = { 's/b0': ['u'], 's/b1': ['v'], 'old/b0': ['w'] };
  recordCapture(index, 's/b0', { take_folder: 'D:/caps/s/b0' });
  recordCapture(index, 's/b1', { take_folder: 'D:/caps/s/b1' });
  let current = index;
  const markDirty = vi.fn();
  let handler = null;
  const io = {
    listen: (name, fn) => { if (name === 'render_take_finished') handler = fn; return Promise.resolve(() => {}); },
    fetchStates: vi.fn(async () => states),
    log: vi.fn(),
    now: () => 0,
  };
  const renders = createProjectRenders({ getTakeIndex: () => current, markDirty, io });
  return { index, renders, markDirty, io, fire: (payload) => handler({ payload }), swap: (next) => { current = next; } };
}

describe('knownTakeFolders', () => {
  it('lists only the takes whose folder the project recorded', () => {
    expect(knownTakeFolders(setUp([]).index)).toEqual([
      { takeKey: 's/b0', folder: 'D:/caps/s/b0' }, { takeKey: 's/b1', folder: 'D:/caps/s/b1' },
    ]);
  });
});

describe('a render finishing', () => {
  it('is recorded for a take the project knows, logged, and marks the project unsaved', () => {
    const { index, renders, markDirty, io, fire } = setUp([]);
    renders.startRecording();
    fire({ take_key: 's/b0', render: finished(5) });
    expect(rendersOf(index, 's/b0')).toHaveLength(1);
    expect(markDirty).toHaveBeenCalledTimes(1);
    expect(io.log.mock.calls[0][0]).toContain('s/b0');
  });

  it('changes nothing for a take the project does not know, or a payload from an older backend', () => {
    const { renders, markDirty } = setUp([]);
    renders.noteFinished({ take_key: 'elsewhere/b0', render: finished(5) });
    renders.noteFinished({ take_key: 's/b0' });
    renders.noteFinished(undefined);
    expect(markDirty).not.toHaveBeenCalled();
  });
});

describe('opening a project', () => {
  it("picks up the take folders' finished renders it does not have yet", async () => {
    const { index, renders, markDirty, io } = setUp([
      { exists: true, renders: [finished(1), { ...finished(2), outcome: 'failed', output_path: '' }] },
      { exists: false, renders: [] },
    ]);
    expect(await renders.pickUpTakeFolderRenders()).toBe(1);
    expect(io.fetchStates).toHaveBeenCalledWith(['D:/caps/s/b0', 'D:/caps/s/b1']);
    expect(rendersOf(index, 's/b0').map((r) => r.started_unix_ms)).toEqual([1]);
    expect(markDirty).toHaveBeenCalledTimes(1);
    expect(await renders.pickUpTakeFolderRenders()).toBe(0);
    expect(markDirty).toHaveBeenCalledTimes(1);
  });

  it('drops the answer when another project opened meanwhile', async () => {
    const setup = setUp([{ exists: true, renders: [finished(1)] }, { exists: true, renders: [] }]);
    setup.io.fetchStates = vi.fn(async () => { setup.swap({}); return [{ exists: true, renders: [finished(1)] }, { exists: true, renders: [] }]; });
    expect(await setup.renders.pickUpTakeFolderRenders()).toBe(0);
    expect(rendersOf(setup.index, 's/b0')).toEqual([]);
  });

  it('logs a failed check and changes nothing', async () => {
    const { renders, markDirty, io } = setUp(null);
    expect(await renders.pickUpTakeFolderRenders()).toBe(0);
    expect(markDirty).not.toHaveBeenCalled();
    expect(io.log).toHaveBeenCalledTimes(1);
  });
});
