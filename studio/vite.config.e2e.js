// Vite config used only for the Playwright frontend test harness
// (tests/e2e/). Never used by `npm run dev`/`build`/`tauri dev` — those keep
// using Vite's zero-config defaults against the real `@tauri-apps/*`
// packages. This one exists solely to redirect the Tauri IPC imports panel
// code makes to the mocks in tests/e2e/mocks/, so the same unmodified
// render_pane.js/ipc_bridge.js can run in a plain browser with no Rust
// backend. See tests/e2e/README.md.
import { defineConfig } from 'vite';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  resolve: {
    alias: {
      '@tauri-apps/api/core': path.resolve(__dirname, 'tests/e2e/mocks/tauri-core.js'),
      '@tauri-apps/api/event': path.resolve(__dirname, 'tests/e2e/mocks/tauri-event.js'),
      '@tauri-apps/api/app': path.resolve(__dirname, 'tests/e2e/mocks/tauri-app.js'),
      '@tauri-apps/api/window': path.resolve(__dirname, 'tests/e2e/mocks/tauri-window.js'),
    },
  },
});
