#!/usr/bin/env python3
"""Regression tests for component-accurate board profile validation."""

import copy
import json
import tempfile
import unittest
from pathlib import Path

from validate_board_catalog import driver_sources, validate_board


REPO = Path(__file__).resolve().parents[1]


class BoardCatalogValidationTests(unittest.TestCase):
    def setUp(self):
        self.profile = json.loads(
            (REPO / "sdcard_layout/config/boards/twatch-ultra.json").read_text()
        )
        self.known_ids, self.known_entries = driver_sources(REPO)

    def validate(self, profile: dict) -> list[str]:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "twatch-ultra.json"
            path.write_text(json.dumps(profile))
            return validate_board(path, self.known_ids, self.known_entries)

    def test_twatch_profile_is_component_accurate(self):
        self.assertEqual(self.validate(self.profile), [])

    def test_partial_fingerprint_is_rejected(self):
        profile = copy.deepcopy(self.profile)
        profile["fingerprint"]["required"].pop()
        errors = self.validate(profile)
        self.assertTrue(any("complete I2C tuple" in error for error in errors))

    def test_malformed_fingerprint_address_is_reported_not_crashed(self):
        profile = copy.deepcopy(self.profile)
        profile["fingerprint"]["required"][0]["address"] = "not-an-address"
        errors = self.validate(profile)
        self.assertTrue(any("invalid address" in error for error in errors))

    def test_wrong_touch_address_is_rejected(self):
        profile = copy.deepcopy(self.profile)
        touch = next(
            driver for driver in profile["drivers"]
            if driver["id"] == "com.thistle.drv.touch-cst9217"
        )
        touch["config"]["i2c_addr"] = "0x5A"
        errors = self.validate(profile)
        self.assertTrue(any("CST9217 address" in error for error in errors))

    def test_preselected_radio_is_rejected(self):
        profile = copy.deepcopy(self.profile)
        profile["drivers"].append({
            "id": "com.thistle.drv.radio-sx1262",
            "hal": "radio",
            "entry": "radio-sx1262.drv.elf",
            "config": {},
        })
        errors = self.validate(profile)
        self.assertTrue(any("must not preselect" in error for error in errors))

    def test_incorrect_legacy_devices_are_rejected(self):
        profile = copy.deepcopy(self.profile)
        profile["drivers"].append({
            "id": "com.thistle.drv.rtc-pcf8563",
            "hal": "rtc",
            "entry": "pcf8563.drv.elf",
            "config": {},
        })
        errors = self.validate(profile)
        self.assertTrue(any("incorrect audio or RTC" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
