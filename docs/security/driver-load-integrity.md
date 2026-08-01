# Driver load integrity

Standalone drivers execute native code in kernel context. The loader therefore
reads each driver ELF exactly once into a size-bounded memory snapshot, verifies
the detached Ed25519 signature over that snapshot, and relocates bytes from the
same snapshot. It never reopens the driver pathname between verification and
relocation.

Replacing the file or its directory entry after the initial read cannot change
the code passed to the ELF loader. Empty files, files larger than 512 KiB,
malformed signatures, unavailable signing state, and signature mismatches fail
before PSRAM allocation or relocation. Debug builds retain the existing
explicit unsigned-driver development exception; release builds require a valid
signature.

This change does not alter the on-disk driver or signature format and requires
no migration.
