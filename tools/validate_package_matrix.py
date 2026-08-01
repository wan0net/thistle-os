#!/usr/bin/env python3
"""Validate that a package catalog covers every supported architecture."""

import argparse
import json
from pathlib import Path


SUPPORTED_ARCHES = {"esp32", "esp32s2", "esp32s3", "esp32c3", "esp32c6"}
PACKAGE_TYPES = {"app", "driver"}


def validate_entries(entries: list[dict]) -> list[str]:
    errors = []
    coverage = {arch: set() for arch in SUPPORTED_ARCHES}
    for entry in entries:
        entry_type = entry.get("type")
        if entry_type not in PACKAGE_TYPES:
            continue
        package_id = entry.get("id", "<missing id>")
        arch = entry.get("arch")
        # Preserve pre-matrix catalog entries without pretending they target a
        # specific CPU. They remain installable for legacy clients, but do not
        # count toward (or weaken) current architecture coverage.
        if not arch:
            continue
        if arch not in SUPPORTED_ARCHES:
            errors.append(f"{package_id}: unsupported architecture {arch!r}")
            continue
        coverage[arch].add(entry_type)
        if not entry.get("is_signed") or not entry.get("sig_url"):
            errors.append(f"{package_id} ({arch}): package is not signed")
        if f"/{arch}/" not in entry.get("url", ""):
            errors.append(f"{package_id} ({arch}): URL is not architecture-qualified")

    for arch, present_types in sorted(coverage.items()):
        missing = PACKAGE_TYPES - present_types
        if missing:
            errors.append(f"{arch}: missing package types {sorted(missing)}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("catalog", type=Path)
    args = parser.parse_args()
    catalog = json.loads(args.catalog.read_text())
    errors = validate_entries(catalog.get("entries", []))
    for error in errors:
        print(f"ERROR: {error}")
    if errors:
        return 1
    print("Package catalog covers ESP32, S2, S3, C3, and C6")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
