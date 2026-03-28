#!/usr/bin/env python3
"""Generate catalog.json from built ThistleOS app/driver artifacts."""

import json
import hashlib
import sys
import argparse
from datetime import date
from pathlib import Path


def scan_artifacts(artifact_dir: str, base_url: str) -> list:
    """Scan directory for .app.elf/.drv.elf + manifest.json pairs."""
    entries = []
    artifact_path = Path(artifact_dir)

    manifest_files = sorted(
        set(list(artifact_path.glob("**/manifest.json"))
            + list(artifact_path.glob("**/*.manifest.json")))
    )

    for manifest_file in manifest_files:
        manifest = json.loads(manifest_file.read_text())

        # Find corresponding ELF
        elf_name = manifest.get("entry", "")
        if not elf_name:
            continue
        elf_path = manifest_file.parent / elf_name
        if not elf_path.exists():
            print(f"Warning: ELF not found for {manifest_file}: {elf_name}",
                  file=sys.stderr)
            continue

        # Compute SHA-256
        elf_data = elf_path.read_bytes()
        sha256 = hashlib.sha256(elf_data).hexdigest()
        size = len(elf_data)

        # Check for signature
        sig_path = elf_path.with_suffix(elf_path.suffix + ".sig")
        has_sig = sig_path.exists()

        # Build relative URL path
        try:
            rel_path = elf_path.relative_to(artifact_path)
        except ValueError:
            print(f"Warning: skipping {manifest_file} — entry path outside artifact dir", file=sys.stderr)
            continue
        elf_url = f"{base_url}/{rel_path}"
        sig_url = f"{base_url}/{rel_path}.sig" if has_sig else ""

        # Map manifest fields to catalog entry
        entry_type = manifest.get("type", "app")
        permissions = manifest.get("permissions", [])
        if isinstance(permissions, list):
            permissions = ",".join(permissions)

        entry = {
            "id": manifest.get("id", ""),
            "type": entry_type,
            "name": manifest.get("name", ""),
            "version": manifest.get("version", "1.0.0"),
            "author": manifest.get("author", ""),
            "description": manifest.get("description", ""),
            "category": manifest.get("category", "tools"),
            "url": elf_url,
            "sig_url": sig_url,
            "sha256": sha256,
            "size_bytes": size,
            "permissions": permissions,
            "min_os_version": manifest.get("min_os", ""),
            "arch": manifest.get("arch", ""),
            "compatible_boards": manifest.get("compatible_boards", []),
            "is_signed": has_sig,
            "updated": "",
            "changelog": manifest.get("changelog", ""),
            "rating": 0,
            "rating_count": 0,
            "downloads": 0,
        }

        # Driver-specific detection fields
        if entry_type == "driver" and "detection" in manifest:
            det = manifest["detection"]
            entry["detection"] = {
                "bus": det.get("bus", ""),
                "address": det.get("address", 0),
                "chip_id_reg": det.get("chip_id_reg", 0),
                "chip_id_value": det.get("chip_id_value", 0),
            }

        entries.append(entry)

    return entries


def main():
    parser = argparse.ArgumentParser(
        description="Generate ThistleOS app catalog"
    )
    parser.add_argument("artifact_dir",
                        help="Directory containing built artifacts")
    parser.add_argument("--base-url", required=True,
                        help="Base URL for download links")
    parser.add_argument("--output", "-o", default="catalog.json",
                        help="Output catalog path")
    parser.add_argument("--merge",
                        help="Existing catalog to merge with "
                             "(preserves ratings/downloads)")

    args = parser.parse_args()

    entries = scan_artifacts(args.artifact_dir, args.base_url)

    # Merge with existing catalog to preserve ratings/downloads
    if args.merge and Path(args.merge).exists():
        existing = json.loads(Path(args.merge).read_text())
        existing_map = {e["id"]: e for e in existing.get("entries", [])}
        for entry in entries:
            if entry["id"] in existing_map:
                old = existing_map[entry["id"]]
                entry["rating"] = old.get("rating", 0)
                entry["rating_count"] = old.get("rating_count", 0)
                entry["downloads"] = old.get("downloads", 0)

    # Set updated date
    today = date.today().isoformat()
    for entry in entries:
        entry["updated"] = today

    catalog = {
        "version": 1,
        "generated": today,
        "entries": entries,
    }

    Path(args.output).write_text(json.dumps(catalog, indent=2) + "\n")
    print(f"Catalog written: {args.output} ({len(entries)} entries)")


if __name__ == "__main__":
    main()
