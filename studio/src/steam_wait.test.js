import { describe, it, expect } from 'vitest';
import { waitForSteam } from './steam_wait.js';

const noSleep = () => Promise.resolve();

describe('waitForSteam', () => {
  it('resolves ready as soon as Steam is signed in', async () => {
    const states = ['signed_out', 'signed_out', 'ready'];
    let calls = 0;
    const result = await waitForSteam(async () => states[calls++], { timeoutMs: 10000, sleep: noSleep });
    expect(result).toBe('ready');
    expect(calls).toBe(3);
  });

  it('gives up after the timeout', async () => {
    let calls = 0;
    const result = await waitForSteam(async () => { calls += 1; return 'signed_out'; }, { timeoutMs: 5000, pollMs: 1000, sleep: noSleep });
    expect(result).toBe('timeout');
    expect(calls).toBe(5);
  });

  it('stops when cancelled, without asking again', async () => {
    let calls = 0;
    let cancelled = false;
    const result = await waitForSteam(
      async () => { calls += 1; cancelled = true; return 'not_running'; },
      { timeoutMs: 10000, sleep: noSleep, isCancelled: () => cancelled }
    );
    expect(result).toBe('cancelled');
    expect(calls).toBe(1);
  });
});
