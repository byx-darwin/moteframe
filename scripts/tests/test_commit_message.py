"""Commit subjects must use the documented contributor convention."""
from pathlib import Path
import importlib.util
import unittest

SCRIPT = Path(__file__).parents[1] / "commit_message.py"
SPEC = importlib.util.spec_from_file_location("commit_check", SCRIPT) if SCRIPT.exists() else None
MODULE = importlib.util.module_from_spec(SPEC) if SPEC is not None else None
if SPEC is not None:
    SPEC.loader.exec_module(MODULE)


class CommitTests(unittest.TestCase):
    def setUp(self):
        if MODULE is None:
            self.fail("commit-message validator is not present")

    def test_should_accept_scopes_and_breaking_changes(self):
        for subject in ["feat(ui): add clipping", "fix!: reject invalid bounds", "docs: explain adapters"]:
            with self.subTest(subject=subject):
                self.assertTrue(MODULE.valid_subject(subject))

    def test_should_reject_empty_untyped_or_multiline_subjects(self):
        for subject in ["", "update code", "feat: ", "feat: add\nsecret", "random: change"]:
            with self.subTest(subject=subject):
                self.assertFalse(MODULE.valid_subject(subject))


if __name__ == "__main__":
    unittest.main()
