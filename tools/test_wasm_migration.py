#!/usr/bin/env python3
"""Regression tests for the WASM simulator during the C-to-Rust app migration."""

from pathlib import Path
import unittest


class WasmMigrationTests(unittest.TestCase):
    def setUp(self):
        self.cmake = Path("wasm/CMakeLists.txt").read_text()
        self.main = Path("wasm/main.c").read_text()
        self.workflow = Path(".github/workflows/pages.yml").read_text()

    def test_cmake_filters_sources_removed_after_rust_migration(self):
        self.assertIn("set(THISTLE_WASM_SRCS)", self.cmake)
        self.assertIn('if(EXISTS "${source}")', self.cmake)
        self.assertIn(
            "add_executable(thistle_wasm ${WASM_SRCS} ${THISTLE_WASM_SRCS} ${LVGL_SRCS})",
            self.cmake,
        )

    def test_deleted_c_apps_are_guarded_in_browser_entrypoint(self):
        migrated_apps = {
            "FILEMGR": "filemgr_app_register();",
            "READER": "reader_app_register();",
            "NAVIGATOR": "navigator_app_register();",
            "NOTES": "notes_app_register();",
            "APPSTORE": "appstore_app_register();",
            "WIFISCANNER": "wifiscanner_app_register();",
            "FLASHLIGHT": "flashlight_app_register();",
            "WEATHER": "weather_app_register();",
            "VAULT": "vault_app_register();",
        }
        for flag, registration in migrated_apps.items():
            with self.subTest(flag=flag):
                guard = f"#if THISTLE_HAVE_{flag}"
                self.assertIn(guard, self.main)
                self.assertGreater(self.main.index(registration), self.main.index(guard))

    def test_pull_requests_build_wasm_without_deploying_pages(self):
        self.assertIn("pull_request:", self.workflow)
        self.assertIn("if: github.event_name == 'push'", self.workflow)


if __name__ == "__main__":
    unittest.main()
