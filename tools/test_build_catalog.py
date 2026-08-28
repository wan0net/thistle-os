#!/usr/bin/env python3
"""Regression tests for standalone package catalog publication."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from build_catalog import merge_catalog_entries, scan_artifacts
from tap_package import build_package


class BuildCatalogTests(unittest.TestCase):
    def write_package(self, root: Path, kind: str, package_id: str) -> tuple[Path, bytes]:
        package_dir = root / "esp32s3" / f"{kind}s" / package_id
        package_dir.mkdir(parents=True)
        suffix = ".app.elf" if kind == "app" else ".drv.elf"
        elf_name = f"{package_id}{suffix}"
        elf_data = f"ELF:{package_id}".encode()
        (package_dir / elf_name).write_bytes(elf_data)
        (package_dir / f"{elf_name}.sig").write_bytes(b"s" * 64)
        manifest = {
            "id": f"com.thistle.{package_id}",
            "type": kind,
            "name": package_id,
            "version": "1.0.0",
            "release_sequence": 1,
            "author": "ThistleOS",
            "description": f"{package_id} package",
            "min_os": "0.3.0",
            "arch": "esp32s3",
            "entry": elf_name,
            "permissions": [],
            "background": False,
            "min_memory_kb": 16,
        }
        (package_dir / "manifest.json").write_text(json.dumps(manifest))
        if kind == "app":
            metadata = package_dir / "metadata.json"
            metadata.write_text(json.dumps({
                "schema": "thistle.app.metadata/v1", "id": manifest["id"],
                "category": "tools", "summary": f"{package_id} app",
                "description": f"{package_id} app", "license": "BSD-3-Clause",
                "releases": [{"version": "1.0.0", "release_sequence": 1,
                              "released": "2026-08-29", "changes": ["Initial"]}],
            }))
            license_path = package_dir / "LICENSE.source"
            license_path.write_text("BSD-3-Clause\n")
            tap = package_dir / f"{manifest['id']}-1.0.0-esp32s3.tap"
            build_package(
                package_dir / "manifest.json", metadata,
                package_dir / elf_name, package_dir / f"{elf_name}.sig",
                license_path, tap,
            )
            Path(f"{tap}.sig").write_bytes(b"t" * 64)
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
            self.assertNotIn("url", by_id["com.thistle.hello"])
            self.assertNotIn("sha256", by_id["com.thistle.hello"])
            self.assertTrue(by_id["com.thistle.hello"]["package_url"].endswith(".tap"))
            self.assertEqual(by_id["com.thistle.keyboard"]["sha256"],
                             hashlib.sha256(driver_data).hexdigest())
            for entry in entries:
                self.assertTrue(entry["is_signed"])
                delivery_url = entry.get("package_url", entry.get("url"))
                self.assertIn("/esp32s3/", delivery_url)
                relative_url = delivery_url.removeprefix("https://example.test/apps/")
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

    def test_apps_publish_only_tap_delivery_fields(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.write_package(root, "app", "notes")

            entries = scan_artifacts(
                str(root), "https://example.test/apps",
                require_signatures=True,
                publisher_key_id="official-2026",
            )
            entry = entries[0]
            self.assertNotIn("url", entry)
            self.assertNotIn("sig_url", entry)
            self.assertNotIn("sha256", entry)
            self.assertNotIn("size_bytes", entry)
            self.assertTrue(entry["package_url"].endswith(".tap"))
            self.assertEqual(entry["package_sig_url"], f'{entry["package_url"]}.sig')
            self.assertEqual(entry["publisher_key_id"], "official-2026")
            self.assertEqual(entry["release_sequence"], 1)

    def test_unsigned_package_fails_when_signatures_are_required(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package_dir, _ = self.write_package(root, "driver", "keyboard")
            (package_dir / "keyboard.drv.elf.sig").unlink()

            with self.assertRaises(FileNotFoundError):
                scan_artifacts(str(root), "https://example.test/apps",
                               require_signatures=True)

    def test_merge_drops_unrebuilt_apps_but_preserves_other_components(self):
        new_entries = [{"id": "com.thistle.hello", "type": "app", "rating": 0,
                        "rating_count": 0, "downloads": 0}]
        existing_entries = [
            {"id": "com.thistle.hello", "rating": 4.5,
             "rating_count": 12, "downloads": 99},
            {"id": "com.thistle.legacy", "type": "app",
             "url": "https://example.test/legacy.app.elf"},
            {"id": "com.thistle.keyboard", "type": "driver",
             "url": "https://example.test/keyboard.drv.elf"},
        ]

        merged = merge_catalog_entries(new_entries, existing_entries)

        by_id = {entry["id"]: entry for entry in merged}
        self.assertEqual(set(by_id), {"com.thistle.hello", "com.thistle.keyboard"})
        self.assertEqual(by_id["com.thistle.hello"]["rating"], 4.5)
        self.assertEqual(by_id["com.thistle.hello"]["downloads"], 99)
        self.assertNotIn("com.thistle.legacy", by_id)
        self.assertEqual(by_id["com.thistle.keyboard"]["url"],
                         "https://example.test/keyboard.drv.elf")


if __name__ == "__main__":
    unittest.main()
