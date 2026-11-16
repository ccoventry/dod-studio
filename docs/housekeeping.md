# Housekeeping

A checklist for keeping the repository, its issues and PRs, its docs and the agents' notes clean and current. Anyone can run it, a person or a coding agent. It replaces one-off triage passes with one routine whose results can be compared from pass to pass (#1422).

## When

- **Light pass, at the end of every working session:** branches, worktrees and test files, plus the agents' notes (sections 4, 10 and 11).
- **Full pass, weekly:** every section below.

## Rules for a pass

- **Report first, act after.** A pass reads and proposes. Nothing is closed, merged, deleted, relabelled or retitled until the owner approves, item by item or as a whole list. The one exception is the agent's own scratch output, which it deletes itself.
- **One report.** Each section gives its findings with a proposed action each (close, update, file, ask, park, delete, leave). Oldest first, so old work ships before new work.
- **Archive rather than delete when unsure.** Docs go to `docs/archive/`. A PR is closed with its branch kept.
- **Never remove a caution or an open decision** without the owner's OK, whether it sits in a doc, a note or an issue.
- **Measured, not guessed.** Counts come from a command or a script, and the report says which one.

## 1. Pull requests

`python -I tools/agent-kit/pr_board.py` lists open PRs oldest first, with CI, behind/conflict, draft, stacked base, test DLL and "waits on the owner" lines. `python -I tools/agent-kit/pr_files.py --stale-drafts` covers old drafts.

- **Ready:** green, reviewed and tested. Propose a merge order for `merge_train.py`.
- **Behind or conflicting:** update, or resolve the conflict, then re-run CI.
- **Red CI:** fix it, or say why it is red.
- **Superseded:** the work shipped elsewhere, or its issue is closed. Propose closing it and keeping the branch.
- **Stale drafts and R&D:** park (a comment saying why and what would revive it) or close with the branch kept.
- **Description drift:** the body still matches the branch (test steps, test DLL, `Release note:` line), and the PR is linked to the issues it closes (CLAUDE.md, GraphQL link).

## 2. Issues

`python -I tools/agent-kit/issue_status.py --lint <numbers>` and `--combined` for an audit.

- **Done but open:** a merged PR finished it. Propose closing it with the PR named.
- **`on-dev` label:** decide each one: close it, or say what is left.
- **Duplicates and overlaps:** propose which one to keep, linking the other.
- **Out-of-date text:** the issue describes code or names that have changed since. Propose an edit.
- **Naming and labels:** titles follow CLAUDE.md (no number, `[R&D]` / `[Chore]` only), and every issue has an `area:` label.
- **Finished R&D:** an `[R&D]` issue whose finding is written up. Propose closing it, or a follow-up build issue.

## 3. Decisions and specs

- **Waiting on a decision:** one list across PRs, issues and the agents' notes, oldest first. Each item gets the question in one line, the options, and a recommendation.
- **Not ready to start:** an issue too vague to work on gets a spec proposal: goal, scope, what is out of scope, how it will be tested, open questions.
- **Missing issues:** work deliberately deferred and written down only in a PR comment, a review or an agent's notes. Propose filing it (CLAUDE.md: file only deferred work, not finished fixes).

## 4. Branches, worktrees and test files

- **Remote branches** whose PR merged or closed (head branches auto-delete on merge; the rest linger). Propose deleting them, with a list of the PRs.
- **Local branches** with no remote and no open PR, and local copies left behind their remote.
- **Worktrees:** `git worktree list`. Remove each whose PR merged, together with its own `CARGO_TARGET_DIR`.
- **Test files:** test DLL folders under `local\test-<n>\` whose PR merged; test output folders; scratch files left beside the repo. `local\` and test folders are machine-specific, so this part runs on the machine that holds them.
- **Free space** on the build drive. A full drive fails tests that check room before they run.

## 5. Docs

- Every file in `docs/` has a line in `docs/README.md`.
- A plan whose work is done moves to `docs/archive/`.
- `features.md` matches what has shipped. A merged feature missing from it is added, and a removed one is taken out.
- After a rename, grep docs, READMEs, `goldsrc-hooks/ui/Commands.txt` and comments for the old name (CLAUDE.md, renames).
- Counts and snapshot numbers in docs ("about 175 sites", "56 files") are re-measured, or reworded so they don't go stale.

## 6. CLAUDE.md and agent instructions

- **Still true:** each rule still matches the code and the workflow. A rule the code has outgrown is fixed or removed.
- **Repeated rules** are kept in one place. CLAUDE.md points to `coding_standards.md` and other docs rather than copying them.
- **Tool-checkable rules** move to a test or a lint when possible (coding standards, "Checked by tools").
- **Length:** instruction files help with conventions an agent can't guess and cost context for everything else (coding standards, last paragraph). A section that grows long is a prompt to move its detail into a doc.

## 7. Coding standards: trend

`coding_standards.md` is mostly checked in review. Each PR's review checks the diff against it and lists breaches separately from bugs. This section tracks whether the code as a whole is moving the right way. Re-measure each full pass and add a row to the table below.

| Measure | How |
|---|---|
| Functions over Clippy's `cognitive_complexity` threshold | `rustup run 1.98.1 cargo clippy --workspace --all-targets -- -A clippy::all -W clippy::cognitive_complexity` (count outside `examples/` too) |
| Rust files over the size prompt, tests excluded | `source-checks/tests/file_ceilings.rs` numbers against the files' current counts |
| `String` errors in `native` | count `Result<…, String>` in `native/src` (#1382) |
| `let _ =` with no comment that isn't a send, emit or cleanup of our own file | a script over the tree (coding standards, section 5) |
| ESLint `max-params` warnings | `npm run lint` in `studio/` |
| Files whose inline tests should move to a sibling file | files over ~1,000 lines in total because of tests (#1380) |

When a list gets short, propose turning it into a CI gate (coding standards, "Candidates, not yet turned on"). Lint rules change only with the owner's OK.

| Pass | Complexity | Files over prompt | `String` errors | Uncommented `let _ =` | `max-params` | Tests to move |
|---|---|---|---|---|---|---|
| (first pass fills this row) | | | | | | |

## 8. CI health

- **Wall-clock:** the longest job against the target in CLAUDE.md ("Changing CI"). Compare the last few runs on `dev`.
- **Flaky tests:** a check that failed and then passed on a re-run with no change. Propose a fix or an issue.
- **Skipped areas:** the `changes` job still lists every path a job reads (`.github/scripts/ci_changes.py`).

## 9. Dependencies

Report only. Upgrades are their own PRs.

- GitHub's security alerts for the repo (Dependabot), and `cargo`/`npm` advisories.
- Crates and packages far behind their latest release, with what upgrading would take.
- Third-party tools users install themselves (FFmpeg, HLAE, OBS, Blender): any feature known to depend on one version's behaviour, and whether the activity log records the version (#1492).

## 10. Published pages

Pages published outside the repo for the team (test queues, specs, dashboards): republish each one that has gone stale, or retire it.

## 11. The agents' notes

Coding agents keep their own notes between sessions (Claude Code's memory). They live outside the repository, so this section is run by the agent on the machine that holds them, and it reports what it changed in one list.

- **Where things live:**
  - repo-wide rules go in CLAUDE.md or a doc;
  - one person's or one machine's rules go in their `CLAUDE.local.md`;
  - everything else stays in the notes.
  - A note that holds a repo-wide rule or an engine fact moves into CLAUDE.md or the matching `docs/` file, leaving no copy behind.
- **What stays, and for how long:**
  - **Session status** (what is running, what waits on whom): finished sections are removed. Anything older than a week that is still open is kept as one line, and the detail moves to an archive file.
  - **Lessons and preferences:** kept. They are merged when two notes say the same thing, and fixed or deleted when proved wrong.
  - **Pointers** (paths, PR numbers, test folders): removed once the thing is merged, closed or deleted.
- **Check before trusting:** a note that names a file, a flag, a PR or a path is checked against the repo before it is acted on or repeated.
- **Size prompts:** the notes' index over about 100 lines, or the session-status note over about 150, is a prompt to prune. It is not a hard limit.
- **Unfiled work:** deferred items found in the notes go to section 3's "missing issues" list.
- **Backup:** if the notes are mirrored somewhere private, run the mirror after the pass.
