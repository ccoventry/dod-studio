import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ logFrontendEvent: vi.fn() }));

const { logFrontendEvent } = await import('./ipc_bridge.js');
const { showToast, toastLogLine } = await import('./toast.js');

/** Just enough DOM for showToast: elements that take children and styles. */
function fakeElement() {
  return {
    style: {},
    classList: { add() {} },
    children: [],
    appendChild(child) { this.children.push(child); child.parentNode = this; },
    removeChild(child) { this.children = this.children.filter((c) => c !== child); child.parentNode = null; },
    addEventListener() {},
  };
}

describe('toast logging (#873)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    logFrontendEvent.mockClear();
    const body = fakeElement();
    globalThis.document = {
      body,
      querySelector: () => body.children.find((c) => c.id === 'toast-container') || null,
      createElement: () => fakeElement(),
    };
  });

  afterEach(() => {
    vi.useRealTimers();
    delete globalThis.document;
  });

  it('puts the kind and the text on one line', () => {
    expect(toastLogLine('Saved.', 'success')).toBe('[toast success] Saved.');
    expect(toastLogLine('Could not save:\n  disk full\n', 'error')).toBe('[toast error] Could not save: disk full');
    expect(toastLogLine('Plain')).toBe('[toast info] Plain');
  });

  it('logs every toast as it shows, once', () => {
    showToast('Render failed: no ffmpeg', 'error');
    showToast('Copied');
    expect(logFrontendEvent.mock.calls).toEqual([
      ['[toast error] Render failed: no ffmpeg'],
      ['[toast info] Copied'],
    ]);
    vi.runAllTimers();
    expect(logFrontendEvent).toHaveBeenCalledTimes(2);
  });

  it('shows the same text and kind once while it is on screen (#1097)', () => {
    const shown = () => document.querySelector().children.map((t) => `${t.className}: ${t.textContent}`);
    showToast('No takes were deleted.', 'warning');
    showToast('No takes were deleted.', 'warning');
    showToast('No takes were deleted.', 'error');
    expect(shown()).toEqual([
      'toast toast-warning: No takes were deleted.',
      'toast toast-error: No takes were deleted.',
    ]);
    expect(logFrontendEvent).toHaveBeenCalledTimes(2);
    vi.runAllTimers();
    showToast('No takes were deleted.', 'warning');
    expect(shown()).toEqual(['toast toast-warning: No takes were deleted.']);
  });
});
