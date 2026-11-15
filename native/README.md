# native

The core engine crate: almost all of DoD Studio's non-UI logic. It drives HLAE
and `hl.exe` for capture batches (`capture_engine.rs`), scans and patches demo
files (`patch/`), manages takes and FFmpeg transcoding (`hlcr/`), and holds the
path, disk and hashing helpers the app and the CLIs share. The Studio app
(`studio/src-tauri`) is a thin Tauri layer over it.

`autobins = false` is set in `Cargo.toml`: a binary only exists if it is listed
there as a `[[bin]]`. Exploratory tools belong in `examples/` instead.

## Binaries

| Binary | Path | Purpose |
| --- | --- | --- |
| `preview_cli` | `src/bin/preview_cli/main.rs` | Headless preview builder: demos to `<stem>_preview.dem` with a bookmark on every highlight. Studio shells out to it. |
| `dod-studio-cli` | `src/bin/cli.rs` | Terminal demo analyzer (scoreboards, kills, chat, rounds). |
| `dod-studio-dump` | `src/bin/dump.rs` | Dumps every frame, engine message and user message of a demo. |
| `dod-studio-inspect` | `src/bin/inspect.rs` | Scans unique demos and reports raw user message types and frequencies. |
| `check_maps` | `src/bin/check_maps.rs` | Verifies (and fetches) the maps a demo folder needs. |
| `check_cfgs` | `src/bin/check_cfgs.rs` | Reports banned or hazardous commands in the game's `.cfg` files. Read-only. |
| `strip_decals` | `src/bin/strip_decals.rs` | Tests decal hygiene in isolation: strips decals outside capture windows and injects flush bursts. |

One-off R&D probes are in [`examples/`](examples/README.md).
