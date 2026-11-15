import { describe, it, expect, vi } from 'vitest';
import { layoutWarning, startKillfeedLayoutWarning } from './killfeed_layout_warning.js';

const WARNING = "Kill feed: the game's own layout is showing, not the even one: the kill-feed atlas has no HD frames for the ultrasharp HD style.";

describe("kill feed: the even layout can't draw (#1339)", () => {
  it('shows a warning, toasts each one once, and clears on none', () => {
    const said = new Set();
    expect(layoutWarning({ warning: WARNING }, said)).toEqual({ show: true, text: WARNING, toast: true });
    said.add(WARNING);
    expect(layoutWarning({ warning: WARNING }, said).toast).toBe(false);
    expect(layoutWarning({ warning: null }, said).show).toBe(false);
    expect(layoutWarning({ warning: '  ' }, said).show).toBe(false);
    expect(layoutWarning(undefined, said).show).toBe(false);
  });

  it('keeps the line after the batch, clears it at the next start, and starts only once', () => {
    const handlers = {};
    const listen = vi.fn((name, fn) => {
      handlers[name] = fn;
      return Promise.resolve(() => {});
    });
    const el = { hidden: true, textContent: '', title: '' };
    const toast = vi.fn();
    startKillfeedLayoutWarning({ el, listen, toast });
    startKillfeedLayoutWarning({ el, listen, toast });
    expect(listen).toHaveBeenCalledTimes(2);

    handlers.capture_status({ payload: { running: true } });
    handlers.capture_killfeed_layout({ payload: { warning: WARNING } });
    expect(el.hidden).toBe(false);
    expect(el.textContent).toBe(WARNING);
    expect(el.title).not.toBe('');
    // The same reason again (a batch start re-sends it): no second toast.
    handlers.capture_killfeed_layout({ payload: { warning: WARNING } });
    expect(toast).toHaveBeenCalledTimes(1);

    // Back from an overnight batch: the line is still there.
    handlers.capture_status({ payload: { running: false } });
    expect(el.hidden).toBe(false);

    // The next batch starts clean, and warns again.
    handlers.capture_status({ payload: { running: true } });
    expect(el.hidden).toBe(true);
    handlers.capture_killfeed_layout({ payload: { warning: WARNING } });
    expect(toast).toHaveBeenCalledTimes(2);

    // Fixed mid-batch: the even layout draws.
    handlers.capture_killfeed_layout({ payload: { warning: null } });
    expect(el.hidden).toBe(true);
    expect(el.textContent).toBe('');
  });
});
