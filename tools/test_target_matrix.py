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
        self.assertIn('set(RUST_TARGET_FEATURES "-C target-feature=+a")', cmake)
        self.assertNotIn("INTERFACE atomic", cmake)

    def test_c6_omits_unavailable_global_gpio_hold_api(self):
        source = Path("components/kernel_rs/src/board_config.rs").read_text()
        self.assertIn('not(feature = "esp32c6")', source)
        self.assertIn("gpio_deep_sleep_hold_dis", source)

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
        self.assertIn("vendor/elf_loader", adapter)
        self.assertNotIn("file(DOWNLOAD", adapter)
        self.assertTrue(Path("components/elf_loader/vendor/elf_loader/license.txt").is_file())
        self.assertIn("IDF_TARGET_ESP32C3", kconfig)

    def test_legacy_s3_board_components_are_not_built_for_other_targets(self):
        for path in [
            "components/board_tdeck/CMakeLists.txt",
            "components/board_tdeck_pro/CMakeLists.txt",
        ]:
            self.assertIn('IDF_TARGET STREQUAL "esp32s3"', Path(path).read_text())

    def test_terminal_selects_an_available_uart(self):
        terminal = Path(
            "components/apps_builtin/terminal/terminal_ui.c"
        ).read_text()
        self.assertIn("CONFIG_IDF_TARGET_ESP32S3", terminal)
        self.assertNotIn("CONFIG_IDF_TARGET_ESP32S2", terminal)
        self.assertIn("UART_NUM_1", terminal)

    def test_embedded_tests_are_not_linked_into_production_firmware(self):
        test_cmake = Path("components/test_thistle/CMakeLists.txt").read_text()
        main_cmake = Path("main/CMakeLists.txt").read_text()
        self.assertIn("if(CONFIG_THISTLE_RUN_TESTS)", test_cmake)
        self.assertIn("if(CONFIG_THISTLE_RUN_TESTS)", main_cmake)
        self.assertNotIn("test_thistle\n)", main_cmake)

    def test_non_s3_targets_do_not_link_legacy_c_app_suite(self):
        main_cmake = Path("main/CMakeLists.txt").read_text()
        main_source = Path("main/main.c").read_text()
        self.assertIn('IDF_TARGET STREQUAL "esp32s3"', main_cmake)
        self.assertIn("THISTLE_LEGACY_C_APPS", main_source)
        self.assertIn("set(thistle_main_requires kernel thistle_hal)", main_cmake)
        self.assertNotIn(
            "set(thistle_main_requires kernel thistle_hal ui apps_builtin)",
            main_cmake,
        )

    def test_file_manager_joins_paths_with_explicit_bounds(self):
        source = Path("apps/file_manager/file_manager/filemgr_ui.c").read_text()
        self.assertIn("static bool join_path", source)
        self.assertNotIn('snprintf(full_path, 512, "/%s"', source)

    def test_lvgl_draw_buffers_are_runtime_sized(self):
        source = Path("components/ui/src/manager.c").read_text()
        self.assertIn("DRAW_BUF_LINES", source)
        self.assertIn("heap_caps_malloc", source)
        self.assertNotIn("s_draw_buf1[MAX_DISPLAY_WIDTH", source)

    def test_meshcore_compatibility_warning_is_scoped_to_vendor_component(self):
        cmake = Path("components/meshcore/CMakeLists.txt").read_text()
        self.assertIn("-Wno-class-memaccess", cmake)


if __name__ == "__main__":
    unittest.main()
