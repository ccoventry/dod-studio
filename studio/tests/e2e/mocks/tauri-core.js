// Test-only stand-in for `@tauri-apps/api/core`, used solely by the
// Playwright harness — `vite.config.e2e.js` aliases the real import to this
// file so panel code (render_pane.js, ipc_bridge.js) runs completely
// unmodified against it. See tests/e2e/README.md.
//
// A test controls what a command resolves to via `window.__mockInvokeHandlers`
// (set through `page.addInitScript`/`page.evaluate` before the call happens),
// and can inspect what was actually called via `window.__mockInvocations`.
window.__mockInvocations = window.__mockInvocations || [];
window.__mockInvokeHandlers = window.__mockInvokeHandlers || {};

// What the boot-time commands answer when a test did not stub them: the empty
// answer of the real command, so a pane that reads the result (`.map`, `.demos`,
// `for..of`) boots instead of throwing on `undefined`. A test still overrides
// any of these through `__mockInvokeHandlers`, which it often replaces whole.
const DEFAULT_HANDLERS = {
  diagnose_executable_paths: () => [],
  diagnose_capture_output_paths: () => [],
  scan_demo_folders: () => [],
  overview_installs: () => [],
  get_export_pool_free_gb: () => 0,
  browse_directory: (args) => ({ path: args?.path ?? '', dirs: [], subdirs: [], demos: [] }),
  // No HD folder: the pane shows its own error line.
  hd_status: () => Promise.reject('hd_status is not stubbed'),
};

export function invoke(cmd, args) {
  window.__mockInvocations.push({ cmd, args });
  const handler = window.__mockInvokeHandlers[cmd] || DEFAULT_HANDLERS[cmd];
  if (handler) {
    const result = handler(args);
    return result instanceof Promise ? result : Promise.resolve(result);
  }
  // An unregistered command resolves to `undefined` rather than rejecting —
  // this harness only cares about Render Studio, and other panels' own
  // boot-time IPC calls (settings, demo scans, ...) must not crash the page
  // just because a test did not bother to stub them.
  return Promise.resolve(undefined);
}

// Only needed so `@tauri-apps/plugin-notification`'s static import of this
// symbol resolves during bundling — no test currently exercises a plugin
// event listener, so this never needs to actually invoke `cb`.
export function addPluginListener(plugin, event, cb) {
  return Promise.resolve({ plugin, event, channelId: 0, unregister: () => Promise.resolve() });
}
