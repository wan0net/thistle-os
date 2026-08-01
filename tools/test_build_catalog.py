#!/usr/bin/env python3
"""Regression tests for standalone package catalog publication."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from build_catalog import scan_artifacts


class BuildCatalogTests(unittest.TestCase):
    def write_package(self, root: Path, kind: str, package_id: str) -> tuple[Path, bytes]:
        package_dir = root / "esp32s3" / f"{kind}s" / package_id
        package_dir.mkdir(parents=True)
        suffix = ".app.elf" if kind == "app" else ".drv.elf"
        elf_name = f"{package_id}{suffix}"
        elf_data = f"ELF:{package_id}".encode()
        (package_dir / elf_name).write_bytes(elf_data)
        (package_dir / f"{elf_name}.sig").write_bytes(b"s" * 64)
        (package_dir / "manifest.json").write_text(json.dumps({
            "id": f"com.thistle.{package_id}",
            "type": kind,
            "name": package_id,
            "version": "1.0.0",
            "arch": "esp32s3",
            "entry": elf_name,
        }))
        return package_dir, elf_data

    def test_signed_apps_and_drivers_are_catalogued_with_qualified_urls(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _, app_data = self.write_package(root, "app", "hello")
            _, driver_data = self.write_package(root, "driver", "keyboard")

            entries = scan_artifacts(str(root), "https://example.test/apps",
                                     require_signatures=True)

            self.assertEqual({entry["type"] for entry in entries}, {"app", "driver"})
            by_id = {entry["id"]: entry for entry in entries}
            self.assertEqual(by_id["com.thistle.hello"]["sha256"],
                             hashlib.sha256(app_data).hexdigest())
            self.assertEqual(by_id["com.thistle.keyboard"]["sha256"],
                             hashlib.sha256(driver_data).hexdigest())
            for entry in entries:
                self.assertTrue(entry["is_signed"])
                self.assertIn("/esp32s3/", entry["url"])
                self.assertEqual(entry["sig_url"], f'{entry["url"]}.sig')
                relative_url = entry["url"].removeprefix("https://example.test/apps/")
                self.assertTrue((root / relative_url).is_file())
                self.assertTrue((root / f"{relative_url}.sig").is_file())

    def test_missing_declared_elf_fails_catalog_generation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_dir = root / "esp32s3" / "drivers" / "missing"
            package_dir.mkdir(parents=True)
            (package_dir / "manifest.json").write_text(json.dumps({
                "id": "com.thistle.missing",
                "type": "driver",
                "entry": "missing.drv.elf",
            }))

            with self.assertRaises(FileNotFoundError):
                scan_artifacts(str(root), "https://example.test/apps")

    def test_unsigned_package_fails_when_signatures_are_required(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_dir, _ = self.write_package(root, "driver", "keyboard")
            (package_dir / "keyboard.drv.elf.sig").unlink()

            with self.assertRaises(FileNotFoundError):
                scan_artifacts(str(root), "https://example.test/apps",
                               require_signatures=True)


if __name__ == "__main__":
    unittest.main()
