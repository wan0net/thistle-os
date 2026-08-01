#!/usr/bin/env python3
"""Regression checks for the supported ThistleOS architecture matrix."""

import json
import re
import unittest
from pathlib import Path


SUPPORTED_ARCHES = {"esp32", "esp32s2", "esp32s3", "esp32c3", "esp32c6"}


def workflow_arches(path: str) -> set[str]:
    text = Path(path).read_text()
    match = re.search(r"arch:\s*\[([^]]+)]", text)
    if not match:
        return set()
    return {item.strip() for item in match.group(1).split(",")}


class TargetMatrixTests(unittest.TestCase):
    def test_firmware_and_package_matrices_cover_supported_targets(self):
        self.assertEqual(workflow_arches(".github/workflows/build.yml"),
                         SUPPORTED_ARCHES)
        self.assertEqual(workflow_arches(".github/workflows/apps.yml"),
                         SUPPORTED_ARCHES)

    def test_h2_is_not_in_build_or_package_workflows(self):
        for path in [".github/workflows/build.yml", ".github/workflows/apps.yml"]:
            self.assertNotIn("esp32h2", Path(path).read_text().lower())

    def test_every_shipped_board_uses_a_supported_architecture(self):
        board_arches = {
            json.loads(path.read_text())["board"]["arch"]
            for path in Path("sdcard_layout/config/boards").glob("*.json")
        }
        self.assertTrue(board_arches)
        self.assertTrue(board_arches <= SUPPORTED_ARCHES)

    def test_rust_kernel_maps_every_idf_target(self):
        cmake = Path("components/kernel_rs/CMakeLists.txt").read_text()
        for arch in SUPPORTED_ARCHES:
            self.assertIn(f'"{arch}"', cmake)
        self.assertIn("riscv32imc-esp-espidf", cmake)

    def test_matrix_artifacts_are_architecture_qualified(self):
        firmware = Path(".github/workflows/build.yml").read_text()
        packages = Path(".github/workflows/apps.yml").read_text()
        release = Path(".github/workflows/release.yml").read_text()
        self.assertIn("thistle-os-firmware-${{ matrix.arch }}", firmware)
        self.assertIn('arch=metadata[\'arch\']', firmware)
        self.assertIn('manifest["arch"] = target_arch', packages)
        self.assertIn("python3 tools/validate_package_matrix.py", packages)
        self.assertIn("for arch in esp32 esp32s2 esp32s3 esp32c3 esp32c6", release)
        self.assertIn("thistle_os-${arch}-${RELEASE_TAG}.bin", release)

    def test_package_build_does_not_configure_unrelated_root_firmware(self):
        packages = Path(".github/workflows/apps.yml").read_text()
        self.assertNotIn('idf.py set-target "$TARGET_ARCH"', packages)

    def test_pinned_elf_loader_adapter_keeps_c3_in_supported_scope(self):
        adapter = Path("components/elf_loader/CMakeLists.txt").read_text()
        kconfig = Path("components/elf_loader/Kconfig").read_text()
        self.assertIn("URL_HASH SHA256=", adapter)
        self.assertIn("IDF_TARGET_ESP32C3", kconfig)


if __name__ == "__main__":
    unittest.main()
