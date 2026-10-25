# Versioning & Releases (studio)

How version numbers get assigned, where they live, and how the two update
channels (`stable`, `experimental`) relate to `main`/`dev`. Written up because
this isn't obvious from the workflow files alone and is easy to forget
between releases — refer back here instead of re-deriving it.

## Where a version number lives

Four files carry a `version` field, but they don't all mean the same thing:

| File | What it's for |
|---|---|
| `studio/src-tauri/Cargo.toml` | **The real one.** Rust bakes this into the compiled binary as `CARGO_PKG_VERSION`, and `tauri-action` reads it for bundle filenames and the published `latest.json` manifest the updater actually polls. |
| `studio/src-tauri/tauri.conf.json` | Display value only (About box, window title). Stamped to match Cargo.toml's version at release time so they never disagree. |
| `studio/package.json` | Same — display/tooling only, stamped to match. |
| Root `Cargo.toml` | The Cargo **workspace's** own version (currently `0.10.0`) — unrelated to the app's version above. This is the whole `dod-studio` workspace (native/analysis/dem-patch/etc.), not studio specifically. Don't confuse the two when checking "what version are we on."

**Important:** the release workflows stamp `tauri.conf.json`/`package.json`/`Cargo.toml` with the computed release version *during the CI run only* — that stamp is **not committed back** to the repo. The checked-in versions in these files stay whatever they were (`Cargo.toml` is `0.2.0`, starting the 0.2 line; `tauri.conf.json`/`package.json` still say `0.1.0`, display-only) until someone deliberately bumps them as part of a release PR (see "Bumping minor/major" below). So `git grep version` in this repo will not tell you what's actually been released — check GitHub Releases instead.

## Stable channel — automatic

`.github/workflows/release_stable.yml` fires on every push to `main`. Since `main` is ruleset-protected (PR-only, no direct pushes), every push here really is a `dev` → `main` release-cutover merge — there's no other kind of push that lands on `main`. `.github/workflows/main_only_from_dev.yml` enforces the other half: a PR into `main` from any branch but `dev` fails its required check.

**Version is auto-computed, no manual entry required:**
1. Read `Cargo.toml`'s checked-in version, take just the `major.minor` (e.g. `0.1`).
2. Look at existing git tags matching `v<major>.<minor>.*` and find the highest patch number.
3. Next patch = highest + 1 (or `0` if no tag under that major.minor exists yet).

So a routine merge to `main` just becomes the next patch automatically — `0.1.0` → `0.1.1` → `0.1.2` → ... with zero manual steps. This is why "main is 0.1.1" even though `Cargo.toml` still says `0.1.0` on disk: `0.1.1` is a tag/release that happened, not a file edit that landed.

The workflow also accepts an optional `workflow_dispatch` `version` input — a manual override for the rare case that needs one (e.g. skipping a patch number, or re-cutting a release). Leave it blank for the normal auto-computed path.

Published as a real (non-draft, non-prerelease) GitHub Release tagged `v<version>` and named `DoD Studio v<version>` (releases up to v0.1.3 were named "DoD Tools Studio"). The body is "See assets below to download." followed by the release notes, covering the PRs merged since the previous stable tag (see "Release notes" below). The `stable` update channel polls this via GitHub's `/releases/latest` alias, which only ever resolves to a release like this one.

## Experimental channel — manual, on-demand

`.github/workflows/release_experimental.yml` is `workflow_dispatch`-only — it never fires automatically, not even on push to `dev`. You trigger it by hand whenever you want to publish a build for experimental-channel testing.

**Version entry is currently required, and free-text:**
- The `version` input asks for "the version this build is heading toward" (e.g. `0.1.2`) — your own guess at what the *next* stable version will eventually be, not a value read from anywhere.
- The workflow appends `-<GitHub run number>` automatically (e.g. `0.1.2-47`). This isn't cosmetic — it's load-bearing for three reasons documented in the workflow itself:
  1. A semver pre-release suffix (`-47`) always sorts *below* the plain version it's attached to, so an experimental build only ever offers itself as an update to someone already on an older experimental build — never to someone on the matching stable release.
  2. The run number is monotonically increasing across dispatches, unlike a git short-SHA (which sorts alphabetically, not chronologically) — so a second experimental build always looks newer than the first.
  3. The MSI/WiX bundler Tauri uses on Windows only accepts a single, purely-numeric pre-release segment. `-dev.<sha>` and `-dev.<run number>` were both tried and rejected by real builds before landing on the current bare `-<run number>` — see the workflow's own comment and [this Tauri discussion](https://github.com/tauri-apps/tauri/discussions/7600) for the failure mode.

Published as a **prerelease** named `DoD Studio <version> (experimental)`, with release notes covering the PRs merged since the **latest stable tag** (so they list what this build has that the stable channel lacks, not what an older stable already shipped), always overwriting the same fixed tag `experimental-latest` (the previous `experimental-latest` release+tag is deleted first). The `experimental` update channel polls this via the fixed URL `.../releases/download/experimental-latest/latest.json` — a direct tag reference, not `/releases/latest`, since GitHub's `/latest` alias never resolves to a prerelease.

## No tests in the release job: CI's results instead

Neither release workflow runs the tests. Every commit on `dev` and `main` arrives through a PR whose required checks passed, so rerunning them cost 2.5-10 minutes a release for nothing (#915). Instead, a "Require passing CI on this commit" step early in the job runs `.github/scripts/require_ci_passed.py`, which reads the required status checks of the branch's ruleset (`dev` for experimental, `main` for stable) and fails the release, naming each check, unless every one has a passing run by its exact job name:

- **On the commit itself, or on a commit with the same tree.** A stable release builds main's `dev` → `main` merge commit, and CI never runs on main, so its CI is on the merge's second parent (`dev`'s tip) or the PR head. That counts because the rulesets require an up-to-date branch: the merge commit's tree equals the PR head's, and CI's pull_request run tested exactly that tree. A commit no CI run tested (an admin push, say) fails.
- **The latest run per commit counts**, so a re-run replaces a failure. `success`, `neutral` and `skipped` pass, as in the rulesets.
- **It waits** (up to 45 minutes) while a run is still going, e.g. dev's push CI right after a merge.

Locally: `python .github/scripts/require_ci_passed.py --sha <commit> --branch main`.

## Release notes

Both release workflows build their release body with `.github/scripts/release_notes.py` (a "Collect release notes" step early in the job, so a failure costs seconds, not a build) and pass it to `tauri-action` as `releaseBody`, with `generateReleaseNotes` off. GitHub's own generated notes (#165, #490) listed ~290 bare PR titles, mostly under "Other changes" because few PRs carry a label, and the experimental build compared from v0.1.3 instead of the last stable v0.1.4, so it relisted shipped PRs. `.github/release.yml` still groups GitHub's notes, but only for someone clicking "Generate release notes" on the release page.

**The `Release note:` line.** Every PR body starts with one (`.github/pull_request_template.md`; CLAUDE.md has the rule):

    Release note: Fixed: The game no longer crashes on the first frame of a preview.

One plain-language line for players. The optional prefix (`New:`, `Fixed:`, `Changed:`) picks the group; without one, a `bug` label means Fixed, `enhancement` means New, and anything else is Changed. `none` (or an empty or unfilled line) means internal work: the PR appears only in the full list. Only the first such line in a body counts, and HTML comments are ignored, so the template's guidance never becomes a note.

**The `Supersedes release note:` line.** When a PR renames or changes something an earlier PR in the same release added, its body can say so (v0.2.0's notes otherwise named the `dodstudio_*` commands that #719 had renamed to `ds_*`):

    Supersedes release note: #710, #731

Matched like the `Release note:` line (case-insensitive, `notes` too, bold or list forms); numbers with or without `#`, separated by commas or spaces; several such lines add up. Each listed PR that is in the same release range is dropped from the notes and from the full PR list, and this PR's own note stands for it. Chains collapse: if A supersedes B and B supersedes C, only A shows. A listed PR outside the range already shipped, so its note is already out: the script prints a notice and drops nothing. A superseding PR with no note of its own (or `none`) still drops the PRs it lists, with a warning naming the notes lost. Every drop is logged to the job log (`release notes: #731 dropped: superseded by #1013`).

**What the script does:**
1. **Previous tag.** The highest stable `vX.Y.Z` tag on `origin`: for experimental, the latest one (never `experimental-latest`); for stable, the highest one below the version being released (`--before`). Only tags on the remote count, since a local clone can also carry the upstream dod-tools tags (`v0.2.0` .. `v0.10.0`), which are not this project's releases. `--from` overrides it.
2. **PRs in the range.** Every `Merge pull request #N` merge commit in `<previous tag>..<target>`, not just first-parent ones (so a stable release on `main` reaches the PRs that landed on `dev`, and a PR merged into another PR's branch counts too), plus squash merges (`... (#N)`) on the target's first-parent history. The range works for experimental even though the last stable tag sits on `main`, not `dev`: the release merge's second parent is `dev`'s tip at that release, so everything shipped is excluded. Release plumbing PRs (`dev` → `main` cutovers and `main` → `dev` syncs) are dropped.
3. **Bodies.** One `gh api graphql` call per 50 PRs reads title, body and labels (`pull-requests: read` on the workflows).
4. **Output.** The workflow's first line ("On-demand experimental build from `dev` …" / "See assets below to download."), then "What's new" (the summary file, below), then `## New`, `## Fixed`, `## Changed` with `- <note> (#N)` bullets, then a collapsed `<details>` list of every merged PR title, then a "Full diff" compare link.

**A hand-written summary per version.** When `.github/release_notes/<version>.md` exists, its content is printed first under "What's new". The version is the experimental workflow's input without the `-<run number>` (an experimental `0.2.0-57` build reads `0.2.0.md`) or the stable workflow's computed version, so one file serves both the experimental builds heading to a version and its stable release. Write it for a release worth a summary (0.2.0 has one); routine patches need none.

**Locally** (needs `gh` logged in; prints to stdout without `--output`):

    python .github/scripts/release_notes.py --target origin/dev --version 0.2.0
    python -m unittest discover -s .github/scripts -p "test_*.py"

## Bumping to a new minor or major version

Not automatic on either channel — deliberately a human decision. Edit `studio/src-tauri/Cargo.toml`'s `version` field (e.g. `0.1.0` → `0.2.0`) as part of whatever PR is landing the milestone that justifies it, same as any other code change. The next push to `main` after that lands will have no existing `v0.2.*` tags yet, so `release_stable.yml`'s patch computation naturally starts that new line at `0.2.0`.

## The app's Windows identity

`tauri.conf.json` holds three values that are the installed app's identity, not just labels: `productName` (`DoD Studio`: the install folder `%LOCALAPPDATA%\DoD Studio`, the Apps entry and its uninstall registry key, the Start-menu shortcut), `identifier` (`com.dodstudio.app`: the WebView2 profile in `%LOCALAPPDATA%\com.dodstudio.app`, which holds the panes' `localStorage`, and the notification app id) and `mainBinaryName` (`dod-studio`, so the shipped exe is `dod-studio.exe` whatever the cargo package is called). A new `productName` makes the next installer install beside the old one instead of upgrading it; a new identifier starts an empty WebView2 profile; a new `mainBinaryName` orphans shortcuts that point at the old exe.

They changed once, from `DoD Tools Studio` / `com.dodtools.studio` / `desktop-studio.exe` (#258). The NSIS hook `studio/src-tauri/windows/installer_hooks.nsh` handles that move: after installing, it copies the old profile's `Local Storage` into the new one and runs the old install's uninstaller silently. It is NSIS only: an MSI install of the old name has to be uninstalled by hand. Rename again only with a hook like it. The app's own data in `%APPDATA%\dod-studio` (`native::shared::paths::APPDATA_DIR_NAME`) does not depend on any of the three.

A push to `main` also runs `.github/workflows/sync_wiki.yml`, which publishes `docs/wiki/` and the generated Commands page to the GitHub wiki (#493); that is separate from the releases.

## Open questions (not yet decided — your call)

**Should the experimental channel's version input auto-detect instead of staying manual?**
Right now it asks you to type a version guess every time. It *could* mirror stable's approach — read `dev`'s own `Cargo.toml` major.minor and auto-compute a patch guess the same way (highest existing `v<major>.<minor>.*` tag + 1), making the input optional with that as the default and a manual override still available for a deliberate minor/major signal. The one thing that computation can't know on its own is *intent* — whether the work currently on `dev` is heading toward a patch or a minor/major bump — so it would still occasionally need a manual override, just less often than every single dispatch. Worth doing if the manual entry keeps being a "wait, what number was I on" moment; not worth doing if it rarely comes up. (This would mean editing `release_experimental.yml` — flag if you want that built.)

**Should a stable release also trigger a matching experimental release, so the experimental channel doesn't fall behind?**
Depends on what "falls behind" means at the moment it happens. Right after a `dev` → `main` merge, `dev` and `main` are usually at the *same commit* (the merge that cut the stable release is the same commit dev was already on) — so re-cutting an experimental release in that exact window would just republish identical code under a different version string. The real risk isn't "the experimental channel is missing what stable has" (it never is, right after a merge) — it's "`experimental-latest` still points at an *older* prerelease build from before that merge," which could look stale to someone checking the experimental channel even though `dev` the branch has already moved on. So the more useful trigger is probably: cut a fresh experimental release whenever meaningful new work has landed on `dev` *since* the last experimental release — not automatically lockstep with every stable cutover, which would frequently be a same-commit no-op.
