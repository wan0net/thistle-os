#!/usr/bin/env python3
"""ThistleOS Ed25519 signing tool for apps, drivers, and firmware."""

import argparse
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)
from cryptography.exceptions import InvalidSignature


def cmd_keygen(args):
    private_key = Ed25519PrivateKey.generate()
    private_bytes = private_key.private_bytes_raw()
    public_bytes = private_key.public_key().public_bytes_raw()

    with open("private.key", "wb") as f:
        f.write(private_bytes)

    with open("public.key", "wb") as f:
        f.write(public_bytes)

    print(f"Private key written to: private.key")
    print(f"Public key written to:  public.key")
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

    args = parser.parse_args()

    if args.command == "keygen":
        cmd_keygen(args)
    elif args.command == "sign":
        cmd_sign(args)
    elif args.command == "verify":
        cmd_verify(args)


if __name__ == "__main__":
    main()
