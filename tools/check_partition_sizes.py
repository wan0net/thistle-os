#!/usr/bin/env python3
"""Check firmware artifacts against partition-table app slots."""

from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path


def parse_size(value: str) -> int:
    value = value.strip()
    return int(value, 16) if value.lower().startswith("0x") else int(value)


def partition_sizes(path: Path) -> dict[str, int]:
    sizes: dict[str, int] = {}
    for row in csv.reader(
        line for line in path.read_text().splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ):
        if len(row) < 5:
            continue
        name = row[0].strip()
        sizes[name] = parse_size(row[4])
    return sizes


def check(label: str, artifact: Path, limit: int) -> list[str]:
    if not artifact.exists():
        return [f"{label}: missing artifact {artifact}"]
    size = artifact.stat().st_size
    free = limit - size
    pct = size * 100 // limit
    print(f"{label}: {size} / {limit} bytes ({pct}% used, {free} free)")
    if size > limit:
        return [f"{label}: {size} exceeds partition limit {limit}"]
    return []


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--partition-table", type=Path, default=Path("partitions.csv"))
    parser.add_argument("--firmware", type=Path)
    parser.add_argument("--recovery", type=Path)
    args = parser.parse_args()

    sizes = partition_sizes(args.partition_table)
    errors: list[str] = []
    if args.firmware:
        errors.extend(check("firmware ota_1", args.firmware, sizes["ota_1"]))
    if args.recovery:
        errors.extend(check("recovery ota_0", args.recovery, sizes["ota_0"]))

    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
