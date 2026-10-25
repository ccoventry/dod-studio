# DoD Studio

Turn your **Day of Defeat 1.3** demos into highlight clips. DoD Studio finds
every kill streak in a folder of demos, lets you review them in the game, records
the ones you keep with HLAE, and renders them into finished videos. It also reads
demos for match stats and fixes how demos look when they play back.

A creative fork of [cgdangelo/dod-tools](https://github.com/cgdangelo/dod-tools).

## Download

**[Get the latest release](https://github.com/ccoventry/dod-studio/releases/latest)**:
download `setup.exe` and run it. Windows 10 or 11, 64-bit.

- DoD Studio tells you when an update is out (the button beside Help) and
  installs it for you.
- Want new features before they're released? Pick the **Experimental** channel
  under Help › Check for Updates. It can have bugs the stable one doesn't.
- Just want stats from one demo? The **[web analyzer](https://ccoventry.github.io/dod-studio/)**
  reads a demo in your browser, with nothing to install.

## What you need

- **Day of Defeat on Steam.** Pre-Anniversary and 25th Anniversary are both
  supported.
- **A separate copy of Half-Life just for movies** is recommended. It keeps your
  movie configs, models and sounds away from the game you play online.
- **[HLAE](https://www.advancedfx.org/)**, which records the game. Required.
- **[FFmpeg](https://ffmpeg.org/)** for video capture and for rendering clips.
  Optional, but you'll want it.

On first launch, a **Setup** window walks you through each one. The Capture
page keeps a checklist until the required items are set.

## What it does

- **Find highlights:** scan a folder (or a whole drive) of POV and HLTV demos.
  Every kill streak becomes a row you can tick, note and trim.
- **Review them in the game:** play every highlight and answer Yes, Maybe or
  No before you capture. Long walks between kills fast-forward.
- **Capture in one batch:** DoD Studio drives the game and HLAE, jumps between
  clips, and records each one, while the game stays behind your other windows.
- **Render:** turn the recorded takes into finished clips with FFmpeg, or have
  each batch finish them on its own, named from your own template.
- **Make clips look right:** an in-game DoD Studio window, spectating that looks
  like POV (gun animations, gunshots, crosshair, scope), a bigger kill feed
  with sharper icons, and switches that hide the scoreboard, spectator bars,
  voice menus, the red head-hit screen and more. Command profiles such as
  **Clean clips** set them in one go.
- **HD textures:** AI-upscaled map textures, models, sprites, HUD icons and
  skies, built from the HD Textures page.
- **Players:** give each player their own spray and crosshair in your clips.
- **Demo tools:** a match analyzer (scoreboard, kills on the map, flags, chat,
  rounds), and an auditor that renames demos, splits demos that span several
  maps, and finds duplicates across every drive.

The full list for each release is on the
[Releases page](https://github.com/ccoventry/dod-studio/releases), and every
in-game `ds_*` console command is in
[`docs/dodstudio_commands.md`](docs/dodstudio_commands.md).

## Staying VAC-safe

DoD Studio loads HLAE and its own hook DLL into the game and patches it in
memory, which is exactly what VAC looks for. So:

- **Only play demos in a game DoD Studio started.** Never join a game server
  from it. If HLAE asks "You are about to connect to a server", answer No,
  unless it's an HLTV proxy you chose to watch. DoD Studio's hook DLL refuses
  `connect` itself while it is loaded, except to an HLTV proxy that says VAC
  is off.
- **Never load the hook DLL by hand** (with HLAE or any injector) into a game
  you then play online with.
- **To play online, start Day of Defeat from Steam, without DoD Studio.** A
  separate copy of Half-Life for movies is recommended but never required: it
  keeps your movie configs, models, sprites and sounds out of the game you
  play online. It protects those files, not your account.

Every launch route and what protects it: [`docs/vac_safety.md`](docs/vac_safety.md).

## Use at your own risk

Everything DoD Studio does happens offline. It plays demos in a game it
started itself, never joins a server, and starts that game with HLAE's connect
warning and the hook DLL's own connect refusal in place. As Valve describes
VAC, it acts when a game connects to a VAC-secured server, so a game that never
joins one has nothing to be banned for. Valve publishes that rule, not what VAC
checks or when it bans, so nobody can promise that a given tool is safe, and
what happens once you connect from a game with the DLL loaded, or load it into
the game you play online with, is outside this project's control.

The same goes for your files. The app reads demos and never moves or deletes
one unless you asked it to and it said so first. That is best effort, not a
guarantee: software has bugs. Back up demos you care about before pointing any
tool at them, this one included.

You use DoD Studio at your own risk. A lost demo or a banned account is yours
to carry, not the project's. The MIT licence says the same in its own words:
the software is provided as is, without warranty of any kind.

Valve's own pages are the authority on VAC, not this README. The addresses are
written out so you can see where each one goes before you click, or type it
into your browser yourself:

- Valve Anti-Cheat (VAC) System, on Steam Support: what VAC is, when a ban
  applies, and that bans are permanent.
  <https://help.steampowered.com/en/faqs/view/571A-97DA-70E9-FF74>
- Steam Subscriber Agreement: the rules of the account itself.
  <https://store.steampowered.com/subscriber_agreement/>

Read them before you decide what to run.

---

## For developers

The rest of this page is about the code.

### Component status

The parts of this repository are at different levels of maturity. Read this
before judging the codebase by whichever part you happen to open.

| Area | Status | Notes |
| --- | --- | --- |
| `dod/` — DoD 1.3 message parsers | **Stable** | Hand-written parsers for the mod's user messages. No public library covers this layer. |
| `analysis/` — match analytics | **Stable** | Players, scoreboard, kills, rounds, chat, survival times, highlights. Validated across 624 demos; derived kill counts reconcile exactly with the game server's own frag counter for ~75% of players and within one kill for ~90%. |
| `dem-patch/` — demo reader/writer | **Stable** | Vendored fork of the [`dem`](https://github.com/khanghugo/dem) crate. See *Why the fork* below. |
| `hl-demo-auditor/` — duplicate finder | **Stable** | Small, self-contained. The app's Demo Auditor builds on it. |
| `native/` — capture engine & patcher | **Active development** | Capture planning, batch scheduling, take management and FFmpeg. Changes often. |
| `goldsrc-hooks/` — in-game DLL | **Active development** | The `ds_*` console surface, HLTV fixes, HUD control and HD textures, on both engine builds. See its [README](goldsrc-hooks/README.md). |
| `studio/` — Tauri desktop app | **Released, changing fast** | Ships as DoD Studio. Expect new features and UI changes between releases. |
| `web-analyzer/` — browser demo viewer | **Active development** | `analysis/` compiled to wasm, deployed to GitHub Pages on every push to `main`. |

If you are here to look at demo parsing or stats extraction, `dod/` and
`analysis/` are the parts to read, and they are the parts you can rely on.

### Workspace layout

    dod/              nom-based parsers for DoD 1.3 network messages (no I/O)
    analysis/         match analytics built on top of dod/
    dem-patch/        vendored + patched `dem` crate (GoldSrc demo read/write)
    native/           capture engine, demo patcher, take management, FFmpeg
    hl-demo-auditor/  duplicate-demo detector and lost-frames check (--integrity)
    benchmark/        parsing/patching performance harness
    goldsrc-hooks/    32-bit DLL injected into hl.exe: HLTV fixes, HUD control, HD textures
    studio/           Tauri v2 + Vite frontend
    web-analyzer/     analysis/ compiled to wasm, browser-based demo viewer

### Quick start

Analyse a demo and print match analytics:

    cargo run -p analysis --bin parse_demo -- path/to/demo.dem

Headless preview CLI (accepts a demo or a folder):

    cargo run -p native --bin preview_cli -- path/to/demo-or-folder

Check demos for silently lost frames, one line each, non-zero exit if any is
broken (#41; see `docs/goldsrc_demo_salvage.md`):

    cargo run --release -p hl-demo-auditor -- --integrity path/to/demo-or-folder

Desktop app:

    cd studio
    npm install
    npm run tauri dev

Tests:

    cargo test --workspace

Contributing: [`CLAUDE.md`](CLAUDE.md) holds the repository's rules (branch
and PR names, CI, engine quirks). Every pull request targets `dev`.

### Localization keys

Tokens are stored **bare** — lowercase, no `#`. The `#` is a lookup-time sigil
meaning "resolve this as a token", not part of the name, which is Valve's own
convention: every shipped game file stores keys without it. `translate_key`
normalizes on both insert and lookup, so a file written either way resolves.

### Why the fork of `dem`

`dem-patch/` is `dem` v0.2.3 with the parse path hardened. Upstream resolves
delta-decoder tables with `.unwrap()` in seven files; when a demo does not carry
the expected delta-description table, that is a **panic rather than a parse
error**, which kills the whole process mid-batch. The fork falls back to the
library's built-in initial delta table where one exists and returns a graceful
parse failure otherwise. As of upstream v0.3.0 all 29 of those call sites are
still present, so the patch is still required.

### Documentation

Engineering notes live in `docs/` ([index](docs/README.md)): architecture
decisions, GoldSrc and DoD engine quirks, and HLAE protocol constraints.
`docs/features.md` describes what the app does, page by page.
`docs/dodstudio_commands.md` lists the in-game `ds_*` console commands. Bugs
and planned work are tracked in GitHub Issues; retired planning docs are in
`docs/archive/`.

## Licensing

This project is MIT (see `LICENSE`), which carries two copyright lines.
Charles D'Angelo's is from [cgdangelo/dod-tools](https://github.com/cgdangelo/dod-tools),
which this is a fork of — MIT requires that notice be retained, so it stays.
The second covers the work done here since the fork.

One directory is **not** MIT: **`dem-patch/` is LGPL-3.0**. It is a vendored
fork of upstream [`dem`](https://github.com/khanghugo/dem) and keeps upstream's
terms; `dem-patch/LICENSE` is the authority for that directory.

Because `dem-patch/` is linked into every binary this workspace builds, anyone
redistributing those binaries is redistributing LGPL-3.0 code and takes on that
licence's obligations for the library portion -- principally the requirement
that recipients be able to relink against a modified version of it. The MIT
licence at the repository root does not override that, and is not intended to
suggest otherwise.

Local modifications to `dem-patch/` are described under *Why the fork of `dem`*
above and are themselves LGPL-3.0, being changes to an LGPL work.
