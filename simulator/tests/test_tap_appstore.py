#!/usr/bin/env python3
"""TAP-only App Store browse/detail simulator regression."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


REPO = Path(__file__).resolve().parents[2]
EXPECTED = {
    "browse": "f408d5ae54dd44e062f6c9f411fdc19e5246b6a4084f4c485f11ced9329f7fb9",
    "detail": "639418b50c580879d7b5ad019efe6c0b6fdfbcbf47066be4a15ae93639b925e9",
}


def framebuffer_hash(path: Path) -> str:
    _, pixels = path.read_bytes().split(b"\n255\n", 1)
    return hashlib.sha256(pixels).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sim", type=Path, default=REPO / "simulator/build/thistle_sim")
    parser.add_argument("--output-dir", type=Path,
                        default=REPO / "simulator/build/tap-appstore")
    args = parser.parse_args()
    simulator = args.sim.resolve()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    entries = []
    for sequence, (app_id, name, category) in enumerate((
        ("com.thistle.notes", "Notes", "productivity"),
        ("com.thistle.flashlight", "Flashlight", "utilities"),
    ), 1):
        entries.append({
            "id": app_id, "type": "app", "name": name, "version": "1.0.0",
            "release_sequence": sequence, "author": "ThistleOS",
            "description": f"{name} for ThistleOS", "category": category,
            "package_url": f"https://example.invalid/{name.lower()}.tap",
            "package_sig_url": f"https://example.invalid/{name.lower()}.tap.sig",
            "package_sha256": "a" * 64, "package_size_bytes": 4096,
            "publisher_key_id": "official-2026", "permissions": "storage",
            "arch": "host", "rating": 4.5, "rating_count": 10, "downloads": 100,
        })

    with tempfile.TemporaryDirectory(prefix="thistle-tap-store-") as directory:
        sdcard = Path(directory)
        (sdcard / "config").mkdir()
        (sdcard / "config/catalog_cache.json").write_text(json.dumps({
            "version": 1, "entries": entries,
        }))
        environment = os.environ.copy()
        environment["THISTLE_SIM_SDCARD"] = str(sdcard)
        cases = (("browse", None), ("detail", "100,75"))
        failures = 0
        print("=== TAP App Store simulator regressions ===")
        for name, tap in cases:
            screenshot = args.output_dir / f"appstore-{name}.ppm"
            command = [
                str(simulator), "--headless", "--device", "tdeck-pro",
                "--launch-app", "com.thistle.appstore", "--timeout", "1000",
                "--screenshot", str(screenshot),
            ]
            if tap:
                command.extend(("--tap", tap))
            completed = subprocess.run(command, env=environment, text=True,
                                       capture_output=True, check=False)
            actual = framebuffer_hash(screenshot) if completed.returncode == 0 else "no-frame"
            if actual != EXPECTED[name]:
                failures += 1
                print(f"FAIL  {name}: expected {EXPECTED[name]}, got {actual}")
            else:
                print(f"PASS  {name}")
        if failures:
            return 1
    print("All 2 TAP App Store visual and interaction tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
