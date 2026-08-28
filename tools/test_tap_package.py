#!/usr/bin/env python3
"""Tests for deterministic Thistle Application Packages."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tap_package


class TapPackageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.manifest = self.root / "manifest.json"
        self.metadata = self.root / "metadata.json"
        self.elf = self.root / "notes.app.elf"
        self.signature = self.root / "notes.app.elf.sig"
        self.license = self.root / "LICENSE"
        self.manifest.write_text(json.dumps({
            "type": "app", "id": "com.thistle.notes", "name": "Notes",
            "version": "1.0.0", "release_sequence": 1, "author": "ThistleOS",
            "description": "Local notes", "min_os": "0.3.0", "arch": "host",
            "permissions": ["storage"], "background": False, "min_memory_kb": 48,
        }))
        self.metadata.write_text(json.dumps({
            "schema": "thistle.app.metadata/v1", "id": "com.thistle.notes",
            "category": "productivity", "summary": "Local notes",
            "description": "Local notes", "license": "BSD-3-Clause",
            "releases": [{"version": "1.0.0", "release_sequence": 1,
                          "released": "2026-08-29", "changes": ["First release"]}],
        }))
        self.elf.write_bytes(b"host-app-elf")
        self.signature.write_bytes(bytes(range(64)))
        self.license.write_text("BSD-3-Clause\n")

    def tearDown(self):
        self.temp.cleanup()

    def build(self, name: str = "notes.tap") -> Path:
        output = self.root / name
        return tap_package.build_package(
            self.manifest, self.metadata, self.elf, self.signature,
            self.license, output, "host",
        )

    def rewrite(self, source: Path, output: Path, transform) -> None:
        with zipfile.ZipFile(source) as archive:
            entries = [(info.filename, archive.read(info)) for info in archive.infolist()]
        entries = transform(entries)
        with zipfile.ZipFile(output, "w", allowZip64=False) as archive:
            for name, data in entries:
                archive.writestr(tap_package._zip_info(name), data)

    def test_repeated_builds_are_byte_identical(self):
        first = self.build("first.tap")
        second = self.build("second.tap")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        package = tap_package.validate_package(first)
        self.assertEqual(package["arch"], "host")
        self.assertEqual(package["entry"], "app.app.elf")

    def test_installs_generation_and_atomically_activates_it(self):
        package_path = self.build()
        apps_root = self.root / "sd" / "apps"
        destination = tap_package.install_package(package_path, apps_root)
        self.assertEqual((destination / "app.app.elf").read_bytes(), self.elf.read_bytes())
        active = json.loads(
            (apps_root / "com.thistle.notes" / "active.json").read_text()
        )
        self.assertEqual(active["release_sequence"], 1)
        self.assertTrue((apps_root / "com.thistle.notes" / "receipt.json").is_file())

    def test_both_package_and_elf_signatures_are_required(self):
        private = Ed25519PrivateKey.generate()
        public = self.root / "publisher.public.key"
        public.write_bytes(private.public_key().public_bytes_raw())
        self.signature.write_bytes(private.sign(self.elf.read_bytes()))
        package_path = self.build("signed.tap")
        package_signature = Path(f"{package_path}.sig")
        package_signature.write_bytes(private.sign(package_path.read_bytes()))
        tap_package.verify_signatures(package_path, public)

        package_signature.write_bytes(b"x" * 64)
        with self.assertRaisesRegex(tap_package.TapError, "signature is invalid"):
            tap_package.verify_signatures(package_path, public)

        self.signature.write_bytes(b"y" * 64)
        bad_elf_package = self.build("bad-elf.tap")
        Path(f"{bad_elf_package}.sig").write_bytes(
            private.sign(bad_elf_package.read_bytes())
        )
        with self.assertRaisesRegex(tap_package.TapError, "signature is invalid"):
            tap_package.verify_signatures(bad_elf_package, public)

    def test_rejects_traversal_entry(self):
        source = self.build()
        malformed = self.root / "traversal.tap"
        self.rewrite(
            source, malformed,
            lambda entries: sorted(entries + [("../escape", b"x")]),
        )
        with self.assertRaisesRegex(tap_package.TapError, "invalid archive path"):
            tap_package.validate_package(malformed)

    def test_rejects_compressed_entry(self):
        source = self.build()
        malformed = self.root / "compressed.tap"
        with zipfile.ZipFile(source) as archive:
            entries = [(info.filename, archive.read(info)) for info in archive.infolist()]
        with zipfile.ZipFile(malformed, "w") as archive:
            for name, data in entries:
                archive.writestr(name, data, compress_type=zipfile.ZIP_DEFLATED)
        with self.assertRaisesRegex(tap_package.TapError, "compressed entry"):
            tap_package.validate_package(malformed)

    def test_rejects_payload_hash_mismatch(self):
        source = self.build()
        malformed = self.root / "hash.tap"
        self.rewrite(
            source, malformed,
            lambda entries: [(name, b"tampered" if name == "app.app.elf" else data)
                             for name, data in entries],
        )
        with self.assertRaisesRegex(tap_package.TapError, "integrity mismatch"):
            tap_package.validate_package(malformed)

    def test_rejects_metadata_release_disagreement(self):
        source = self.build()
        malformed = self.root / "metadata.tap"
        def transform(entries):
            changed = []
            for name, data in entries:
                if name == "metadata.json":
                    value = json.loads(data)
                    value["releases"][0]["version"] = "2.0.0"
                    data = tap_package.canonical_json(value)
                changed.append((name, data))
            return changed
        self.rewrite(source, malformed, transform)
        with self.assertRaisesRegex(tap_package.TapError, "does not match"):
            tap_package.validate_package(malformed)


if __name__ == "__main__":
    unittest.main()
