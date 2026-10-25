import { describe, it, expect, vi } from 'vitest';

// The command runs off the window's thread (#873), so ipc_bridge sends the
// lines one at a time to keep them in order.
const sent = [];
const answers = [];
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn((cmd, { message }) => {
    sent.push(message);
    return new Promise((resolve, reject) => answers.push({ resolve, reject }));
  }),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn(async () => '0.0.0') }));
vi.mock('@tauri-apps/plugin-opener', () => ({ revealItemInDir: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const { logFrontendEvent } = await import('./ipc_bridge.js');
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('logFrontendEvent', () => {
  it('sends the next line only once the last one is written, and a failure stops nothing', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    logFrontendEvent('one');
    logFrontendEvent('two');
    logFrontendEvent('three');
    await settle();
    expect(sent).toEqual(['one']);
    answers[0].reject(new Error('disk'));
    await settle();
    expect(sent).toEqual(['one', 'two']);
    answers[1].resolve();
    await settle();
    expect(sent).toEqual(['one', 'two', 'three']);
    expect(error).toHaveBeenCalledTimes(1);
    error.mockRestore();
  });
});
