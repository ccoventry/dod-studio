import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./ipc_bridge.js', () => ({ logFrontendEvent: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const { createBatchGuard } = await import('./batch_guard.js');
const { STRINGS } = await import('./strings.js');

/** A control as the guard handles it. */
function fakeControl(title = 'Its own tooltip') {
  const classes = new Set();
  const attributes = new Map();
  return {
    title,
    classes,
    attributes,
    classList: { add: (c) => classes.add(c), remove: (c) => classes.delete(c) },
    setAttribute: (name, value) => attributes.set(name, value),
    removeAttribute: (name) => attributes.delete(name),
  };
}

/** A click on an element matching `selector`, or not when it's null. */
function fakeClick(matches) {
  return {
    target: { closest: () => (matches ? {} : null) },
    stopImmediatePropagation: vi.fn(),
    preventDefault: vi.fn(),
  };
}

function setUp({ isStarting } = {}) {
  const load = fakeControl();
  const page = { '#load-project-btn': load };
  let clickHandler = null;
  globalThis.document = {
    querySelector: (selector) => page[selector] ?? null,
    addEventListener: (type, handler, capture) => {
      expect([type, capture]).toEqual(['click', true]);
      clickHandler = handler;
    },
  };
  let report = null;
  const toast = vi.fn();
  const log = vi.fn();
  let clock = 1000;
  const guard = createBatchGuard({
    isStarting,
    toast,
    log,
    now: () => clock,
    listen: (name, handler) => {
      expect(name).toBe('capture_status');
      report = handler;
      return Promise.resolve(() => {});
    },
  });
  return {
    guard, load, toast, log,
    status: (running) => report({ payload: { running } }),
    click: (event) => clickHandler(event),
    advance: (ms) => { clock += ms; },
  };
}

describe('between batches', () => {
  let t;
  beforeEach(() => { t = setUp(); });

  it('holds nothing and lets clicks through', () => {
    const click = fakeClick(true);
    t.click(click);
    expect(t.guard.isHeld()).toBe(false);
    expect(t.guard.heldReason()).toBe(null);
    expect(click.stopImmediatePropagation).not.toHaveBeenCalled();
  });

  it('refuses nothing', () => {
    expect(t.guard.refuseDuringBatch()).toBe(false);
    expect(t.toast).not.toHaveBeenCalled();
  });
});

describe('while a batch runs', () => {
  let t;
  beforeEach(() => {
    t = setUp();
    t.status(true);
  });

  it('greys each control and gives it the reason as its tooltip', () => {
    expect(t.load.classes.has('batch-held')).toBe(true);
    expect(t.load.title).toBe(STRINGS.BATCH_GUARD.HELD_TITLE);
    expect(t.load.attributes.get('aria-disabled')).toBe('true');
  });

  it('stops a click on a held control and says why', () => {
    const click = fakeClick(true);
    t.click(click);
    expect(click.stopImmediatePropagation).toHaveBeenCalled();
    expect(t.toast).toHaveBeenCalledWith(STRINGS.BATCH_GUARD.REFUSED_TOAST, 'info');
  });

  it('leaves every other click alone', () => {
    const click = fakeClick(false);
    t.click(click);
    expect(click.stopImmediatePropagation).not.toHaveBeenCalled();
    expect(t.toast).not.toHaveBeenCalled();
  });

  it('turns away an open that comes through no control', () => {
    expect(t.guard.refuseDuringBatch()).toBe(true);
  });

  it('gives the reason for the rows Open Recent builds later', () => {
    expect(t.guard.heldReason()).toBe(STRINGS.BATCH_GUARD.HELD_TITLE);
  });

  it('stays held through the running reports that follow, a cancelled batch\'s take check too', () => {
    const before = t.log.mock.calls.length;
    t.status(true);
    t.status(true);
    expect(t.guard.isHeld()).toBe(true);
    expect(t.log.mock.calls.length).toBe(before);
  });
});

describe('when the batch ends', () => {
  it('puts every control back as it was, tooltip included', () => {
    const t = setUp();
    t.status(true);
    t.status(false);
    expect(t.load.classes.size).toBe(0);
    expect(t.load.title).toBe('Its own tooltip');
    expect(t.load.attributes.size).toBe(0);
    expect(t.guard.isHeld()).toBe(false);
  });

  it('logs how long it held and how many attempts it refused', () => {
    const t = setUp();
    t.status(true);
    t.advance(5000);
    t.guard.refuseDuringBatch();
    t.status(false);
    const lines = t.log.mock.calls.map(([line]) => line);
    expect(lines.at(-1)).toBe('[project] the batch ended after 5000 ms: released, 1 attempt(s) refused');
  });

  it('tells each listener when a batch starts and ends', () => {
    const t = setUp();
    const listener = vi.fn();
    t.guard.onChange(listener);
    t.status(true);
    t.status(false);
    expect(listener.mock.calls).toEqual([[true], [false]]);
  });
});

describe('while Start Capture runs its checks', () => {
  it('refuses a project change before the batch\'s first report', () => {
    const t = setUp({ isStarting: () => true });
    expect(t.guard.isHeld()).toBe(true);
    expect(t.guard.refuseDuringBatch()).toBe(true);
  });
});
