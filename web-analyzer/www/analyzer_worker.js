// Runs the wasm analyzer off the page's thread, so the page keeps drawing
// and answering clicks while a demo is parsed. One worker, so one wasm
// instance, serves every demo: memory it grew for one demo is reused by the
// next. Messages: in { id, file }; out { id, type: 'progress', pct },
// { id, type: 'done', analysis } or { id, type: 'error', error, fatal }.
import init, { analyzeDemo } from './pkg/web_analyzer.js';
import { createProgressThrottle } from './progress.js';

const ready = init({ module_or_path: new URL('./pkg/web_analyzer_bg.wasm', import.meta.url) });

self.onmessage = async ({ data }) => {
  const { id, file } = data;
  try {
    await ready;
    const bytes = new Uint8Array(await file.arrayBuffer());
    const report = createProgressThrottle((pct) => self.postMessage({ id, type: 'progress', pct }));
    const analysis = analyzeDemo(bytes, report);
    self.postMessage({ id, type: 'done', analysis });
  } catch (err) {
    // A trap (RuntimeError) means the instance may be broken; the page then
    // replaces this worker. A parse error comes back as a plain string.
    const fatal = err instanceof WebAssembly.RuntimeError || err instanceof WebAssembly.CompileError;
    self.postMessage({ id, type: 'error', error: String(err), fatal });
  }
};
