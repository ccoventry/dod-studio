// The New UI switch (#1085): off by default, listeners told now and on each
// flip, a flip logged and saved, a load neither logged nor saved.

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ logFrontendEvent: vi.fn() }));

// Configuration's box, as a stand-in element.
const box = {
  checked: false,
  handlers: {},
  addEventListener(type, fn) { this.handlers[type] = fn; },
  flip(value) { this.checked = value; this.handlers.change(); },
};
globalThis.document = { querySelector: (sel) => (sel === '#config-new-ui' ? box : null) };

const { isNewUiOn, onNewUiChange, loadNewUi, createNewUi } = await import('./new_ui.js');
const ipc = await import('./ipc_bridge.js');

beforeEach(() => {
  loadNewUi(false);
  vi.mocked(ipc.logFrontendEvent).mockClear();
});

describe('the New UI switch', () => {
  it('is off by default, and for a settings file without it', () => {
    expect(isNewUiOn()).toBe(false);
    loadNewUi(undefined);
    expect(isNewUiOn()).toBe(false);
    loadNewUi('yes');
    expect(isNewUiOn()).toBe(false);
  });

  it('tells a listener at once, then on each change only', () => {
    const seen = [];
    const off = onNewUiChange((on) => seen.push(on));
    loadNewUi(true);
    loadNewUi(true);
    loadNewUi(false);
    off();
    loadNewUi(true);
    expect(seen).toEqual([false, true, false]);
  });

  it('shows a loaded value in the box, without logging it', () => {
    loadNewUi(true);
    expect(box.checked).toBe(true);
    expect(ipc.logFrontendEvent).not.toHaveBeenCalled();
  });

  it('a flip in the box turns it on, logs one line and saves', () => {
    const persist = vi.fn();
    createNewUi({ persist });
    const seen = [];
    onNewUiChange((on) => seen.push(on));
    box.flip(true);
    expect(isNewUiOn()).toBe(true);
    expect(seen).toEqual([false, true]);
    expect(persist).toHaveBeenCalledTimes(1);
    expect(vi.mocked(ipc.logFrontendEvent).mock.calls).toEqual([['[settings] New UI switch turned on: the new pages show']]);
    box.flip(false);
    expect(isNewUiOn()).toBe(false);
    expect(vi.mocked(ipc.logFrontendEvent).mock.calls[1][0]).toContain('off: the new pages are hidden');
  });
});
