import { describe, it, expect, beforeAll } from 'vitest';

// public/boot_watchdog.js is a classic script: importing it only defines
// `globalThis.dodStudioBootWatchdog` (it starts itself only in the page).
let W;
beforeAll(async () => {
  await import('../public/boot_watchdog.js');
  W = globalThis.dodStudioBootWatchdog;
});

function harness({ reloaded = false } = {}) {
  const h = {
    time: 0,
    timers: [],
    saved: reloaded ? { [W.RELOAD_KEY]: '1' } : {},
    lines: [],
    banner: null,
    reloads: 0,
  };
  h.watchdog = W.create({
    deadlineMs: 20000,
    now: () => h.time,
    setTimer: (fn, ms) => { h.timers.push({ fn, ms, live: true }); return h.timers.length - 1; },
    clearTimer: (id) => { h.timers[id].live = false; },
    store: {
      get: (k) => h.saved[k] ?? null,
      set: (k, v) => { h.saved[k] = v; },
      remove: (k) => { delete h.saved[k]; },
    },
    log: (line) => { h.lines.push(line); return Promise.resolve(); },
    show: (text) => { h.banner = text; },
    hide: () => { h.banner = null; },
    reload: () => { h.reloads += 1; },
    countScripts: () => 76,
  });
  h.fire = async () => {
    for (const t of h.timers) if (t.live) { t.live = false; t.fn(); }
    await new Promise((r) => setTimeout(r, 0));
  };
  return h;
}

describe('boot watchdog', () => {
  it('a normal start logs how long it took and never reloads', async () => {
    const h = harness();
    h.time = 850;
    h.watchdog.done();
    await h.fire();
    expect(h.watchdog.state()).toBe('started');
    expect(h.lines).toEqual([W.TEXT.started(850)]);
    expect(h.reloads).toBe(0);
    expect(h.banner).toBeNull();
  });

  it('a start that stops short reloads once, saying so', async () => {
    const h = harness();
    await h.fire();
    expect(h.timers[0].ms).toBe(20000);
    expect(h.watchdog.state()).toBe('reloading');
    expect(h.banner).toBe(W.TEXT.BANNER_RELOADING);
    expect(h.lines).toEqual([W.TEXT.stuck(20, 76)]);
    expect(h.saved[W.RELOAD_KEY]).toBe('1');
    expect(h.reloads).toBe(1);
  });

  it('stuck again after its reload: says to restart and never loops', async () => {
    const h = harness({ reloaded: true });
    await h.fire();
    expect(h.watchdog.state()).toBe('stuck');
    expect(h.banner).toBe(W.TEXT.BANNER_RESTART);
    expect(h.lines).toEqual([W.TEXT.stuckAgain(20, 76)]);
    expect(h.reloads).toBe(0);
  });

  it('a start after its reload clears the marker, so the next stuck start reloads again', async () => {
    const h = harness({ reloaded: true });
    h.time = 1200;
    h.watchdog.done();
    expect(h.lines).toEqual([W.TEXT.startedAfterReload(1200)]);
    expect(h.saved[W.RELOAD_KEY]).toBeUndefined();
  });

  it('a start that finishes late takes the banner down and counts once', async () => {
    const h = harness({ reloaded: true });
    await h.fire();
    h.time = 30000;
    h.watchdog.done();
    h.watchdog.done();
    expect(h.banner).toBeNull();
    expect(h.watchdog.state()).toBe('started');
    expect(h.lines).toEqual([W.TEXT.stuckAgain(20, 76), W.TEXT.startedAfterReload(30000)]);
  });

  it('a start that finishes while the reload line is going out stays', async () => {
    const h = harness();
    for (const t of h.timers) { t.live = false; t.fn(); }
    h.watchdog.done();
    await new Promise((r) => setTimeout(r, 0));
    expect(h.reloads).toBe(0);
    expect(h.watchdog.state()).toBe('started');
  });
});
