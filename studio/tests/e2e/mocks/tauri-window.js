// Test-only stand-in for `@tauri-apps/api/window`, paired with `tauri-core.js`.
// Covers exactly what main.js/updater_pane.js call on the current window:
// onCloseRequested (registers a handler, never fired in tests), destroy,
// and setTitle.
export function getCurrentWindow() {
  return {
    onCloseRequested: () => Promise.resolve(() => {}),
    destroy: () => Promise.resolve(),
    setTitle: (title) => {
      document.title = title;
      return Promise.resolve();
    },
  };
}
