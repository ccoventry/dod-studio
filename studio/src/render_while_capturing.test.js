import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve()) }));
vi.mock('./ipc_bridge.js', () => ({ queueRenderBatch: vi.fn(), startQueuedRender: vi.fn() }));
vi.mock('./finish_clips.js', () => ({ renderPayloadFor: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const {
  settingHint, alreadyRendering, scanBusy, createClipRenderQueue, roomStatusLine,
} = await import('./render_while_capturing.js');
const { STRINGS } = await import('./strings.js');

describe('settingHint', () => {
  it('says clips render after the batch while off', () => {
    expect(settingHint(false, 'delete')).toBe(STRINGS.RENDER_WHILE_CAPTURING.HINT_OFF);
  });
  it('says the batch waits for room only when takes are deleted', () => {
    expect(settingHint(true, 'delete')).toBe(STRINGS.RENDER_WHILE_CAPTURING.HINT_ON_DELETE);
    expect(settingHint(true, 'keep')).toBe(STRINGS.RENDER_WHILE_CAPTURING.HINT_ON_KEEP);
    expect(settingHint(true, 'recycle')).toBe(STRINGS.RENDER_WHILE_CAPTURING.HINT_ON_KEEP);
  });
});

describe('the backend errors it tells apart', () => {
  it('reads "already rendering" and "a scan holds the queue"', () => {
    expect(alreadyRendering('Render batch already in progress')).toBe(true);
    expect(alreadyRendering(new Error('Nothing queued to render'))).toBe(false);
    expect(scanBusy('A scan for takes is already running — wait for it to finish.')).toBe(true);
    expect(scanBusy('disk full')).toBe(false);
  });
});

function setup(overrides = {}) {
  const calls = [];
  const deps = {
    queue: vi.fn(async (payload) => { calls.push(['queue', payload.render_directories[0]]); }),
    start: vi.fn(async () => { calls.push(['start']); }),
    payloadFor: (clip) => ({ payload: { render_directories: [clip.take_folder] } }),
    wait: vi.fn(async () => {}),
    onSkip: vi.fn(),
    onError: vi.fn(),
    ...overrides,
  };
  return { calls, deps, q: createClipRenderQueue(deps) };
}

describe('createClipRenderQueue', () => {
  it('queues each take then starts the Render tab, one take at a time in order', async () => {
    const { calls, q } = setup();
    q.clipReady({ take_folder: 'a' });
    await q.clipReady({ take_folder: 'b' });
    expect(calls).toEqual([['queue', 'a'], ['start'], ['queue', 'b'], ['start']]);
  });

  it('a running render batch picks the take up on its own', async () => {
    const { deps, q } = setup({ start: vi.fn(async () => { throw 'Render batch already in progress'; }) });
    await q.clipReady({ take_folder: 'a' });
    expect(deps.onError).not.toHaveBeenCalled();
  });

  it('tries again while a scan the user started holds the queue', async () => {
    let busy = 2;
    const queue = vi.fn(async () => {
      if (busy-- > 0) throw 'A scan for takes is already running — wait for it to finish.';
    });
    const { deps, q } = setup({ queue });
    await q.clipReady({ take_folder: 'a' });
    expect(queue).toHaveBeenCalledTimes(3);
    expect(deps.wait).toHaveBeenCalledTimes(2);
    expect(deps.start).toHaveBeenCalledTimes(1);
    expect(deps.onError).not.toHaveBeenCalled();
  });

  it('reports any other failure and goes on with the next take', async () => {
    let first = true;
    const queue = vi.fn(async () => {
      if (first) { first = false; throw 'no FFmpeg'; }
    });
    const { deps, q } = setup({ queue });
    q.clipReady({ take_folder: 'a' });
    await q.clipReady({ take_folder: 'b' });
    expect(deps.onError).toHaveBeenCalledWith('no FFmpeg');
    expect(deps.start).toHaveBeenCalledTimes(1);
  });

  it('a take that cannot be rendered is skipped with the reason', async () => {
    const { deps, q } = setup({ payloadFor: () => ({ skip: 'no_export_dir' }) });
    await q.clipReady({ take_folder: 'a' });
    expect(deps.onSkip).toHaveBeenCalledWith('no_export_dir');
    expect(deps.queue).not.toHaveBeenCalled();
  });
});

describe('roomStatusLine', () => {
  const size = (b) => `${b / 1e9} GB`;
  it('says nothing when the batch is not holding', () => {
    expect(roomStatusLine(null, size)).toBeNull();
    expect(roomStatusLine({}, size)).toBeNull();
  });
  it('says the batch will wait, then that it waits, with the time left when known', () => {
    expect(roomStatusLine({ state: 'holding', short_bytes: 4e9 }, size))
      .toBe(STRINGS.RENDER_WHILE_CAPTURING.holdingLine('4 GB'));
    expect(roomStatusLine({ state: 'held', short_bytes: 4e9, eta_seconds: 150 }, size))
      .toBe(STRINGS.RENDER_WHILE_CAPTURING.heldLine('4 GB', 3));
    expect(roomStatusLine({ state: 'held', short_bytes: 4e9, eta_seconds: null }, size))
      .toBe(STRINGS.RENDER_WHILE_CAPTURING.heldLine('4 GB', null));
  });
});
