#!/usr/bin/env python3
"""Guard the standalone package workflow against silently dropping drivers."""

import unittest
from pathlib import Path


class PackageWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = Path(".github/workflows/apps.yml").read_text()

    def test_driver_changes_trigger_the_workflow(self):
        self.assertIn("standalone_drivers/**", self.workflow)

    def test_apps_and_drivers_are_built_and_signed(self):
        self.assertIn("standalone_apps", self.workflow)
        self.assertIn("standalone_drivers", self.workflow)
        self.assertIn("*.app.elf", self.workflow)
        self.assertIn("*.drv.elf", self.workflow)

    def test_publication_is_strict_and_architecture_qualified(self):
        self.assertIn("--require-signatures", self.workflow)
        self.assertIn('artifacts/$TARGET_ARCH', self.workflow)
        self.assertIn('path: artifacts/', self.workflow)
        self.assertIn('merge-multiple: true', self.workflow)
        self.assertIn('all-artifacts/. publish/apps/', self.workflow)
        self.assertNotIn("2>/dev/null || true", self.workflow)


if __name__ == "__main__":
    unittest.main()
