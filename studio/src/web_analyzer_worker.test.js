// The web analyzer's worker plumbing (web-analyzer/www): the page-side client
// and the progress throttle. Here rather than in web-analyzer/tests because
// this suite runs in CI; the worker itself needs the wasm build.
import { describe, it, expect } from 'vitest';
import { createAnalyzerClient, CANCELLED } from '../../web-analyzer/www/analyzer_client.js';
import { createProgressThrottle } from '../../web-analyzer/www/progress.js';

function fakeWorkers() {
  const made = [];
  const make = () => {
    const w = {
      posted: [],
      terminated: false,
      postMessage(msg) { this.posted.push(msg); },
      terminate() { this.terminated = true; },
      reply(data) { this.onmessage({ data }); },
    };
    made.push(w);
    return w;
  };
  return { made, make };
}

describe('createAnalyzerClient', () => {
  it('posts the file, forwards progress and resolves with the analysis', async () => {
    const { made, make } = fakeWorkers();
    const client = createAnalyzerClient(make);
    const pcts = [];
    const file = { name: 'a.dem' };
    const p = client.analyze(file, (pct) => pcts.push(pct));
    const [{ id, file: sent }] = made[0].posted;
    expect(sent).toBe(file);
    made[0].reply({ id, type: 'progress', pct: 40 });
    made[0].reply({ id, type: 'done', analysis: { ok: 1 } });
    await expect(p).resolves.toEqual({ ok: 1 });
    expect(pcts).toEqual([40]);
  });

  it('reuses one worker across demos, so the wasm memory is reused', async () => {
    const { made, make } = fakeWorkers();
    const client = createAnalyzerClient(make);
    for (let i = 0; i < 3; i++) {
      const p = client.analyze({ name: `${i}.dem` });
      made[0].reply({ id: made[0].posted.at(-1).id, type: 'done', analysis: i });
      await expect(p).resolves.toBe(i);
    }
    expect(made).toHaveLength(1);
  });

  it('keeps the worker after a parse error', async () => {
    const { made, make } = fakeWorkers();
    const client = createAnalyzerClient(make);
    const p = client.analyze({ name: 'bad.dem' });
    made[0].reply({ id: made[0].posted[0].id, type: 'error', error: 'Parse error: x', fatal: false });
    await expect(p).rejects.toBe('Parse error: x');
    expect(made[0].terminated).toBe(false);
    client.analyze({ name: 'next.dem' });
    expect(made).toHaveLength(1);
  });

  it('replaces the worker after a wasm trap', async () => {
    const { made, make } = fakeWorkers();
    const client = createAnalyzerClient(make);
    const p = client.analyze({ name: 'huge.dem' });
    made[0].reply({ id: made[0].posted[0].id, type: 'error', error: 'RuntimeError: unreachable', fatal: true });
    await expect(p).rejects.toBe('RuntimeError: unreachable');
    expect(made[0].terminated).toBe(true);
    client.analyze({ name: 'next.dem' });
    expect(made).toHaveLength(2);
  });

  it('cancel stops the running demo and the next one starts on a new worker', async () => {
    const { made, make } = fakeWorkers();
    const client = createAnalyzerClient(make);
    const p = client.analyze({ name: 'slow.dem' });
    client.cancel();
    await expect(p).rejects.toBe(CANCELLED);
    expect(made[0].terminated).toBe(true);
    // A late reply from the old worker is ignored.
    made[0].reply({ id: made[0].posted[0].id, type: 'done', analysis: 'stale' });
    const q = client.analyze({ name: 'new.dem' });
    made[1].reply({ id: made[1].posted[0].id, type: 'done', analysis: 'fresh' });
    await expect(q).resolves.toBe('fresh');
  });

  it('cancel with nothing running keeps the warm worker', () => {
    const { made, make } = fakeWorkers();
    const client = createAnalyzerClient(make);
    const p = client.analyze({ name: 'a.dem' });
    made[0].reply({ id: made[0].posted[0].id, type: 'done', analysis: 1 });
    return p.then(() => {
      client.cancel();
      expect(made[0].terminated).toBe(false);
    });
  });

  it('an error event from a replaced worker does not touch the new one', async () => {
    const { made, make } = fakeWorkers();
    const client = createAnalyzerClient(make);
    const p = client.analyze({ name: 'slow.dem' });
    client.cancel();
    await expect(p).rejects.toBe(CANCELLED);
    const q = client.analyze({ name: 'new.dem' });
    made[0].onerror({ message: 'terminated', preventDefault() {} });
    expect(made[1].terminated).toBe(false);
    made[1].reply({ id: made[1].posted[0].id, type: 'done', analysis: 'fresh' });
    await expect(q).resolves.toBe('fresh');
  });

  it('a worker that fails to load rejects with its message', async () => {
    const { made, make } = fakeWorkers();
    const client = createAnalyzerClient(make);
    const p = client.analyze({ name: 'a.dem' });
    made[0].onerror({ message: 'Failed to fetch wasm', preventDefault() {} });
    await expect(p).rejects.toBe('Failed to fetch wasm');
  });
});

describe('createProgressThrottle', () => {
  it('sends whole percentages, only when they change and at most every 33 ms', () => {
    let t = 0;
    const sent = [];
    const report = createProgressThrottle((pct) => sent.push(pct), { now: () => t });
    report(1, 100); // 1% at t=0
    report(1, 100); // unchanged
    t = 10; report(5, 100); // too soon
    t = 40; report(6, 100); // 33 ms later
    t = 41; report(100, 100); // 100 always goes through
    report(0, 0); // no total yet
    expect(sent).toEqual([1, 6, 100]);
  });
});
