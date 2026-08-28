#!/usr/bin/env python3
"""Build and validate deterministic Thistle Application Packages (.tap)."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import sys
import tempfile
import zipfile
from pathlib import Path, PurePosixPath


MAX_ARCHIVE_SIZE = 2 * 1024 * 1024
MAX_EXTRACTED_SIZE = 4 * 1024 * 1024
MAX_ENTRIES = 64
MAX_JSON_SIZE = 64 * 1024
MAX_ELF_SIZE = 1024 * 1024
FIXED_TIMESTAMP = (1980, 1, 1, 0, 0, 0)
ARCHES = {"esp32", "esp32s2", "esp32s3", "esp32c3", "esp32c6", "host"}
ID_RE = re.compile(r"^[a-z0-9]+(?:[.-][a-z0-9]+)+$")
VERSION_RE = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)


class TapError(ValueError):
    """A package violates the TAP v1 contract."""


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def validate_path(name: str) -> None:
    if not name or len(name) > 160 or "\\" in name or name.startswith("/"):
        raise TapError(f"invalid archive path: {name!r}")
    path = PurePosixPath(name)
    if name.endswith("/") or any(part in ("", ".", "..") for part in path.parts):
        raise TapError(f"invalid archive path: {name!r}")
    if path.as_posix() != name:
        raise TapError(f"non-canonical archive path: {name!r}")


def require_string(value: object, field: str, maximum: int) -> str:
    if not isinstance(value, str) or not value or len(value) > maximum:
        raise TapError(f"{field} must be a non-empty string of at most {maximum} chars")
    return value


def validate_documents(package: object, metadata: object) -> dict:
    if not isinstance(package, dict) or not isinstance(metadata, dict):
        raise TapError("package.json and metadata.json must contain JSON objects")
    if package.get("schema") != "thistle.app.package/v1":
        raise TapError("unsupported package schema")
    if package.get("type") != "app":
        raise TapError("package type must be app")
    app_id = require_string(package.get("id"), "id", 63)
    if not ID_RE.fullmatch(app_id):
        raise TapError("invalid app id")
    version = require_string(package.get("version"), "version", 31)
    if not VERSION_RE.fullmatch(version):
        raise TapError("invalid semantic version")
    sequence = package.get("release_sequence")
    if not isinstance(sequence, int) or isinstance(sequence, bool) or sequence < 1:
        raise TapError("release_sequence must be a positive integer")
    if package.get("arch") not in ARCHES:
        raise TapError("unsupported package architecture")
    require_string(package.get("name"), "name", 63)
    require_string(package.get("author"), "author", 63)
    require_string(package.get("description"), "description", 512)
    min_os = require_string(package.get("min_os"), "min_os", 31)
    if not VERSION_RE.fullmatch(min_os):
        raise TapError("invalid min_os version")
    permissions = package.get("permissions")
    if not isinstance(permissions, list) or any(
        not isinstance(item, str) or not item for item in permissions
    ) or len(set(permissions)) != len(permissions):
        raise TapError("permissions must be a unique string array")
    if not isinstance(package.get("background"), bool):
        raise TapError("background must be boolean")
    memory = package.get("min_memory_kb")
    if not isinstance(memory, int) or isinstance(memory, bool) or not 0 <= memory <= 8192:
        raise TapError("min_memory_kb must be an integer from 0 to 8192")

    if metadata.get("schema") != "thistle.app.metadata/v1":
        raise TapError("unsupported metadata schema")
    if metadata.get("id") != app_id:
        raise TapError("metadata id does not match package id")
    releases = metadata.get("releases")
    if not isinstance(releases, list) or not releases or not isinstance(releases[0], dict):
        raise TapError("metadata must contain at least one release")
    latest = releases[0]
    if latest.get("version") != version or latest.get("release_sequence") != sequence:
        raise TapError("first metadata release does not match package release")

    files = package.get("files")
    if not isinstance(files, list) or not 2 <= len(files) <= 60:
        raise TapError("files must contain 2 to 60 entries")
    declared: dict[str, dict] = {}
    for item in files:
        if not isinstance(item, dict) or set(item) != {"path", "sha256", "size_bytes"}:
            raise TapError("each files item must contain path, sha256, and size_bytes")
        path = item.get("path")
        if not isinstance(path, str):
            raise TapError("declared file path must be a string")
        validate_path(path)
        if path in declared:
            raise TapError(f"duplicate declared file: {path}")
        digest = item.get("sha256")
        size = item.get("size_bytes")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise TapError(f"invalid SHA-256 for {path}")
        if not isinstance(size, int) or isinstance(size, bool) or not 1 <= size <= MAX_ELF_SIZE:
            raise TapError(f"invalid size for {path}")
        declared[path] = item

    for field in ("entry", "signature"):
        path = package.get(field)
        if not isinstance(path, str) or path not in declared:
            raise TapError(f"{field} must name a declared file")
    if package["entry"] != "app.app.elf" or package["signature"] != "app.app.elf.sig":
        raise TapError("v1 entry and signature names must use the canonical layout")
    icon = package.get("icon")
    if icon is not None and icon not in declared:
        raise TapError("icon must name a declared file")
    return declared


def _zip_info(name: str) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, FIXED_TIMESTAMP)
    info.compress_type = zipfile.ZIP_STORED
    info.create_system = 3
    info.external_attr = (stat.S_IFREG | 0o644) << 16
    info.flag_bits |= 0x800
    return info


def build_package(
    manifest_path: Path,
    metadata_path: Path,
    elf_path: Path,
    signature_path: Path,
    license_path: Path,
    output_path: Path,
    arch: str | None = None,
    assets: tuple[Path, ...] = (),
) -> Path:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    elf = elf_path.read_bytes()
    signature = signature_path.read_bytes()
    license_data = license_path.read_bytes()
    if not elf or len(elf) > MAX_ELF_SIZE:
        raise TapError("ELF must be between 1 byte and 1 MiB")
    if len(signature) != 64:
        raise TapError("ELF signature must be exactly 64 bytes")
    if arch is not None:
        if arch not in ARCHES:
            raise TapError(f"unsupported architecture: {arch}")
        manifest["arch"] = arch

    payloads: dict[str, bytes] = {
        "app.app.elf": elf,
        "app.app.elf.sig": signature,
    }
    for asset in assets:
        name = f"assets/{asset.name}"
        validate_path(name)
        if name in payloads:
            raise TapError(f"duplicate package asset: {name}")
        payloads[name] = asset.read_bytes()

    package = {
        "schema": "thistle.app.package/v1",
        "type": "app",
        "id": manifest.get("id"),
        "name": manifest.get("name"),
        "version": manifest.get("version"),
        "release_sequence": manifest.get("release_sequence"),
        "author": manifest.get("author"),
        "description": manifest.get("description"),
        "min_os": manifest.get("min_os"),
        "arch": manifest.get("arch"),
        "entry": "app.app.elf",
        "signature": "app.app.elf.sig",
        "permissions": manifest.get("permissions", []),
        "background": manifest.get("background", False),
        "min_memory_kb": manifest.get("min_memory_kb", 0),
        "files": [
            {"path": name, "sha256": sha256(data), "size_bytes": len(data)}
            for name, data in sorted(payloads.items())
        ],
    }
    if "compatible_boards" in manifest:
        package["compatible_boards"] = manifest["compatible_boards"]
    package_bytes = canonical_json(package)
    metadata_bytes = canonical_json(metadata)
    if len(package_bytes) > MAX_JSON_SIZE or len(metadata_bytes) > MAX_JSON_SIZE:
        raise TapError("package JSON exceeds the 64 KiB limit")
    validate_documents(package, metadata)

    entries = {
        "LICENSE": license_data,
        "metadata.json": metadata_bytes,
        "package.json": package_bytes,
        **payloads,
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output_path, "w", allowZip64=False) as archive:
        for name in sorted(entries):
            archive.writestr(_zip_info(name), entries[name])
    if output_path.stat().st_size > MAX_ARCHIVE_SIZE:
        output_path.unlink(missing_ok=True)
        raise TapError("archive exceeds the 2 MiB limit")
    validate_package(output_path)
    return output_path


def validate_package(package_path: Path) -> dict:
    if package_path.suffix != ".tap":
        raise TapError("application packages must use the .tap extension")
    if package_path.stat().st_size > MAX_ARCHIVE_SIZE:
        raise TapError("archive exceeds the 2 MiB limit")
    with zipfile.ZipFile(package_path, "r") as archive:
        infos = archive.infolist()
        names = [info.filename for info in infos]
        if not 1 <= len(infos) <= MAX_ENTRIES:
            raise TapError("archive entry count is outside v1 limits")
        if names != sorted(names):
            raise TapError("archive entries are not sorted")
        if len(names) != len(set(names)):
            raise TapError("archive contains duplicate entries")
        extracted = 0
        for info in infos:
            validate_path(info.filename)
            if info.compress_type != zipfile.ZIP_STORED:
                raise TapError(f"compressed entry is forbidden: {info.filename}")
            if info.flag_bits & 0x09:
                raise TapError(f"encrypted or data-descriptor entry: {info.filename}")
            if info.extract_version >= 45 or b"\x01\x00" in info.extra:
                raise TapError(f"ZIP64 entry is forbidden: {info.filename}")
            mode = (info.external_attr >> 16) & 0xFFFF
            if mode and not stat.S_ISREG(mode):
                raise TapError(f"non-regular archive entry: {info.filename}")
            extracted += info.file_size
        if extracted > MAX_EXTRACTED_SIZE:
            raise TapError("extracted package exceeds the 4 MiB limit")
        required = {"package.json", "metadata.json", "app.app.elf", "app.app.elf.sig", "LICENSE"}
        missing = required - set(names)
        if missing:
            raise TapError(f"archive is missing required entries: {', '.join(sorted(missing))}")
        if archive.getinfo("package.json").file_size > MAX_JSON_SIZE or archive.getinfo("metadata.json").file_size > MAX_JSON_SIZE:
            raise TapError("package JSON exceeds the 64 KiB limit")
        try:
            package = json.loads(archive.read("package.json"))
            metadata = json.loads(archive.read("metadata.json"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise TapError(f"invalid package JSON: {error}") from error
        declared = validate_documents(package, metadata)
        payload_names = set(names) - {"package.json", "metadata.json", "LICENSE", "NOTICE"}
        if payload_names != set(declared):
            raise TapError("archive payload does not exactly match declared files")
        for name, item in declared.items():
            data = archive.read(name)
            if len(data) != item["size_bytes"] or sha256(data) != item["sha256"]:
                raise TapError(f"payload integrity mismatch: {name}")
        if len(archive.read(package["signature"])) != 64:
            raise TapError("ELF signature must be exactly 64 bytes")
        return package


def verify_signatures(package_path: Path, public_key_path: Path) -> None:
    try:
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError as error:
        raise TapError("cryptography is required for signature verification") from error
    public = public_key_path.read_bytes()
    if len(public) != 32:
        raise TapError("public key must be exactly 32 raw bytes")
    verifier = Ed25519PublicKey.from_public_bytes(public)
    tap_signature_path = Path(f"{package_path}.sig")
    if not tap_signature_path.is_file():
        raise TapError(f"package signature not found: {tap_signature_path}")
    try:
        verifier.verify(tap_signature_path.read_bytes(), package_path.read_bytes())
        with zipfile.ZipFile(package_path, "r") as archive:
            verifier.verify(archive.read("app.app.elf.sig"), archive.read("app.app.elf"))
    except InvalidSignature as error:
        raise TapError("package or ELF signature is invalid") from error


def install_package(
    package_path: Path,
    apps_root: Path,
    public_key_path: Path | None = None,
    allow_downgrade: bool = False,
) -> Path:
    """Install a verified generation using an atomic active.json switch."""
    package = validate_package(package_path)
    if public_key_path is not None:
        verify_signatures(package_path, public_key_path)
    app_dir = apps_root / package["id"]
    generations = app_dir / "generations"
    sequence = package["release_sequence"]
    destination = generations / str(sequence)
    active_path = app_dir / "active.json"
    if active_path.is_file():
        try:
            active = json.loads(active_path.read_text(encoding="utf-8"))
            active_sequence = active["release_sequence"]
        except (KeyError, TypeError, json.JSONDecodeError) as error:
            raise TapError(f"invalid existing activation record: {active_path}") from error
        if not allow_downgrade and sequence < active_sequence:
            raise TapError(
                f"refusing downgrade from sequence {active_sequence} to {sequence}"
            )
    if destination.exists() or destination.is_symlink():
        raise TapError(f"generation already exists: {destination}")

    generations.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=f".{sequence}.stage-", dir=generations))
    activated = False
    try:
        with zipfile.ZipFile(package_path, "r") as archive:
            for info in archive.infolist():
                target = stage / info.filename
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(info, "r") as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output, 64 * 1024)
        os.replace(stage, destination)
        activated = True
        receipt = {
            "schema": "thistle.app.receipt/v1",
            "id": package["id"],
            "version": package["version"],
            "release_sequence": sequence,
            "arch": package["arch"],
            "package_sha256": sha256(package_path.read_bytes()),
            "granted_permissions": package["permissions"],
        }
        receipt_temp = app_dir / ".receipt.json.tmp"
        receipt_temp.write_bytes(canonical_json(receipt))
        os.replace(receipt_temp, app_dir / "receipt.json")
        active_temp = app_dir / ".active.json.tmp"
        active_temp.write_bytes(canonical_json({
            "version": package["version"],
            "release_sequence": sequence,
        }))
        os.replace(active_temp, active_path)
        return destination
    except Exception:
        if stage.exists():
            shutil.rmtree(stage)
        if activated and destination.exists():
            shutil.rmtree(destination)
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    build = subparsers.add_parser("build", help="build a deterministic .tap archive")
    build.add_argument("--manifest", type=Path, required=True)
    build.add_argument("--metadata", type=Path, required=True)
    build.add_argument("--elf", type=Path, required=True)
    build.add_argument("--elf-signature", type=Path, required=True)
    build.add_argument("--license", type=Path, required=True)
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--arch", choices=sorted(ARCHES))
    build.add_argument("--asset", type=Path, action="append", default=[])
    verify = subparsers.add_parser("verify", help="validate a .tap archive")
    verify.add_argument("package", type=Path)
    verify.add_argument("--pubkey", type=Path)
    install = subparsers.add_parser(
        "install", help="transactionally install a .tap generation"
    )
    install.add_argument("package", type=Path)
    install.add_argument("--apps-root", type=Path, required=True)
    install.add_argument("--pubkey", type=Path)
    install.add_argument("--allow-downgrade", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "build":
            output = build_package(
                args.manifest, args.metadata, args.elf, args.elf_signature,
                args.license, args.output, args.arch, tuple(args.asset),
            )
            print(f"Built: {output}")
            print(f"SHA-256: {sha256(output.read_bytes())}")
        elif args.command == "verify":
            package = validate_package(args.package)
            if args.pubkey:
                verify_signatures(args.package, args.pubkey)
            print(
                f"PASS: {package['id']} {package['version']} "
                f"({package['arch']}, sequence {package['release_sequence']})"
            )
        else:
            destination = install_package(
                args.package, args.apps_root, args.pubkey, args.allow_downgrade
            )
            print(f"Installed: {destination}")
        return 0
    except (OSError, TapError, zipfile.BadZipFile, json.JSONDecodeError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
