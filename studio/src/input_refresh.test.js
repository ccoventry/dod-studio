import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { refreshAfterTyping, REFRESH_AFTER_TYPING_MS } from './input_refresh.js';

describe('refreshAfterTyping (#535)', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const field = () => new EventTarget();
  const fire = (el, type) => el.dispatchEvent(new Event(type));

  it('an undo (an input with no change after it) still refreshes', () => {
    const el = field();
    const refresh = vi.fn();
    refreshAfterTyping(el, refresh);
    fire(el, 'input');
    expect(refresh).not.toHaveBeenCalled();
    vi.advanceTimersByTime(REFRESH_AFTER_TYPING_MS);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('typing refreshes once, after the last keystroke', () => {
    const el = field();
    const refresh = vi.fn();
    refreshAfterTyping(el, refresh);
    for (let i = 0; i < 5; i++) {
      fire(el, 'input');
      vi.advanceTimersByTime(REFRESH_AFTER_TYPING_MS - 1);
    }
    expect(refresh).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('a change refreshes at once and drops the pending one', () => {
    const el = field();
    const refresh = vi.fn();
    refreshAfterTyping(el, refresh);
    fire(el, 'input');
    fire(el, 'change');
    expect(refresh).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(REFRESH_AFTER_TYPING_MS * 2);
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it('no element is a no-op', () => {
    expect(() => refreshAfterTyping(null, () => {})()).not.toThrow();
  });
});
