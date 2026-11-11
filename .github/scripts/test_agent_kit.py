"""Runs tools/agent-kit's unit tests in CI's Python step, which discovers only this folder.

The agent kit is dev-only (tools Claude agents and the user run to test the app), but
its pure parts have tests, and they belong in CI like any other: a broken parser in a
test tool shows up as a wrong verdict during a test, not as an error.
"""
import importlib.util
import sys
from pathlib import Path

KIT = Path(__file__).resolve().parents[2] / "tools" / "agent-kit"


def load_tests(loader, tests, pattern):
    for folder in (KIT, KIT / "studio_live"):
        if str(folder) not in sys.path:
            sys.path.insert(0, str(folder))
    for path in sorted(KIT.rglob("test_*.py")):
        spec = importlib.util.spec_from_file_location(f"agent_kit_{path.stem}", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        tests.addTests(loader.loadTestsFromModule(module))
    return tests
