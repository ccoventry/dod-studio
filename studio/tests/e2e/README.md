# Frontend e2e tests

Playwright, run in headless Chromium against a minimal test harness
(`render-studio.html`) — not the real app shell. Tauri's IPC (`invoke`/`listen`)
is mocked (`mocks/`, wired in via `../vite.config.e2e.js`), so `render_pane.js`
and `ipc_bridge.js` run completely unmodified with no Rust backend at all.

## Run it

```
npm run test:e2e
```

Playwright starts `npm run dev:e2e` (Vite on port 5183, using
`vite.config.e2e.js`'s IPC aliasing) itself and tears it down after.

## What this covers

Frontend/DOM behavior only: does the right HTML render for a given
`RenderJobView[]` snapshot, does clicking a button call the right IPC command
with the right arguments, does a row survive an unrelated update instead of
being torn down and rebuilt. This is specifically the class of bug that
motivated adding this suite — issue #80 (rapid Cancel clicks getting eaten
during an active render) was a pure DOM-identity bug that unit tests couldn't
have caught and that was previously only verifiable by clicking through the
real app.

## What this does NOT cover

- Real Rust command execution, real `run_render_job`/scanner behavior — that's
  `cargo test -p native` (see `native/src/hlcr/*.rs`'s own test modules).
- The real native window, real file dialogs, real filesystem access.
- Anything outside Render Studio — the harness only wires up what
  `render_pane.js` touches.

For that level of coverage, the fallback is still a real
`npm run tauri dev` session and manual clicking, or investing further in a
`tauri-driver`/WebDriver setup (a real end-to-end route, deliberately not
what this is — see the PR that introduced this suite for the tradeoff).

## Adding a fixture the harness doesn't have yet

`render-studio.html` only carries the DOM elements `render_pane.js` currently
queries. If a change to that file starts querying a new element id, add it to
the harness too — `grep "querySelector('#" src/render_pane.js` finds the
full current list.
