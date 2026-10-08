# web-analyzer

`analysis/` compiled to `wasm32-unknown-unknown`, with a static vanilla-JS
frontend (`www/`) that runs the demo parser and analytics entirely in the
browser — drop a `.dem` file in, no server-side upload. Dropping or picking
several demos at once analyses them one after another into a list: each can
be viewed, or downloaded as its own JSON file, and "Download all (JSON)" saves
one JSON array with an entry per demo (file name, size, modified time, then
the same `demo_info`/`state` the report is drawn from, or `error` for a demo
that failed to parse). Deployed to GitHub
Pages on every push to `main` that touches `web-analyzer/`, `analysis/`,
`dod/`, or `dem-patch/` (`.github/workflows/deploy_web.yml`).

## Build and serve locally

    rustup target add wasm32-unknown-unknown
    cargo install wasm-bindgen-cli --version 0.2.126 --locked
    cargo build -p web-analyzer --target wasm32-unknown-unknown --release
    wasm-bindgen --target web --out-dir web-analyzer/www/pkg \
      target/wasm32-unknown-unknown/release/web_analyzer.wasm

`www/pkg/` is generated output (gitignored) — re-run `wasm-bindgen` after any
`src/lib.rs` change. Then serve `www/` with any static file server (opening
`index.html` directly via `file://` won't work — the wasm module needs to be
fetched over HTTP):

    npx serve web-analyzer/www

The export helpers in `www/export.js` have no DOM dependency and are unit
tested under plain Node (not wired into CI):

    node --test web-analyzer/tests/export.test.mjs

The `wasm-bindgen` crate dependency and CLI version **must match exactly** —
a mismatch fails at the bindgen step. Bump both together (the pin and its
comment are in the root `Cargo.toml`, and the CLI version is also set in
`.github/workflows/deploy_web.yml`).
