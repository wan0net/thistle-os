#!/usr/bin/env python3
"""Security regression tests for tools/sign.py key generation."""

import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest

import sign


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]


class KeyGenerationSecurityTests(unittest.TestCase):
    def run_in_temporary_directory(self, callback):
        original_directory = Path.cwd()
        with tempfile.TemporaryDirectory() as directory:
            os.chdir(directory)
            try:
                callback(Path(directory))
            finally:
                os.chdir(original_directory)

    def test_keygen_forces_private_key_mode_under_permissive_umask(self):
        def exercise(directory):
            old_umask = os.umask(0)
            try:
                sign.cmd_keygen(None)
            finally:
                os.umask(old_umask)

            private_key = directory / "private.key"
            public_key = directory / "public.key"
            self.assertEqual(private_key.stat().st_size, 32)
            self.assertEqual(public_key.stat().st_size, 32)
            self.assertEqual(stat.S_IMODE(private_key.stat().st_mode), 0o600)

        self.run_in_temporary_directory(exercise)

    def test_keygen_refuses_existing_private_key_without_modifying_it(self):
        def exercise(directory):
            private_key = directory / "private.key"
            private_key.write_bytes(b"existing-secret")
            private_key.chmod(0o644)

            with self.assertRaises(FileExistsError):
                sign.cmd_keygen(None)

            self.assertEqual(private_key.read_bytes(), b"existing-secret")
            self.assertFalse((directory / "public.key").exists())

        self.run_in_temporary_directory(exercise)

    def test_keygen_refuses_private_key_symlink_without_touching_target(self):
        def exercise(directory):
            victim = directory / "victim"
            victim.write_bytes(b"do-not-overwrite")
            (directory / "private.key").symlink_to(victim)

            with self.assertRaises(FileExistsError):
                sign.cmd_keygen(None)

            self.assertEqual(victim.read_bytes(), b"do-not-overwrite")
            self.assertFalse((directory / "public.key").exists())

        self.run_in_temporary_directory(exercise)

    def test_keygen_rejects_existing_public_path_before_private_key_sink(self):
        def exercise(directory):
            (directory / "public.key").write_bytes(b"existing-public-key")

            with self.assertRaises(FileExistsError):
                sign.cmd_keygen(None)

            self.assertFalse((directory / "private.key").exists())

        self.run_in_temporary_directory(exercise)

    def test_generated_private_key_name_is_ignored(self):
        result = subprocess.run(
            ["git", "check-ignore", "-q", "private.key"],
            cwd=REPOSITORY_ROOT,
            check=False,
        )
        self.assertEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
