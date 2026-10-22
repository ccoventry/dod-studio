# studio

The active `dod-studio` desktop app — Tauri v2 backend (`src-tauri/`) + Vite/vanilla-JS
frontend (`src/`, one module per pane: `capture_pane.js`, `render_pane.js`,
`analyzer_pane.js`, `auditor_pane.js`, `detail_pane.js`, `hd_pane.js`,
`master_pane.js`, `updater_pane.js`; all backend calls go through `ipc_bridge.js`).

Workspace context lives in the repo root [`README.md`](../README.md) and
[`CLAUDE.md`](../CLAUDE.md).

    npm install
    npm run tauri dev     # Launch the Tauri window with Vite HMR
    npm run dev            # Vite dev server only, no Tauri window
    npm run build           # Production Vite build
    npm run test:unit      # vitest, src/*.test.js
    npm run test:e2e       # Playwright, see tests/e2e/README.md

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
