// Test-only stand-in for `@tauri-apps/api/webview`, paired with `tauri-core.js`.
// demo_drop.js subscribes to file drops on the current webview; no test drops a
// real file, so the handler is registered and never fired.
export function getCurrentWebview() {
  return { onDragDropEvent: () => Promise.resolve(() => {}) };
}
