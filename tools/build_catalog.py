#!/usr/bin/env python3
"""Generate catalog.json from built ThistleOS app/driver artifacts."""

import json
import hashlib
import argparse
from datetime import date
from pathlib import Path

from tap_package import TapError, validate_package


def scan_artifacts(artifact_dir: str, base_url: str,
                   require_signatures: bool = False,
                   publisher_key_id: str = "") -> list:
    """Scan directory for .app.elf/.drv.elf + manifest.json pairs."""
    entries = []
    artifact_path = Path(artifact_dir).resolve()

    manifest_files = sorted(
        set(list(artifact_path.glob("**/manifest.json"))
            + list(artifact_path.glob("**/*.manifest.json")))
    )

    for manifest_file in manifest_files:
        manifest = json.loads(manifest_file.read_text())
        entry_type = manifest.get("type", "app")
        permissions = manifest.get("permissions", [])
        if isinstance(permissions, list):
            permissions = ",".join(permissions)

        if entry_type == "app":
            tap_files = sorted(manifest_file.parent.glob("*.tap"))
            if len(tap_files) != 1:
                raise FileNotFoundError(
                    f"Expected exactly one TAP package beside {manifest_file}"
                )
            tap_path = tap_files[0]
            try:
                tap_manifest = validate_package(tap_path)
            except TapError as exc:
                raise ValueError(f"Invalid TAP package {tap_path}: {exc}") from exc
            for field in ("id", "version", "arch", "release_sequence"):
                if tap_manifest.get(field) != manifest.get(field):
                    raise ValueError(f"TAP {field} mismatch for {manifest_file}")
            tap_sig = Path(f"{tap_path}.sig")
            if require_signatures and not tap_sig.is_file():
                raise FileNotFoundError(f"TAP signature not found for {tap_path}")
            tap_rel = tap_path.resolve().relative_to(artifact_path)
            package_url = f"{base_url.rstrip('/')}/{tap_rel.as_posix()}"
            tap_data = tap_path.read_bytes()
            entries.append({
                "id": tap_manifest["id"],
                "type": "app",
                "name": tap_manifest["name"],
                "version": tap_manifest["version"],
                "release_sequence": tap_manifest["release_sequence"],
                "author": tap_manifest["author"],
                "description": tap_manifest["description"],
                "category": manifest.get("category", "tools"),
                "permissions": permissions,
                "min_os_version": tap_manifest["min_os"],
                "arch": tap_manifest["arch"],
                "compatible_boards": tap_manifest.get("compatible_boards", []),
                "package_url": package_url,
                "package_sig_url": f"{package_url}.sig" if tap_sig.is_file() else "",
                "package_sha256": hashlib.sha256(tap_data).hexdigest(),
                "package_size_bytes": len(tap_data),
                "publisher_key_id": publisher_key_id,
                "is_signed": tap_sig.is_file(),
                "updated": "",
                "changelog": manifest.get("changelog", ""),
                "rating": 0,
                "rating_count": 0,
                "downloads": 0,
            })
            continue

        # Find corresponding ELF
        elf_name = manifest.get("entry", "")
        if not elf_name:
            raise ValueError(f"Manifest has no entry: {manifest_file}")
        elf_path = (manifest_file.parent / elf_name).resolve()
        try:
            rel_path = elf_path.relative_to(artifact_path)
        except ValueError as exc:
            raise ValueError(
                f"Manifest entry escapes artifact directory: {manifest_file}"
            ) from exc
        if not elf_path.exists():
            raise FileNotFoundError(
                f"ELF not found for {manifest_file}: {elf_name}"
            )

        expected_suffix = ".drv.elf" if entry_type == "driver" else ".wm.elf"
        if not elf_name.endswith(expected_suffix):
            raise ValueError(
                f"{entry_type} manifest has invalid entry suffix: {manifest_file}"
            )

        # Compute SHA-256
        elf_data = elf_path.read_bytes()
        sha256 = hashlib.sha256(elf_data).hexdigest()
        size = len(elf_data)

        # Check for signature
        sig_path = elf_path.with_suffix(elf_path.suffix + ".sig")
        has_sig = sig_path.exists()
        if require_signatures and not has_sig:
            raise FileNotFoundError(f"Signature not found for {elf_path}")

        # Build relative URL path
        elf_url = f"{base_url.rstrip('/')}/{rel_path.as_posix()}"
        sig_url = f"{elf_url}.sig" if has_sig else ""

        # Map manifest fields to catalog entry
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


def merge_catalog_entries(entries: list, existing_entries: list) -> list:
    """Carry usage metadata forward without retaining legacy app releases.

    Applications are TAP-only. An app absent from the current artifact set must
    be republished as a TAP before it can reappear in the catalog. Other
    component types retain the existing incremental-publication behaviour.
    """
    existing_map = {
        entry.get("id", ""): entry
        for entry in existing_entries
        if entry.get("id")
    }
    merged = []
    rebuilt_ids = set()
    for entry in entries:
        package_id = entry.get("id", "")
        rebuilt_ids.add(package_id)
        old = existing_map.get(package_id, {})
        entry["rating"] = old.get("rating", entry.get("rating", 0))
        entry["rating_count"] = old.get(
            "rating_count", entry.get("rating_count", 0)
        )
        entry["downloads"] = old.get("downloads", entry.get("downloads", 0))
        merged.append(entry)

    merged.extend(
        entry for entry in existing_entries
        if entry.get("id") not in rebuilt_ids and entry.get("type") != "app"
    )
    return merged


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
    parser.add_argument("--require-signatures", action="store_true",
                        help="Fail if any catalogued ELF has no signature")
    parser.add_argument("--publisher-key-id", default="",
                        help="Publisher key identifier recorded for TAP packages")

    args = parser.parse_args()

    entries = scan_artifacts(
        args.artifact_dir, args.base_url,
        require_signatures=args.require_signatures,
        publisher_key_id=args.publisher_key_id,
    )

    # Set the publication date only on packages rebuilt in this run.
    today = date.today().isoformat()
    for entry in entries:
        entry["updated"] = today

    # Preserve packages not rebuilt here and retain their usage metadata.
    if args.merge and Path(args.merge).exists():
        existing = json.loads(Path(args.merge).read_text())
        entries = merge_catalog_entries(entries, existing.get("entries", []))

    catalog = {
        "version": 1,
        "generated": today,
        "entries": entries,
    }

    Path(args.output).write_text(json.dumps(catalog, indent=2) + "\n")
    print(f"Catalog written: {args.output} ({len(entries)} entries)")


if __name__ == "__main__":
    main()
