# Password KDF and envelope migration

ThistleOS uses a versioned Argon2id profile for new password-derived keys:

- Argon2id v1.3
- 64 KiB memory
- 6 iterations
- 1 lane
- 16-byte cryptographically random salt
- 32-byte derived key

Password inputs are bounded before the KDF reads them (128 characters in the
vault UI, 256 bytes for messaging, and 1,024 bytes in the generic crypto ABI).
Random-generation failures abort vault creation or saving, and any partially
filled random output is zeroed before the error is returned.

The 64 KiB profile is the common baseline calibrated for the lowest-memory
supported ESP32-family targets, including boards without PSRAM. The parameters
are stored in each vault envelope so a future release can introduce a stronger
profile without guessing how an existing file was derived. Unsupported or
malformed parameters fail closed before key derivation or decryption.

## Vault files

New vaults use the authenticated `THV2` envelope:

```
magic(4) | version(1) | kdf(1) | reserved(2)
memory_kib(4) | time_cost(4) | lanes(4)
salt(16) | iv(16) | ciphertext(n) | hmac_sha256(32)
```

All numeric fields are little-endian. The HMAC covers the entire header and
ciphertext, including the KDF parameters and salt.

Legacy vaults have no magic and use the old 10,000-iteration PBKDF2 layout.
They are accepted only as a migration source. After successful authentication
and decryption, the vault generates a new random salt and rewrites the data as
v2. The rewrite uses a temporary file and recoverable backup; an interrupted or
failed rewrite retains the original vault and does not report a successful
unlock.

## Messaging

New encrypted messages use wire version 2:

```
version(1) | salt(16) | nonce(16) | ciphertext(n) | hmac_sha256(32)
```

Each channel receives a fresh random salt. The salt is included in the
authenticated envelope so both peers derive the same Argon2id key. Version 1
messages remain decryptable with the legacy key during migration, but all new
encryption emits version 2. The old unsalted standalone derivation API now
fails closed; callers must provide a 16-byte salt to the v2 API.

Changing a shared passphrase re-establishes the channel with a new random salt.
Destroying or replacing a channel zeroes its v2 key, salt, password material,
and legacy migration key.
