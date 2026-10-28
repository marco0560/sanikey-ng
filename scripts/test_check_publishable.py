"""Tests for developer-only publication privacy checks."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.check_publishable import private_path_errors, publishability_errors


class PrivacyPublicationTests(unittest.TestCase):
    """Verify publication guards reject private content without Git."""

    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tempdir.cleanup)
        self.root = Path(self.tempdir.name)

    def test_host_local_unix_path_is_rejected(self) -> None:
        path = self.root / "procedure.md"
        path.write_text("source = /" + "home/operator/patient-a\n", encoding="utf-8")

        errors = private_path_errors(self.root, [Path("procedure.md")])

        self.assertEqual(len(errors), 1)
        self.assertIn("procedure.md:1", errors[0])

    def test_relative_private_data_example_is_allowed(self) -> None:
        path = self.root / "procedure.md"
        path.write_text(
            'source = "local-data/patient-a/documents"\n',
            encoding="utf-8",
        )

        self.assertEqual(private_path_errors(self.root, [Path("procedure.md")]), [])

    def test_private_directory_candidates_are_rejected(self) -> None:
        errors = publishability_errors(
            [Path("patients/patient-a/file.pdf"), Path("generated/patient-a/index.html")]
        )

        self.assertEqual(len(errors), 2)


if __name__ == "__main__":
    unittest.main()
