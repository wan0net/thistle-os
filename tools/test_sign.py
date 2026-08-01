#!/usr/bin/env python3
"""Security regression tests for tools/sign.py key generation."""

import os
from pathlib import Path
import stat
import subprocess
import tempfile
from types import SimpleNamespace
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


class ArtifactManifestTests(unittest.TestCase):
    def test_arch_wide_firmware_manifest_allows_no_board_profiles(self):
        manifest = sign.canonical_manifest(
            artifact_type="firmware",
            artifact_id="thistle-os",
            version="0.5.0",
            security_version=500,
            arch="esp32s2",
            compatible_boards=[],
            url="https://downloads.example/thistle-os-esp32s2.bin",
            payload=b"firmware bytes",
        )
        self.assertIn(b"arch=esp32s2\n", manifest)
        self.assertIn(b"compatible_boards=\n", manifest)

    def test_non_firmware_manifest_still_requires_a_board_profile(self):
        with self.assertRaisesRegex(ValueError, "compatible board"):
            sign.canonical_manifest(
                artifact_type="driver",
                artifact_id="kbd",
                version="1.0.0",
                security_version=1,
                arch="esp32s2",
                compatible_boards=[],
                url="https://downloads.example/kbd.drv.elf",
                payload=b"driver bytes",
            )

    def test_manifest_signature_binds_every_security_field(self):
        private_key = sign.Ed25519PrivateKey.generate()
        payload = b"firmware bytes"
        fields = {
            "artifact_type": "firmware",
            "artifact_id": "thistle-os",
            "version": "0.5.0",
            "security_version": 500,
            "arch": "esp32s3",
            "compatible_boards": ["tdeck-pro", "tdeck"],
            "url": "https://downloads.example/thistle-os.bin",
            "payload": payload,
        }
        canonical = sign.canonical_manifest(**fields)
        signature = private_key.sign(canonical)
        private_key.public_key().verify(signature, canonical)

        mutations = {
            "artifact_type": "driver",
            "artifact_id": "other",
            "version": "0.4.0",
            "security_version": 499,
            "arch": "esp32c3",
            "compatible_boards": ["c3-mini"],
            "url": "https://downloads.example/other.bin",
            "payload": b"other bytes",
        }
        for field, value in mutations.items():
            changed = dict(fields)
            changed[field] = value
            with self.subTest(field=field), self.assertRaises(sign.InvalidSignature):
                private_key.public_key().verify(
                    signature, sign.canonical_manifest(**changed)
                )

    def test_manifest_generation_is_deterministic_and_sorts_boards(self):
        arguments = {
            "artifact_type": "board",
            "artifact_id": "tdeck-pro",
            "version": "1.0.0",
            "security_version": 1,
            "arch": "esp32s3",
            "compatible_boards": ["tdeck-pro", "tdeck", "tdeck-pro"],
            "url": "https://downloads.example/tdeck-pro.json",
            "payload": b"{}",
        }
        first = sign.canonical_manifest(**arguments)
        second = sign.canonical_manifest(**arguments)
        self.assertEqual(first, second)
        self.assertIn(b"compatible_boards=tdeck,tdeck-pro\n", first)
        self.assertIn(b"destination=config/boards/tdeck-pro.json\n", first)

    def test_manifest_command_writes_verifiable_sidecars(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            key = sign.Ed25519PrivateKey.generate()
            key_path = root / "private.key"
            payload_path = root / "driver.elf"
            key_path.write_bytes(key.private_bytes_raw())
            payload_path.write_bytes(b"driver bytes")
            sign.cmd_manifest(SimpleNamespace(
                key=str(key_path),
                file=str(payload_path),
                type="driver",
                id="qmi8658c",
                version="1.0.0",
                security_version=1,
                arch="esp32s3",
                compatible_board=["tdeck-pro"],
                url="https://downloads.example/qmi8658c.drv.elf",
            ))
            manifest = Path(f"{payload_path}.manifest").read_bytes()
            signature = Path(f"{payload_path}.manifest.sig").read_bytes()
            key.public_key().verify(signature, manifest)


if __name__ == "__main__":
    unittest.main()
