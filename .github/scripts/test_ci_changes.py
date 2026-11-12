"""Tests for ci_changes.py's decisions. No git, no network.

    python -m unittest discover -s .github/scripts -p "test_*.py"
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ci_changes import OUTPUTS, decide  # noqa: E402


def on(paths, event="pull_request", base="dev"):
    result = decide(event, base, paths)
    return {name for name in OUTPUTS if result[name]}


ALL = set(OUTPUTS) - {"docs_only"}


class EverythingTests(unittest.TestCase):
    def test_push_and_prs_into_main_run_everything(self):
        self.assertEqual(on(["docs/a.md"], event="push", base=""), ALL)
        self.assertEqual(on(["docs/a.md"], base="main"), ALL)

    def test_build_inputs_run_everything(self):
        for path in ("Cargo.toml", "native/Cargo.toml", "Cargo.lock", "studio/package.json",
                     "studio/package-lock.json", ".github/workflows/ci.yml", "rust-toolchain.toml",
                     ".cargo/config.toml", "test-fixtures/ci_fixture.dem"):
            self.assertEqual(on(["docs/a.md", path]), ALL, path)

    def test_an_empty_diff_runs_everything(self):
        self.assertEqual(on([]), ALL)


class AreaTests(unittest.TestCase):
    def test_docs_only_runs_nothing(self):
        self.assertEqual(on(["docs/README.md", "CLAUDE.md", "tools/agent-kit/README.md"]), {"docs_only"})

    def test_a_readme_inside_a_crate_still_counts_for_the_crate(self):
        self.assertEqual(on(["goldsrc-hooks/README.md"]), {"rust", "hooks"})

    def test_frontend_js_runs_only_e2e(self):
        self.assertEqual(on(["studio/src/main.js", "studio/tests/e2e/a.spec.js", "studio/index.html"]),
                         {"frontend"})

    def test_frontend_files_a_rust_test_reads_run_rust_too(self):
        self.assertEqual(on(["studio/src/scan_version.js"]), {"frontend", "rust"})
        self.assertEqual(on(["studio/src/agr_rate.js"]), {"frontend", "rust"})

    def test_tauri_backend_is_rust_not_frontend(self):
        self.assertEqual(on(["studio/src-tauri/src/lib.rs", "studio/src-tauri/tauri.conf.json"]), {"rust"})

    def test_native_runs_rust_and_hooks_only_for_the_files_hook_tests_read(self):
        self.assertEqual(on(["native/src/capture_engine.rs"]), {"rust"})
        self.assertEqual(on(["native/src/sys/studio_pipe.rs"]), {"rust", "hooks"})

    def test_parser_crates_run_rust_hooks_and_wasm(self):
        self.assertEqual(on(["analysis/src/kill.rs"]), {"rust", "hooks", "web_python"})
        self.assertEqual(on(["localizations/dod_english.txt"]), {"rust", "hooks", "web_python"})

    def test_bundled_scripts_count_as_rust(self):
        self.assertEqual(on(["vegas/DoD Studio Nameplates.cs"]), {"rust"})
        self.assertEqual(on(["blender/agr_render.py"]), {"rust", "web_python"})

    def test_python_tools_run_only_the_python_job(self):
        self.assertEqual(on(["tools/agent-kit/merge_train.py"]), {"web_python"})

    def test_windows_separators_are_normalised(self):
        self.assertEqual(on(["studio\\src\\main.js"]), {"frontend"})


if __name__ == "__main__":
    unittest.main()
