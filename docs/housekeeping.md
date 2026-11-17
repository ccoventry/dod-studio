# Housekeeping

A checklist for keeping the repository, its issues and PRs, its docs and the agents' notes clean and current. Anyone can run it, a person or a coding agent. It replaces one-off triage passes with one routine whose results can be compared from pass to pass (#1422).

## When

- **Light pass, at the end of every working session:** branches, worktrees and test files, published pages, and the agents' notes (sections 4, 10 and 11).
- **Full pass, weekly:** every section below.

## Rules for a pass

- **Report first, act after.** A pass reads and proposes. Nothing is closed, merged, deleted, relabelled or retitled until the owner approves, item by item or as a whole list. The exceptions are the agent's own scratch output and its own notes (section 11): it changes those itself and reports what it changed; the caution rule below still applies to the notes.
- **One report.** Each section gives its findings with a proposed action each (close, update, file, ask, park, delete, leave). Oldest first, so old work ships before new work.
- **Archive rather than delete when unsure.** Docs go to `docs/archive/`. A PR is closed with its branch kept.
- **Never remove a caution or an open decision** without the owner's OK, whether it sits in a doc, a note or an issue.
- **Measured, not guessed.** Counts come from a command or a script, and the report says which one.

## 1. Pull requests

`python -I tools/agent-kit/pr_board.py` lists open PRs oldest first, with CI, behind/conflict, draft, stacked base, test DLL and "waits on the owner" lines. `python -I tools/agent-kit/pr_files.py --stale-drafts` covers old drafts.

- **Ready:** green, reviewed and tested. Propose a merge order, previewed with `python -I tools/agent-kit/merge_train.py --dry-run N M`.
- **Behind:** leave it. `merge_train.py` updates each PR as it merges, and updating them all at once only wastes CI (each merge puts the rest behind again).
- **Conflicting:** list the conflicting files and propose who resolves it.
- **Red CI:** name the failing check and its cause, and propose a fix.
- **Stacked on another branch:** no CI runs (CLAUDE.md). Check that local results are posted on the PR.
- **Superseded:** the work shipped elsewhere, or its issue is closed. Propose closing it with the branch kept.
- **Stale drafts and R&D:** propose parking it (a comment saying why and what would revive it) or closing it with the branch kept. Before a PR is closed, its issue carries the current state and a note to merge `dev` into the branch when the work is picked up again.
- **PR text:** the body still matches the branch (test steps, test DLL, `Release note:` line), and the PR is linked to the issues it closes (CLAUDE.md, GraphQL link). The PR targets `dev`; its title follows CLAUDE.md (a plain line ending in the issue, no `feat:` prefix); and a PR changing an unreleased PR's feature carries `Supersedes release note:`.

## 2. Issues

`python -I tools/agent-kit/issue_status.py --lint --all-open` lists label and title problems. `python -I tools/agent-kit/issue_status.py --all-open --combined <file>` writes the full audit (the PRs citing each issue, dev commits, branches) to one file.

- **Done but open:** a merged PR finished it. Propose closing it with the PR named.
- **`on-dev` label:** decide each one: close it, or say what is left.
- **Duplicates and overlaps:** propose which one to keep, linking the other.
- **Out-of-date text:** the issue describes code or names that have changed since. Propose an edit.
- **Naming and labels:** titles follow CLAUDE.md (no number, `[R&D]` / `[Chore]` only), and every issue has an `area:` label.
- **Finished R&D:** an `[R&D]` issue whose finding is written up. Propose closing it, or a follow-up build issue.
- **Private details:** `source-checks/tests/private_paths.rs` covers tracked files only. Scan the week's issue and PR bodies and comments for a real user folder, email or machine name.

## 3. Decisions and specs

- **Waiting on a decision:** one list across PRs, issues and the agents' notes, oldest first. Each item gets the question in one line, the options, and a recommendation.
- **Not ready to start:** an issue too vague to work on gets a spec proposal: goal, scope, what is out of scope, how it will be tested, open questions.
- **Missing issues:** work deliberately deferred and written down only in a PR comment, a review or an agent's notes. Propose filing it (CLAUDE.md: file only deferred work, not finished fixes).

## 4. Branches, worktrees and test files

- **Remote branches** whose PR merged (head branches auto-delete on merge; the rest linger). Propose deleting them, with a list of the PRs. Keep:
  - a branch whose PR was closed unmerged (rules above);
  - any branch an open PR is stacked on (deleting it closes those PRs, CLAUDE.md);
  - `experimental/xash-transcode` (a historical record, CLAUDE.md).
- **Local branches** with no remote and no open PR, and local copies left behind their remote.
- **Worktrees:** `git worktree list`. Remove each whose PR merged, together with its own `CARGO_TARGET_DIR`.
- **Test files:** test DLL folders under `local\test-<n>\` whose PR merged, test output folders, and scratch files left beside the repo. `local\` and test folders are machine-specific, so this part runs on the machine that holds them.
- **Leftover agent state:** the merge train's queue (`python -I tools/agent-kit/merge_train.py status`), a game lock left held (`tools/agent-kit/game_lock.py`), the agent-kit cache folder, and `git stash list`.
- **Free space:** report free GB on the build drive and the size of each `target` folder. A full drive fails tests that check for room before they run.

## 5. Docs

- Every file in `docs/` has a line in `docs/README.md`.
- A plan whose work is done moves to `docs/archive/`.
- `features.md` matches what has shipped. A merged feature missing from it is added, and a removed one is taken out.
- After a rename, grep docs, READMEs, `goldsrc-hooks/ui/Commands.txt`, `goldsrc-hooks/tools/` and comments for the old name (CLAUDE.md, renames).
- Counts and snapshot numbers in docs ("about 175 sites", "56 files") are re-measured, or reworded so they don't go stale.
- Comments and ceiling reasons that cite an issue now closed or renamed (`CEILINGS` reasons, "until #N" comments) are updated.

## 6. CLAUDE.md and agent instructions

- **Still true:** each rule still matches the code and the workflow. A rule the code has outgrown is fixed or removed.
- **Repeated rules** are kept in one place. CLAUDE.md points to `coding_standards.md` and other docs rather than copying them.
- **Tool-checkable rules** move to a test or a lint when possible (coding standards, "Checked by tools").
- **Length:** instruction files help with conventions an agent can't guess and cost context for everything else (coding standards, the paragraph before Sources). A section that grows long is a prompt to move its detail into a doc.

## 7. Coding standards: trend

`coding_standards.md` is mostly checked in review: each PR's review checks the diff against it and lists breaches separately from bugs. This section tracks whether the code as a whole is moving the right way. Re-measure each full pass. The workspace build needs the hook DLL first in a fresh worktree (CLAUDE.md, build traps).

| Measure | How |
|---|---|
| Functions over Clippy's `cognitive_complexity` threshold | `rustup run 1.98.1 cargo clippy --workspace --all-targets -- -A clippy::all -W clippy::cognitive_complexity`. Report two numbers: all, and outside `examples/`. |
| Files over the size prompt | Rust files over about 1,000 non-test lines (lines before the file's first top-level `#[cfg(test)]`, skipping `tests.rs` and `*_tests.rs`, the rule `source-checks/tests/source_tree`'s `non_test_lines` uses) and JS files in `studio/src` over 600. Also list any ceiling raised since the last pass in `file_ceilings.rs`, `file_ceilings.test.js` or `main.js`'s `BUDGET`. |
| `String` errors in `native` | grep `, String>` in `native/src/**/*.rs`, non-test lines (#1382). |
| `let _ =` with no comment that isn't a send, emit or cleanup of our own file | No script yet: grep `let _ =` in `*.rs` outside tests, drop sends, emits and removals of our own files, and count the rest with no comment on or above the line (coding standards, section 5). |
| ESLint `max-params` warnings | `npm run lint` in `studio/`. |
| Files whose inline tests should move to a sibling file | Files over 1,000 lines in total whose non-test lines are under 1,000 (#1380). |

When a list gets short, propose turning it into a CI gate (coding standards, "Candidates, not yet turned on"). Lint rules change only with the owner's OK. The new row goes in a docs PR (`Release note: none`), with the date in the Pass column, since it dates a measurement.

| Pass | Complexity (all / outside examples) | Files over prompt | `String` errors | Uncommented `let _ =` | `max-params` | Tests to move |
|---|---|---|---|---|---|---|
| (the first pass fills this row) | | | | | | |

## 8. CI and tests

- **Wall-clock:** compare the longest job against the target in CLAUDE.md ("Changing CI"): `gh run list --branch dev --workflow ci.yml --limit 5`, then `gh run view <id> --json jobs`.
- **Flaky tests:** a check that failed and then passed on a re-run with no change. Propose a fix or an issue.
- **Skipped areas:** the `changes` job still lists every path a job reads (`.github/scripts/ci_changes.py`).
- **Rulesets:** the required-check names in both rulesets (`dev-protection`, `main-protection`) still match the jobs' `name:`s.
- **Caches:** `gh cache list`. Caches saved from PRs evict dev's (CLAUDE.md, CI section).
- **Ignored tests:** the `#[ignore]`d tests that need real demos never run in CI. Run them with `DOD_ANALYSIS_DEMOS` / `DOD_ROUNDTRIP_DEMO` set (CLAUDE.md, tests that need a real demo) and report failures.

## 9. Dependencies and releases

Report only. Upgrades are their own PRs.

- **Security:** `gh api repos/{owner}/{repo}/dependabot/alerts`, and `npm audit` in `studio/`. Run `cargo audit` if it is installed.
- **Outdated:** `npm outdated` in `studio/`, and `cargo outdated` if it is installed. List crates and packages far behind their latest release, with what upgrading would take.
- **Third-party tools** users install themselves (FFmpeg, HLAE, OBS, Blender): any feature known to depend on one version's behaviour, and whether the activity log records the version (#1492).
- **Release drift:** what is on `dev` but not on `main`, merged PRs missing a `Release note:` line, and whether `main` has been merged back into `dev` (docs/versioning_and_releases.md).

## 10. Published pages

Pages published outside the repo for the team (test queues, specs, dashboards): republish each one that has gone stale, or retire it.

## 11. The agents' notes

Coding agents keep their own notes between sessions (Claude Code's memory). They live outside the repository, so this section is run by the agent on the machine that holds them. It reports what it changed in one list.

- **Where things live:**
  - repo-wide rules go in CLAUDE.md or a doc;
  - one person's or one machine's rules go in their `CLAUDE.local.md`;
  - everything else stays in the notes.
  - A note that holds a repo-wide rule or an engine fact moves into CLAUDE.md or the matching `docs/` file, leaving no copy behind.
- **No duplicates:** for each note, grep CLAUDE.md, `CLAUDE.local.md` and `docs/` for its key term (a command, cvar, file name or issue number). If the rule is already there, delete the note, or cut it to a one-line pointer.
- **What stays, and for how long:**
  - **Session status** (what is running, what waits on whom): finished sections are removed. Anything older than a week that is still open is kept as one line, and its detail moves to an archive file.
  - **Lessons and preferences:** kept. Two notes that say the same thing are merged; a note proved wrong is fixed or deleted.
  - **Pointers** (paths, PR numbers, test folders): removed once the thing is merged, closed or deleted.
- **Check before trusting:** a note that names a file, a flag, a PR or a path is checked against the repo before it is acted on or repeated. At the end of a session, re-check each note the session relied on.
- **Size prompts:** the notes' index over about 100 lines, or the session-status note over about 150, is a prompt to prune. It is not a hard limit.
- **Unfiled work:** deferred items found in the notes go to section 3's "missing issues" list.
- **Backup:** if the notes are mirrored somewhere private, run the mirror after the pass.
