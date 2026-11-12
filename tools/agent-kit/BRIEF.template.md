# Unattended pass {{DATE}}: rules every agent follows

Copy this to the main checkout's `local\unattended\BRIEF.md` (gitignored), fill in every `{{...}}`,
and point each agent's prompt at it. Placeholders:
`{{DATE}}`, `{{RESULTS}}` (results folder), `{{SCRATCH}}` (the session scratchpad),
`{{TEST_OUT}}` (one folder per pass on a data drive, e.g. `D:\dodstudio-tests\unattended-<date>`),
`{{OFF_LIMITS}}` (PRs, issues, folders and set-ups that are the user's this pass).

THIS PASS: each agent pushes its OWN branch and opens its OWN PR against dev (or a stacked base, if its
prompt says so), unmerged. No merges, no `gh pr update-branch`, no edits to other PRs, no issue edits
(unless your prompt says otherwise); a results comment on your own PR or your own item's issue is fine.
Never touch: {{OFF_LIMITS}}.
Several agents run at once: the game is shared through `tools/agent-kit/game_lock.py` (one game at a time;
expect to wait), and e2e port 5183 is shared too: give Playwright your own port from your prompt.
Before starting an issue: read its comments, `git branch -r | grep <issue number>` and
`gh pr list -S "#NN" --state all`. If earlier work exists, build on it and say so; don't redo it.

Repo: the main checkout (on branch dev; leave it).
The USER'S STUDIO MAY BE RUNNING FROM IT with `npm run tauri dev` (Vite port 5173): never write tracked files
there, never pull or switch it, never stop desktop-studio/node by name. Gitignored `local\` is fine to write.
Read the repo's CLAUDE.md, CLAUDE.local.md and docs/coding_standards.md: they apply. The user is away and
can't test or answer. Decide the simple, safe option, write it in your results under "Decisions", and move on.
Anything that truly needs the user goes in your results file. Decisions already made for your item are in
your prompt: don't re-open them.
Assume the user might sit down at the PC at any time: the lock check and account check still apply, a game
of theirs means wait, and never stop a process you didn't start.
Before reporting a game crash, check memory known-game-crashes.md (e.g. any dod_lennon2 HLTV demo crashes
PRE, #207): pick a demo on another map instead.
EVERY path absolute under your worktree: the shell's cwd resets to the main checkout. Any process a test
spawns gets CREATE_NO_WINDOW and stdout to a file, so no console windows pop up. COPY node_modules.

RESULTS = {{RESULTS}} (write results files here)
SCRATCH = {{SCRATCH}} (your helper scripts in SCRATCH\<your topic>\, never at its root)
Agent kit: `tools/agent-kit/` in the repo (README there). Use it before writing a helper; if you had to
improvise one anyway, add a "Tooling ideas" section to your results saying what and why.

## Where to work
- Your own git worktree OUTSIDE the repo: `git worktree add <repos>\wt-<topic> -b <type>/<slug>-<issue> origin/dev`
  (existing PR branch: `-b <branch> origin/<branch>`). Never switch branches in the main checkout.
- Own CARGO_TARGET_DIR per worktree, outside the repo: `<repos>\wt-<topic>-target`. Set it for every cargo command.
- Studio builds need a hook DLL at `<worktree>\target\i686-pc-windows-msvc\release\dodstudio_goldsrc_hooks.dll`
  (your PR's own build, or a fresh `goldsrc-hooks` build from origin/dev). The main checkout's copy is STALE:
  fine only to satisfy the build script, never for a game test.
- node_modules: copy the main checkout's `studio\node_modules` into your worktree; never share `.vite` between
  two running Vite servers (delete `.vite` in your copy).
- JS: `npm run test:unit`; e2e `CI=1 npx playwright test`, always include `tests/e2e/app-boot.spec.js` for
  anything touching Studio.
- Commit WIP to your branch and push as you go (an editor restart kills agents).
- When done: stop every server you started (by PID), push, `git worktree remove --force`, delete the target dir.
  Clean-up commands use ABSOLUTE paths only.

## Tools
- The Bash tool eats backslashes in heredocs, `python -c`, sed, perl. Anything with `\` goes through
  Write/Edit, or a script file run with `python -I <file>`. PowerShell 5.1: no `&&`. Run gh from Bash
  (`--jq` quoting breaks in PowerShell). Commit from Bash with `git commit -F - <<'EOF'`. Prefix
  `MSYS_NO_PATHCONV=1` in Bash.
- gh bodies/comments: write to a file, `--body-file` / `-F`.
- Rust: `rustup run 1.98.1 cargo fmt --all`; `rustup run 1.98.1 cargo clippy --workspace --all-targets --locked -- -D warnings`;
  `cargo test --workspace --locked --exclude goldsrc-hooks --exclude web-analyzer` with `CARGO_PROFILE_DEV_DEBUG=0`
  and `CARGO_PROFILE_TEST_DEBUG=0`. goldsrc-hooks: clippy on BOTH `--target i686-pc-windows-msvc` and the host.
- Hook DLL build: `cargo build -p goldsrc-hooks --release --target i686-pc-windows-msvc --lib`. Check cargo's
  exit code before copying the DLL anywhere. Test DLLs go in the MAIN checkout's `local\test-<issue>\`.
- A hook PR that adds/renames a console name: CLAUDE.md's list (Commands.txt, console_commands_data.js,
  cfg_scan tier, dodstudio_commands.md, NEW_SINCE_555).
- Never wait with a foreground `sleep`. `gh pr checks N --watch --interval 30` in the background once.
- Stop only processes you started, by PID. Never `taskkill /IM`, never kill node/desktop-studio/hl.exe by name.
- Open PRs at a glance: `python -I tools/agent-kit/pr_board.py`.

## PRs
- `gh pr create -B dev`; body starts with `Release note:`; branch/title rules in CLAUDE.md; GraphQL issue link
  when the PR closes an issue (only when it finishes it; else "Part of #NN").
- "Test done" section: what you ran and saw (install, demo, time, result). Honest: if something was not
  tested, say so. "Decisions" section.
- Do NOT merge any PR. Self-review your diff against docs/coding_standards.md before opening it.

## Live Studio tests (no mouse/keyboard; never touch the user's own Studio)
- `tools/agent-kit/studio_live/`: `studio_live.py patch|build|seed|start|stop` (own ports from your prompt,
  own settings folder under SCRATCH\<topic>\, user's settings hash checked at stop), `cdp.cjs` to drive the
  page, `dlg.ps1` for native file dialogs. Test projects are named `.dodstudio`.
- Test output folders: `{{TEST_OUT}}\<topic>\`. Check free space first.
- Real test demos: the PRE install's `dod\` folder (memory test-demos-and-times.md); POST plays them by a
  `../` path. Never move or delete demos out of the game folders.

## The game (the allowed test account only, one game at a time)
- Before EVERY launch: `python -I tools/agent-kit/game_lock.py acquire <topic> --wait 5`. Exit 0 = go.
  BUSY (1) = someone else, or a game still running: do other work and retry. REFUSED (2) = wrong Steam
  account: do not launch, write it in your results.
- `release <topic>` only after your game has exited (it refuses while hl.exe runs); never hold it while
  writing code.
- Games stay behind the user's windows: Studio's `keep_game_behind` on; probes launch with
  `-dodstudio_start_behind +ds_capture_run_in_background 1 +ds_capture_stay_behind 1` and send
  `ds_capture_batch away`; no game_probe `focus`/`key`/`click`/`type` steps.
- `goldsrc-hooks/tools/game_probe.py` (read its docstring); `--dll` ABSOLUTE path. To play demos to their
  end: `tools/agent-kit/play_through.py`. Installs: PRE and POST movie installs, NEVER the stock `Half-Life`.
  EVERY in-game test runs on BOTH PRE and POST, each reported separately; only skip one when the change
  provably runs identical code on both, and say why. Never write the game's .cfg files or anything in `dod\`.
- Screenshots and videos: main checkout's `local\review-images\<issue>\`. Every comparison video gets the
  stopwatch drawn last: `python -I tools/agent-kit/frames.py clock <in.mp4> <out_stem> --fps 120`.

## Results
Write RESULTS\<topic>.md early and keep it current: tests run and one-line output, probe results (install +
what you saw), decisions, anything for the user, tooling ideas. Your final reply: the same, short.
