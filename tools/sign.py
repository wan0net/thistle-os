#!/usr/bin/env python3
"""ThistleOS Ed25519 signing tool for apps, drivers, and firmware."""

import argparse
import hashlib
import os
from pathlib import Path
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)
from cryptography.exceptions import InvalidSignature


PRIVATE_KEY_PATH = Path("private.key")
PUBLIC_KEY_PATH = Path("public.key")
MANIFEST_HEADER = "THISTLE-ARTIFACT-MANIFEST-V1"


def _require_absent(path):
    """Reject files, directories, and symlinks before generating key material."""
    try:
        path.lstat()
    except FileNotFoundError:
        return
    raise FileExistsError(f"refusing to overwrite existing path: {path}")


def _write_new_file(path, data, mode):
    """Create a new regular file without following or replacing existing paths."""
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    flags |= getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags, mode)
    try:
        # os.open applies the process umask. Force the intended final mode on
        # the already-open descriptor before writing any sensitive bytes.
        os.fchmod(descriptor, mode)
        with os.fdopen(descriptor, "wb", closefd=True) as output:
            descriptor = -1
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def cmd_keygen(args):
    _require_absent(PRIVATE_KEY_PATH)
    _require_absent(PUBLIC_KEY_PATH)

    private_key = Ed25519PrivateKey.generate()
    private_bytes = private_key.private_bytes_raw()
    public_bytes = private_key.public_key().public_bytes_raw()

    _write_new_file(PRIVATE_KEY_PATH, private_bytes, 0o600)
    _write_new_file(PUBLIC_KEY_PATH, public_bytes, 0o644)

    print("Private key written to: private.key")
    print("Public key written to:  public.key")
    print(f"Public key (hex): {public_bytes.hex()}")


def cmd_sign(args):
    seed = open(args.key, "rb").read()
    if len(seed) != 32:
        print(f"ERROR: private key must be 32 bytes, got {len(seed)}", file=sys.stderr)
        sys.exit(1)

    private_key = Ed25519PrivateKey.from_private_bytes(seed)
    data = open(args.file, "rb").read()
    signature = private_key.sign(data)

    sig_path = args.file + ".sig"
    with open(sig_path, "wb") as f:
        f.write(signature)

    print(f"Signed: {args.file}")
    print(f"Signature written to: {sig_path}")


def cmd_verify(args):
    pub_bytes = open(args.pubkey, "rb").read()
    if len(pub_bytes) != 32:
        print(f"ERROR: public key must be 32 bytes, got {len(pub_bytes)}", file=sys.stderr)
        sys.exit(1)

    public_key = Ed25519PublicKey.from_public_bytes(pub_bytes)
    data = open(args.file, "rb").read()
    signature = open(args.file + ".sig", "rb").read()

    try:
        public_key.verify(signature, data)
        print("PASS")
    except InvalidSignature:
        print("FAIL")
        sys.exit(1)


def manifest_destination(artifact_type, artifact_id):
    destinations = {
        "firmware": "ota_1",
        "board": f"config/boards/{artifact_id}.json",
        "driver": f"drivers/{artifact_id}.drv.elf",
        "wm": f"wm/{artifact_id}.wm.elf",
    }
    return destinations[artifact_type]


def canonical_manifest(*, artifact_type, artifact_id, version, security_version,
                       arch, compatible_boards, url, payload):
    boards = sorted(set(compatible_boards))
    if not boards:
        raise ValueError("at least one compatible board is required")
    fields = [artifact_id, version, arch, url, *boards]
    if any(not value or "\n" in value or "\r" in value for value in fields):
        raise ValueError("manifest fields must be non-empty single-line values")
    digest = hashlib.sha256(payload).hexdigest()
    return (
        f"{MANIFEST_HEADER}\n"
        f"type={artifact_type}\n"
        f"id={artifact_id}\n"
        f"version={version}\n"
        f"security_version={security_version}\n"
        f"arch={arch}\n"
        f"compatible_boards={','.join(boards)}\n"
        f"destination={manifest_destination(artifact_type, artifact_id)}\n"
        f"sha256={digest}\n"
        f"size={len(payload)}\n"
        f"url={url}\n"
    ).encode("utf-8")


def cmd_manifest(args):
    seed = Path(args.key).read_bytes()
    if len(seed) != 32:
        raise ValueError(f"private key must be 32 bytes, got {len(seed)}")
    payload_path = Path(args.file)
    canonical = canonical_manifest(
        artifact_type=args.type,
        artifact_id=args.id,
        version=args.version,
        security_version=args.security_version,
        arch=args.arch,
        compatible_boards=args.compatible_board,
        url=args.url,
        payload=payload_path.read_bytes(),
    )
    signature = Ed25519PrivateKey.from_private_bytes(seed).sign(canonical)
    manifest_path = Path(f"{args.file}.manifest")
    signature_path = Path(f"{args.file}.manifest.sig")
    _write_new_file(manifest_path, canonical, 0o644)
    _write_new_file(signature_path, signature, 0o644)
    print(f"Manifest written to: {manifest_path}")
    print(f"Manifest signature written to: {signature_path}")


def main():
    parser = argparse.ArgumentParser(
        description="ThistleOS Ed25519 signing tool"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    # keygen
    subparsers.add_parser("keygen", help="Generate a new Ed25519 keypair")

    # sign
    sign_parser = subparsers.add_parser("sign", help="Sign a file")
    sign_parser.add_argument("file", help="File to sign")
    sign_parser.add_argument("--key", required=True, metavar="private.key",
                             help="Path to private key (raw 32-byte seed)")

    # verify
    verify_parser = subparsers.add_parser("verify", help="Verify a file signature")
    verify_parser.add_argument("file", help="File to verify")
    verify_parser.add_argument("--pubkey", required=True, metavar="public.key",
                               help="Path to public key (raw 32 bytes)")

    manifest_parser = subparsers.add_parser(
        "manifest", help="Create and sign a canonical artifact manifest"
    )
    manifest_parser.add_argument("file", help="Artifact payload")
    manifest_parser.add_argument("--key", required=True, help="Raw 32-byte private key")
    manifest_parser.add_argument(
        "--type", required=True, choices=["firmware", "board", "driver", "wm"]
    )
    manifest_parser.add_argument("--id", required=True)
    manifest_parser.add_argument("--version", required=True)
    manifest_parser.add_argument("--security-version", required=True, type=int)
    manifest_parser.add_argument(
        "--arch", required=True,
        choices=["esp32", "esp32s2", "esp32s3", "esp32c3", "esp32c6"],
    )
    manifest_parser.add_argument(
        "--compatible-board", required=True, action="append",
        help="Signed compatible board ID; repeat for multiple boards",
    )
    manifest_parser.add_argument("--url", required=True)

    args = parser.parse_args()

    try:
        if args.command == "keygen":
            cmd_keygen(args)
        elif args.command == "sign":
            cmd_sign(args)
        elif args.command == "verify":
            cmd_verify(args)
        elif args.command == "manifest":
            cmd_manifest(args)
    except (OSError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
