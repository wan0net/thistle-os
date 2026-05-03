#!/usr/bin/env python3
"""Build a recovery-downloadable board catalog from sdcard board configs."""

from __future__ import annotations

import argparse
import hashlib
import json
from datetime import date
from pathlib import Path


def board_status(config: dict) -> str:
    notes = config.get("notes", {})
    if isinstance(notes, dict):
        return str(notes.get("status", "supported"))
    return "supported"


def build_catalog(board_dir: Path, base_url: str) -> dict:
    entries = []
    for path in sorted(board_dir.glob("*.json")):
        config = json.loads(path.read_text())
        board = config.get("board", {})
        board_id = board.get("board_id") or path.stem
        data = path.read_bytes()
        entries.append(
            {
                "id": board_id,
                "type": "board",
                "board_id": board_id,
                "name": board.get("name", board_id),
                "version": board.get("version", "0.1"),
                "arch": board.get("arch", ""),
                "status": board_status(config),
                "url": f"{base_url.rstrip('/')}/{path.name}",
                "sig_url": "",
                "sha256": hashlib.sha256(data).hexdigest(),
                "size_bytes": len(data),
                "driver_count": len(config.get("drivers", [])),
            }
        )

    return {
        "version": 1,
        "generated": date.today().isoformat(),
        "entries": entries,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("board_dir", type=Path)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--output", "-o", type=Path, required=True)
    args = parser.parse_args()

    catalog = build_catalog(args.board_dir, args.base_url)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(catalog, indent=2) + "\n")
    print(f"Wrote {args.output} ({len(catalog['entries'])} board entries)")


if __name__ == "__main__":
    main()
