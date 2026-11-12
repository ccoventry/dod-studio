#!/usr/bin/env python3
"""Which of CI's expensive jobs a pull request needs, from the files it changes.

ci.yml's `changes` job runs this first; each expensive job then runs only when
its area changed. A job skipped by its `if:` reports "skipped", which the
rulesets count as passing, and the two aggregator checks (`Rust workspace
tests`, `Frontend e2e tests`) accept a skip only when this job succeeded and
said the area was untouched. Anything this can't decide runs everything.

Everything runs when:
- the event is not a pull request (the push to dev, whose run also saves the
  Rust caches), or the pull request goes into main (a release);
- a file in ALWAYS_ALL changes: the build's own inputs (Cargo manifests and
  lock, npm manifests and lock, toolchain, `.cargo/`), CI itself (`.github/`)
  and the shared test demos.

Usage (CI):

    git diff --name-only --no-renames HEAD^1 HEAD > changed.txt
    python3 .github/scripts/ci_changes.py --event pull_request --base dev \
        --files changed.txt >> "$GITHUB_OUTPUT"

Prints `name=true|false` lines, one per output in OUTPUTS.
"""
import argparse
import fnmatch
import sys

# A path matches a pattern when the pattern ends in `/` and the path starts with
# it, or when the whole path (or, for patterns without a `/`, its file name, so
# `Cargo.toml` is every crate's manifest) matches the glob.
ALWAYS_ALL = [
    "Cargo.toml", "Cargo.lock",
    "package.json", "package-lock.json",
    "rust-toolchain", "rust-toolchain.toml",
    ".cargo/", ".github/", "test-fixtures/",
]

# Every crate in the workspace, plus what their builds and tests read from
# outside their own folder: `localizations/` (analysis's build script),
# `blender/` and `vegas/` (bundled by tauri.conf.json, which tauri-build checks),
# rustfmt/clippy config, and the two frontend files a studio test reads to
# check a mirrored constant (scan_version.js, agr_rate.js). A new Rust test
# that reads a file outside the crates adds that path here.
RUST = [
    "*.rs",
    "analysis/", "benchmark/", "dem-patch/", "dod/", "goldsrc-hooks/", "hl-demo-auditor/",
    "native/", "source-checks/", "studio/src-tauri/", "web-analyzer/",
    "localizations/", "blender/", "vegas/",
    "rustfmt.toml", "clippy.toml",
    "studio/src/scan_version.js", "studio/src/agr_rate.js",
]

# Hooks 32-bit: the hook crate, the crates it depends on, and the native files
# its tests read (native/src/sys/: the pipe and review-request tables).
HOOKS = [
    "goldsrc-hooks/", "analysis/", "dod/", "dem-patch/", "localizations/", "native/src/sys/",
    "rustfmt.toml", "clippy.toml",
]

# The Playwright suite: the frontend, its harness and its config. Vitest is not
# here; it reads Rust and web-analyzer files, so it runs on every PR (in
# Frontend lint).
FRONTEND = ["studio/"]
FRONTEND_EXCLUDE = ["studio/src-tauri/"]

# Web analyzer and Python: the wasm build's crates, the web analyzer's node
# tests, and every Python file CI compiles or tests.
WEB_AND_PYTHON = [
    "*.py", "web-analyzer/", "analysis/", "dod/", "dem-patch/", "localizations/",
]

DOCS = ["docs/", "*.md"]

OUTPUTS = ["everything", "docs_only", "rust", "hooks", "frontend", "web_python"]


def matches(path, pattern):
    if pattern.endswith("/"):
        return path.startswith(pattern)
    if "/" in pattern:
        return fnmatch.fnmatchcase(path, pattern)
    return fnmatch.fnmatchcase(path.rsplit("/", 1)[-1], pattern)


def any_match(paths, patterns, exclude=()):
    return any(
        any(matches(p, pat) for pat in patterns) and not any(matches(p, ex) for ex in exclude)
        for p in paths
    )


def decide(event, base, paths):
    """{output name: bool} for these changed paths."""
    paths = [p.strip().replace("\\", "/") for p in paths if p.strip()]
    everything = event != "pull_request" or base != "dev" or not paths or any_match(paths, ALWAYS_ALL)
    if everything:
        return {name: name != "docs_only" for name in OUTPUTS}
    areas = {
        "rust": any_match(paths, RUST),
        "hooks": any_match(paths, HOOKS),
        "frontend": any_match(paths, FRONTEND, FRONTEND_EXCLUDE),
        "web_python": any_match(paths, WEB_AND_PYTHON),
    }
    # Only a report line: a README inside a crate still runs that crate's jobs.
    docs_only = not any(areas.values()) and all(any(matches(p, d) for d in DOCS) for p in paths)
    return {"everything": False, "docs_only": docs_only, **areas}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--event", required=True, help="github.event_name")
    ap.add_argument("--base", default="", help="the pull request's base branch")
    ap.add_argument("--files", required=True, help="file with one changed path per line")
    args = ap.parse_args(argv)
    with open(args.files, encoding="utf-8") as f:
        paths = f.readlines()
    result = decide(args.event, args.base, paths)
    for name in OUTPUTS:
        print(f"{name}={'true' if result[name] else 'false'}")
    print(f"{len([p for p in paths if p.strip()])} changed files: "
          + ", ".join(n for n in OUTPUTS if result[n]), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
