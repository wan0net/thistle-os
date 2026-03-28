#!/usr/bin/env python3
"""ThistleOS ELF signing tool — signs .app.elf and .drv.elf with Ed25519."""

import sys
import hashlib
import argparse
from pathlib import Path

# Use PyNaCl (libsodium binding) for Ed25519
# pip install pynacl
from nacl.signing import SigningKey, VerifyKey


def sign_file(elf_path: str, private_key_hex: str) -> tuple:
    """Sign an ELF file and write the .sig file. Returns (sig_path, sha256, size)."""
    elf_data = Path(elf_path).read_bytes()
    sk = SigningKey(bytes.fromhex(private_key_hex))
    signed = sk.sign(elf_data)
    sig = signed.signature  # 64 bytes

    sig_path = elf_path + ".sig"
    Path(sig_path).write_bytes(sig)

    # Also compute SHA-256
    sha256 = hashlib.sha256(elf_data).hexdigest()

    return sig_path, sha256, len(elf_data)


def verify_file(elf_path: str, public_key_hex: str) -> bool:
    """Verify an ELF signature."""
    elf_data = Path(elf_path).read_bytes()
    sig_data = Path(elf_path + ".sig").read_bytes()
    vk = VerifyKey(bytes.fromhex(public_key_hex))
    try:
        vk.verify(elf_data, sig_data)
        return True
    except Exception:
        return False


def keygen():
    """Generate a new Ed25519 keypair."""
    sk = SigningKey.generate()
    print(f"Private key (hex): {sk.encode().hex()}")
    print(f"Public key (hex):  {sk.verify_key.encode().hex()}")


def main():
    parser = argparse.ArgumentParser(description="ThistleOS ELF signing tool")
    sub = parser.add_subparsers(dest="command")

    # sign
    p_sign = sub.add_parser("sign", help="Sign an ELF file")
    p_sign.add_argument("elf", help="Path to .app.elf or .drv.elf")
    p_sign.add_argument("--key", required=True,
                        help="Private key hex (64 chars) or @file")

    # verify
    p_verify = sub.add_parser("verify", help="Verify an ELF signature")
    p_verify.add_argument("elf", help="Path to .app.elf or .drv.elf")
    p_verify.add_argument("--pubkey", required=True,
                          help="Public key hex (64 chars)")

    # keygen
    sub.add_parser("keygen", help="Generate a new Ed25519 keypair")

    args = parser.parse_args()

    if args.command == "keygen":
        keygen()
    elif args.command == "sign":
        key = args.key.strip()
        if key.startswith("@"):
            key = Path(key[1:]).read_text().strip()
        if len(key) != 64:
            print(f"Error: private key must be 64 hex characters, got {len(key)}", file=sys.stderr)
            sys.exit(1)
        sig_path, sha256, size = sign_file(args.elf, key)
        print(f"Signed: {sig_path}")
        print(f"SHA-256: {sha256}")
        print(f"Size: {size} bytes")
    elif args.command == "verify":
        if verify_file(args.elf, args.pubkey):
            print("Signature valid")
            sys.exit(0)
        else:
            print("Signature INVALID")
            sys.exit(1)
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
