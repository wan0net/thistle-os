#!/usr/bin/env python3
"""Deterministic framebuffer regression tests for both launcher WMs."""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Case:
    name: str
    device: str
    timeout_ms: int
    tap: str | None
    width: int
    height: int
    expected_sha256: str
    empty_home: bool = False


CASES = (
    Case(
        "eink-home",
        "tdeck-pro",
        2500,
        None,
        240,
        320,
        "4c4bb06c6a0d9f79330e6650c90ceadbc100ceb4cb85518e72616a0dafa317b4",
        True,
    ),
    Case(
        "eink-apps",
        "tdeck-pro",
        2500,
        "120,295",
        240,
        320,
        "acaf9bcaf054b9c5577fe9f00fdf30cb89ece9f90cd2c012d28cd04ef46fe2cc",
    ),
    Case(
        "lcd-home",
        "tdeck",
        3000,
        None,
        320,
        240,
        "262d7fd552cc445ae3f4483446cfc82fd4e6ff452af09c2f0d935c1b351d3ba6",
        True,
    ),
    Case(
        "lcd-apps",
        "tdeck",
        3000,
        "183,215",
        320,
        240,
        "49262aedee08e1614e66e6fdbdc982d62b94d30c525c76ed6aa8a1a59d421567",
    ),
)

STATUS_BAR_HEIGHT = 24
DOCK_HEIGHT = 50


def read_ppm(path: Path) -> tuple[int, int, bytearray]:
    data = path.read_bytes()
    try:
        header, pixels = data.split(b"\n255\n", 1)
        lines = header.splitlines()
        if lines[0] != b"P6":
            raise ValueError("not a binary PPM")
        width, height = (int(value) for value in lines[-1].split())
    except (ValueError, IndexError) as exc:
        raise ValueError(f"invalid PPM: {path}") from exc
    if len(pixels) != width * height * 3:
        raise ValueError(f"incorrect pixel payload length in {path}")
    return width, height, bytearray(pixels)


def masked_hash(width: int, pixels: bytearray) -> str:
    # Clock and battery text are intentionally dynamic. Everything below the
    # system bar is stable and remains under exact framebuffer comparison.
    pixels[: width * STATUS_BAR_HEIGHT * 3] = bytes(width * STATUS_BAR_HEIGHT * 3)
    return hashlib.sha256(pixels).hexdigest()


def assert_empty_home(case: Case, pixels: bytearray) -> None:
    row_bytes = case.width * 3
    start = STATUS_BAR_HEIGHT * row_bytes
    end = (case.height - DOCK_HEIGHT) * row_bytes
    canvas = pixels[start:end]
    colors = Counter(bytes(canvas[i : i + 3]) for i in range(0, len(canvas), 3))
    dominant = colors.most_common(1)[0][1]
    ratio = dominant / (len(canvas) // 3)
    if ratio < 0.995:
        raise AssertionError(
            f"Home canvas is no longer empty: dominant-color ratio {ratio:.4f}"
        )


def run_case(simulator: Path, output_dir: Path, case: Case) -> None:
    screenshot = output_dir / f"{case.name}.ppm"
    command = [
        str(simulator),
        "--headless",
        "--device",
        case.device,
        "--timeout",
        str(case.timeout_ms),
        "--screenshot",
        str(screenshot),
    ]
    if case.tap:
        command.extend(("--tap", case.tap))

    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    if completed.returncode != 0:
        raise RuntimeError(
            f"simulator exited {completed.returncode}\n{completed.stdout}\n{completed.stderr}"
        )

    width, height, pixels = read_ppm(screenshot)
    if (width, height) != (case.width, case.height):
        raise AssertionError(
            f"expected {case.width}x{case.height}, got {width}x{height}"
        )
    if case.empty_home:
        assert_empty_home(case, pixels)

    actual = masked_hash(width, pixels)
    if actual != case.expected_sha256:
        raise AssertionError(
            f"framebuffer changed\n"
            f"      expected: {case.expected_sha256}\n"
            f"      actual:   {actual}\n"
            f"      capture:  {screenshot}"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sim", type=Path, default=Path("build/thistle_sim"))
    parser.add_argument(
        "--output-dir", type=Path, default=Path("build/launcher-visuals")
    )
    args = parser.parse_args()

    simulator = args.sim.resolve()
    if not simulator.is_file():
        parser.error(f"simulator not found: {simulator}")
    args.output_dir.mkdir(parents=True, exist_ok=True)

    failures = 0
    print("=== Launcher framebuffer regressions ===")
    for case in CASES:
        try:
            run_case(simulator, args.output_dir, case)
        except (AssertionError, RuntimeError, ValueError) as exc:
            failures += 1
            print(f"FAIL  {case.name}: {exc}")
        else:
            print(f"PASS  {case.name}")

    if failures:
        print(f"{failures}/{len(CASES)} launcher visual tests failed")
        return 1
    print(f"All {len(CASES)} launcher visual tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
