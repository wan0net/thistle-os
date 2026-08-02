#!/usr/bin/env python3
"""Validate firmware and package architecture coverage against board profiles."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def advertised_architectures(board_dir: Path) -> set[str]:
    return {
        json.loads(path.read_text())["board"]["arch"]
        for path in board_dir.glob("*.json")
    }


def validate_firmware(board_arches: set[str], firmware_dir: Path) -> list[str]:
    errors = []
    for arch in sorted(board_arches):
        artifact = firmware_dir / f"thistle-os-firmware-{arch}" / "thistle_os.bin"
        if not artifact.is_file():
            errors.append(f"missing firmware artifact for advertised {arch}: {artifact}")
    return errors


def validate_packages(board_arches: set[str], package_dir: Path) -> list[str]:
    errors = []
    for manifest_path in package_dir.glob("**/manifest.json"):
        manifest = json.loads(manifest_path.read_text())
        arch = manifest.get("arch", "")
        if arch not in board_arches:
            errors.append(f"{manifest_path}: package declares unsupported architecture {arch!r}")
            continue
        relative = manifest_path.relative_to(package_dir)
        if not relative.parts or relative.parts[0] != arch:
            errors.append(f"{manifest_path}: package is not published under architecture {arch}")
            continue
        entry = manifest.get("entry", "")
        artifact = manifest_path.parent / entry
        if not entry or not artifact.is_file():
            errors.append(f"{manifest_path}: missing declared artifact {entry!r}")
            continue
        for suffix in (".sig", ".sha256"):
            if not Path(str(artifact) + suffix).is_file():
                errors.append(f"{manifest_path}: missing {suffix} for {entry}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--board-dir", type=Path, required=True)
    parser.add_argument("--firmware-dir", type=Path)
    parser.add_argument("--package-dir", type=Path)
    args = parser.parse_args()

    arches = advertised_architectures(args.board_dir)
    errors = []
    if args.firmware_dir:
        errors.extend(validate_firmware(arches, args.firmware_dir))
    if args.package_dir:
        errors.extend(validate_packages(arches, args.package_dir))
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1
    print(f"Validated architecture coverage: {', '.join(sorted(arches))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
