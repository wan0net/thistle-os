#!/usr/bin/env python3
"""Validate board configs and generated recovery board catalogs."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


REQUIRED_BOARD_KEYS = {"name", "arch", "board_id", "version"}
VALID_ARCHES = {"esp32", "esp32s2", "esp32s3", "esp32c3", "esp32c6"}
VALID_FINGERPRINT_BUSES = {"i2c", "spi", "uart"}
DRIVER_ENTRY_ALIASES = {
    "lcd-st7789-i80.drv.elf": "lcd-st7789.drv.elf",
    "qmi8658.drv.elf": "accel-qmi8658.drv.elf",
    "touch-ft3168.drv.elf": "touch-ft3x68.drv.elf",
}


def driver_sources(repo: Path) -> tuple[set[str], set[str]]:
    known_ids: set[str] = set()
    known_entries: set[str] = set()
    for path in (repo / "sdcard_layout/drivers").glob("*.manifest.json"):
        data = json.loads(path.read_text())
        if data.get("id"):
            known_ids.add(data["id"])
        if data.get("entry"):
            known_entries.add(data["entry"])
    for path in (repo / "components/kernel_rs/src").glob("drv_*.rs"):
        slug = path.stem.removeprefix("drv_").replace("_", "-")
        known_ids.add(f"com.thistle.drv.{slug}")
        known_entries.add(f"{slug}.drv.elf")
    for path in (repo / "components").glob("drv_*"):
        slug = path.name.removeprefix("drv_").replace("_", "-")
        known_ids.add(f"com.thistle.drv.{slug}")
        known_entries.add(f"{slug}.drv.elf")
    return known_ids, known_entries


def validate_board(path: Path, known_ids: set[str], known_entries: set[str]) -> list[str]:
    errors: list[str] = []
    data = json.loads(path.read_text())
    board = data.get("board", {})
    missing = REQUIRED_BOARD_KEYS - set(board)
    if missing:
        errors.append(f"{path}: board missing keys: {', '.join(sorted(missing))}")
    if board.get("arch") not in VALID_ARCHES:
        errors.append(f"{path}: unsupported arch {board.get('arch')!r}")
    if board.get("board_id") != path.stem:
        errors.append(f"{path}: board_id must match filename stem {path.stem!r}")
    if not data.get("drivers"):
        errors.append(f"{path}: must declare at least one driver")

    for idx, drv in enumerate(data.get("drivers", [])):
        drv_id = drv.get("id")
        if not drv_id:
            errors.append(f"{path}: drivers[{idx}] missing id")
            continue
        entry = drv.get("entry", "")
        aliased_entry = DRIVER_ENTRY_ALIASES.get(entry, entry)
        if drv_id not in known_ids and entry not in known_entries and aliased_entry not in known_entries:
            errors.append(f"{path}: {drv_id} / {entry} has no manifest/source driver")
        if not entry.endswith(".drv.elf"):
            errors.append(f"{path}: {drv_id} entry must end with .drv.elf")
        if "hal" not in drv:
            errors.append(f"{path}: {drv_id} missing hal")

    fingerprint = data.get("fingerprint")
    if fingerprint is not None:
        if fingerprint.get("match") != "all":
            errors.append(f"{path}: fingerprint match must be 'all'")
        required = fingerprint.get("required")
        if not isinstance(required, list) or len(required) < 2:
            errors.append(f"{path}: fingerprint requires at least two components")
        else:
            seen: set[tuple[str, int, int]] = set()
            for idx, component in enumerate(required):
                bus = component.get("bus")
                bus_index = component.get("bus_index")
                address = component.get("address")
                if bus not in VALID_FINGERPRINT_BUSES:
                    errors.append(f"{path}: fingerprint.required[{idx}] has invalid bus")
                    continue
                if not isinstance(bus_index, int) or bus_index < 0:
                    errors.append(f"{path}: fingerprint.required[{idx}] has invalid bus_index")
                    continue
                try:
                    numeric_address = int(address, 0) if isinstance(address, str) else int(address)
                except (TypeError, ValueError):
                    errors.append(f"{path}: fingerprint.required[{idx}] has invalid address")
                    continue
                if bus == "i2c" and not 0 < numeric_address < 0x80:
                    errors.append(f"{path}: fingerprint.required[{idx}] I2C address is out of range")
                key = (bus, bus_index, numeric_address)
                if key in seen:
                    errors.append(f"{path}: fingerprint contains duplicate component address {key}")
                seen.add(key)
        if fingerprint.get("reject_partial") is not True:
            errors.append(f"{path}: fingerprint must reject partial matches")

    if board.get("board_id") == "twatch-ultra":
        expected_addresses = {0x1A, 0x20, 0x28, 0x34, 0x51, 0x5A}
        actual_addresses = set()
        for item in data.get("fingerprint", {}).get("required", []):
            if item.get("bus") != "i2c":
                continue
            try:
                address = item.get("address")
                actual_addresses.add(int(address, 0) if isinstance(address, str) else int(address))
            except (TypeError, ValueError):
                pass
        if actual_addresses != expected_addresses:
            errors.append(f"{path}: T-Watch Ultra fingerprint must use the complete I2C tuple")
        forbidden = {"com.thistle.drv.audio-pcm5102a", "com.thistle.drv.rtc-pcf8563"}
        configured_ids = {driver.get("id") for driver in data.get("drivers", [])}
        if configured_ids & forbidden:
            errors.append(f"{path}: T-Watch Ultra declares an incorrect audio or RTC driver")
        touch = next((driver for driver in data.get("drivers", []) if driver.get("id") == "com.thistle.drv.touch-cst9217"), None)
        if not touch or touch.get("config", {}).get("i2c_addr") != "0x1A":
            errors.append(f"{path}: T-Watch Ultra CST9217 address must be 0x1A")
        radio = data.get("variants", {}).get("radio", {})
        supported = set(radio.get("supported", []))
        if radio.get("selection") != "explicit" or not {"sx1262", "sx1280"} <= supported:
            errors.append(f"{path}: T-Watch Ultra radio variant must require explicit selection")
        if any(driver.get("hal") == "radio" for driver in data.get("drivers", [])):
            errors.append(f"{path}: T-Watch Ultra base profile must not preselect a radio driver")
    return errors


def validate_catalog(path: Path, board_ids: set[str]) -> list[str]:
    errors: list[str] = []
    data = json.loads(path.read_text())
    entries = data.get("entries", [])
    catalog_board_ids = {
        entry.get("board_id") or entry.get("id")
        for entry in entries
        if entry.get("type") == "board"
    }
    missing = board_ids - catalog_board_ids
    extra = catalog_board_ids - board_ids
    if missing:
        errors.append(f"{path}: missing board entries: {', '.join(sorted(missing))}")
    if extra:
        errors.append(f"{path}: unknown board entries: {', '.join(sorted(extra))}")
    for entry in entries:
        if entry.get("type") != "board":
            continue
        if not re.match(r"^https?://", entry.get("url", "")):
            errors.append(f"{path}: {entry.get('id')} url must be absolute http(s)")
        if not entry.get("sha256"):
            errors.append(f"{path}: {entry.get('id')} missing sha256")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, default=Path("."))
    parser.add_argument("--catalog", type=Path)
    args = parser.parse_args()

    repo = args.repo.resolve()
    boards = sorted((repo / "sdcard_layout/config/boards").glob("*.json"))
    known_ids, known_entries = driver_sources(repo)
    errors: list[str] = []
    board_ids: set[str] = set()

    for path in boards:
        data = json.loads(path.read_text())
        board_ids.add(data.get("board", {}).get("board_id", path.stem))
        errors.extend(validate_board(path, known_ids, known_entries))

    if args.catalog:
        errors.extend(validate_catalog(args.catalog, board_ids))

    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print(f"Validated {len(boards)} board configs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
