#!/usr/bin/env python3
"""Build, package, install, and render standalone Notes on both WMs."""

from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "tools"))
import tap_package


@dataclass(frozen=True)
class Case:
    name: str
    device: str
    width: int
    height: int
    status_height: int
    expected_sha256: str


CASES = (
    Case("eink", "tdeck-pro", 240, 320, 0,
         "0d6ae0a8dcc201df0d86aca3ef3fa2e7628136dbc659d9298897a5ad7144b978"),
    Case("lcd", "tdeck", 320, 240, 24,
         "628b93bd6bf814c788d3afbd7537cf81cff86ec5eb2412c792ddf00decdd76b2"),
)
EDITOR_CASE = Case(
    "eink-editor", "tdeck-pro", 240, 320, 0,
    "82e2659a64427f1ac02019e33e52b22ff57bccd4bfa43d75f0689324414929d4",
)


def run(command: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(command, text=True, capture_output=True, check=False, **kwargs)
    if completed.returncode:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {' '.join(command)}\n"
            f"{completed.stdout}\n{completed.stderr}"
        )
    return completed


def read_masked_hash(path: Path, case: Case) -> str:
    data = path.read_bytes()
    header, payload = data.split(b"\n255\n", 1)
    width, height = (int(item) for item in header.splitlines()[-1].split())
    if (width, height) != (case.width, case.height):
        raise AssertionError(f"{case.name}: got {width}x{height}")
    pixels = bytearray(payload)
    pixels[: width * case.status_height * 3] = bytes(width * case.status_height * 3)
    return hashlib.sha256(pixels).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sim", type=Path, default=REPO / "simulator/build/thistle_sim")
    parser.add_argument("--output-dir", type=Path,
                        default=REPO / "simulator/build/tap-notes")
    args = parser.parse_args()
    simulator = args.sim.resolve()
    if not simulator.is_file():
        parser.error(f"simulator not found: {simulator}")
    args.output_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="thistle-tap-notes-") as directory:
        root = Path(directory)
        build = root / "build"
        run(["cmake", "-S", str(REPO / "standalone_apps/notes"),
             "-B", str(build), "-DTHISTLE_HOST_APP=ON"])
        run(["cmake", "--build", str(build), "--parallel"])
        elf = build / "notes.app.elf"

        private = Ed25519PrivateKey.generate()
        signature = root / "notes.app.elf.sig"
        signature.write_bytes(private.sign(elf.read_bytes()))
        public = root / "publisher.public.key"
        public.write_bytes(private.public_key().public_bytes_raw())
        package = root / "com.thistle.notes-1.0.0-host.tap"
        tap_package.build_package(
            REPO / "standalone_apps/notes/manifest.json",
            REPO / "standalone_apps/notes/metadata.json",
            elf, signature, REPO / "LICENSE", package, "host",
        )
        Path(f"{package}.sig").write_bytes(private.sign(package.read_bytes()))

        sdcard = root / "sdcard"
        tap_package.install_package(package, sdcard / "apps", public)
        environment = os.environ.copy()
        environment["THISTLE_SIM_SDCARD"] = str(sdcard)

        print("=== TAP Notes simulator regressions ===")
        failures = 0
        for case in CASES:
            screenshot = args.output_dir / f"notes-{case.name}.ppm"
            try:
                completed = run([
                    str(simulator), "--headless", "--device", case.device,
                    "--launch-app", "com.thistle.notes", "--timeout", "2500",
                    "--screenshot", str(screenshot),
                ], env=environment)
                output = completed.stdout + completed.stderr
                for expected in (
                    "TAP app registered: com.thistle.notes 1.0.0",
                    "App launched: com.thistle.notes",
                ):
                    if expected not in output:
                        raise AssertionError(f"missing simulator evidence: {expected}")
                actual = read_masked_hash(screenshot, case)
                if actual != case.expected_sha256:
                    raise AssertionError(
                        f"framebuffer changed; expected {case.expected_sha256}, got {actual}; "
                        f"capture: {screenshot}"
                    )
            except (AssertionError, RuntimeError, tap_package.TapError) as error:
                failures += 1
                print(f"FAIL  {case.name}: {error}")
            else:
                print(f"PASS  {case.name}")
        editor_screenshot = args.output_dir / "notes-eink-editor.ppm"
        try:
            completed = run([
                str(simulator), "--headless", "--device", EDITOR_CASE.device,
                "--launch-app", "com.thistle.notes", "--tap", "190,30",
                "--timeout", "1000", "--screenshot", str(editor_screenshot),
            ], env=environment)
            if "[notes] new note" not in completed.stdout + completed.stderr:
                raise AssertionError("packaged app button callback did not run")
            actual = read_masked_hash(editor_screenshot, EDITOR_CASE)
            if actual != EDITOR_CASE.expected_sha256:
                raise AssertionError(
                    f"editor framebuffer changed; expected {EDITOR_CASE.expected_sha256}, "
                    f"got {actual}; capture: {editor_screenshot}"
                )
        except (AssertionError, RuntimeError) as error:
            failures += 1
            print(f"FAIL  {EDITOR_CASE.name}: {error}")
        else:
            print(f"PASS  {EDITOR_CASE.name}")
        if failures:
            return 1
        print("All 3 packaged Notes visual and interaction tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
