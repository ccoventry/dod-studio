import { describe, it, expect, vi, beforeEach } from 'vitest';

const dialog = vi.hoisted(() => ({ answer: true, calls: [] }));
vi.mock('./themed_confirm.js', () => ({
  themedConfirm: vi.fn(async (message, opts) => {
    dialog.calls.push({ message, opts });
    return dialog.answer;
  }),
}));
vi.mock('./ipc_bridge.js', () => ({ getRenderJobs: vi.fn(), logFrontendEvent: vi.fn() }));

import { confirmCloseDuringRenders, countRendering } from './render_close_prompt.js';
import { STRINGS } from './strings.js';

const s = STRINGS.RENDER_CLOSE_MODAL;
const rows = (...statuses) => statuses.map((status, i) => ({ id: String(i), status }));

beforeEach(() => {
  dialog.calls = [];
  dialog.answer = true;
});

describe('countRendering', () => {
  it('counts only the rows rendering now', () => {
    expect(countRendering(rows('Rendering', 'Queued', 'Finished', 'Rendering', 'Error'))).toBe(2);
  });

  it('counts nothing when the list could not be read', () => {
    expect(countRendering(null)).toBe(0);
  });
});

describe('confirmCloseDuringRenders', () => {
  it('closes without asking when nothing is rendering', async () => {
    const log = vi.fn();
    const ok = await confirmCloseDuringRenders({ fetchJobs: async () => rows('Queued', 'Finished'), log });
    expect(ok).toBe(true);
    expect(dialog.calls).toHaveLength(0);
    expect(log).not.toHaveBeenCalled();
  });

  it('closes without asking when the job list cannot be read', async () => {
    const ok = await confirmCloseDuringRenders({ fetchJobs: async () => null, log: vi.fn() });
    expect(ok).toBe(true);
    expect(dialog.calls).toHaveLength(0);
  });

  it('asks with a red yes that says the renders stop, and logs the answer', async () => {
    const log = vi.fn();
    const ok = await confirmCloseDuringRenders({ fetchJobs: async () => rows('Rendering', 'Rendering', 'Queued'), log });
    expect(ok).toBe(true);
    expect(dialog.calls).toHaveLength(1);
    const { message, opts } = dialog.calls[0];
    expect(message).toBe(s.message(2));
    expect(opts).toMatchObject({ title: s.TITLE, confirmLabel: s.CLOSE_BUTTON, cancelLabel: s.KEEP_OPEN_BUTTON, destructive: true });
    expect(log).toHaveBeenCalledWith(s.logLine(2, true));
  });

  it('keeps Studio open when the user says so', async () => {
    dialog.answer = false;
    const log = vi.fn();
    const ok = await confirmCloseDuringRenders({ fetchJobs: async () => rows('Rendering'), log });
    expect(ok).toBe(false);
    expect(log).toHaveBeenCalledWith(s.logLine(1, false));
  });

  it('says one render or several in plain words', () => {
    expect(s.message(1)).toMatch(/^A render is running\./);
    expect(s.message(3)).toMatch(/^3 renders are running\./);
  });
});
