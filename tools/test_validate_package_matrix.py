#!/usr/bin/env python3
"""Tests for package architecture coverage validation."""

import unittest

from validate_package_matrix import SUPPORTED_ARCHES, validate_entries


def entry(arch: str, package_type: str) -> dict:
    suffix = "app.elf" if package_type == "app" else "drv.elf"
    return {
        "id": f"com.thistle.{arch}.{package_type}",
        "type": package_type,
        "arch": arch,
        "url": f"https://example.test/apps/{arch}/{package_type}s/pkg.{suffix}",
        "sig_url": f"https://example.test/apps/{arch}/{package_type}s/pkg.{suffix}.sig",
        "is_signed": True,
    }


class PackageMatrixValidationTests(unittest.TestCase):
    def test_complete_signed_matrix_passes(self):
        entries = [
            entry(arch, package_type)
            for arch in SUPPORTED_ARCHES
            for package_type in ["app", "driver"]
        ]
        self.assertEqual(validate_entries(entries), [])

    def test_missing_architecture_and_unsigned_package_fail(self):
        entries = [
            entry(arch, package_type)
            for arch in SUPPORTED_ARCHES - {"esp32c6"}
            for package_type in ["app", "driver"]
        ]
        entries[0]["is_signed"] = False
        errors = validate_entries(entries)
        self.assertTrue(any("not signed" in error for error in errors))
        self.assertTrue(any("esp32c6: missing" in error for error in errors))

    def test_legacy_unqualified_entries_do_not_break_current_matrix(self):
        entries = [
            entry(arch, package_type)
            for arch in SUPPORTED_ARCHES
            for package_type in ["app", "driver"]
        ]
        entries.append({
            "id": "com.thistle.legacy",
            "type": "app",
            "url": "https://example.test/apps/legacy.app.elf",
        })

        self.assertEqual(validate_entries(entries), [])


if __name__ == "__main__":
    unittest.main()
