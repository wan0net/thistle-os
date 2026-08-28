# ThistleOS application package format

Status: v1 tooling and simulator reference implementation complete; firmware
package-manager integration remains pending.

The reference tooling is `tools/tap_package.py`. It builds byte-reproducible
archives, validates the bounded ZIP profile and metadata relationships, checks
both signature layers when given a publisher key, and transactionally installs
simulator generations. Application catalog delivery is TAP-only: raw app ELF
URLs are neither catalogued nor retained in published application artifacts.
Drivers, window managers, and firmware retain their separate signed formats.

```text
python3 tools/tap_package.py build --manifest manifest.json \
  --metadata metadata.json --elf notes.app.elf \
  --elf-signature notes.app.elf.sig --license LICENSE \
  --arch host --output com.thistle.notes-1.0.0-host.tap

python3 tools/tap_package.py verify com.thistle.notes-1.0.0-host.tap \
  --pubkey publisher.public.key

python3 tools/tap_package.py install com.thistle.notes-1.0.0-host.tap \
  --apps-root simulator/sdcard/apps --pubkey publisher.public.key
```

## Decision

A distributable ThistleOS application is one immutable Thistle Application
Package (TAP): a ZIP archive with the extension `.tap`. One archive contains one
application release for one CPU architecture. It carries the loadable
`.app.elf`, the ELF signature, a machine-readable package manifest, descriptive
store metadata, and any runtime assets required by that release.

The package is the download and installation unit. The ELF remains the boot-time
execution unit.

This separation gives the installer one object to download and activate while
retaining the kernel's existing rule that every executable is verified before
it is loaded.

## Naming

Published packages use:

```text
<app-id>-<version>-<arch>.tap
```

For example:

```text
com.thistle.notes-1.2.0-esp32s3.tap
```

Supported architecture slugs are `esp32`, `esp32s2`, `esp32s3`, `esp32c3`,
`esp32c6`, and `host`. `host` packages are simulator-only and must never be
offered to physical devices.

## Archive layout

Every archive has this root layout, without a containing directory:

```text
package.json                         required
metadata.json                        required
app.app.elf                          required
app.app.elf.sig                      required
assets/icon-1bit.bin                 optional
assets/icon-rgb565.bin               optional
assets/...                           optional runtime assets
LICENSE                              required
NOTICE                               optional
```

`package.json` is the authoritative identity, compatibility, permission, and
payload manifest for the installed release. `metadata.json` contains store
presentation data and an informational release history snapshot.

Screenshots and other large promotional media should normally remain in the
catalog host rather than the package. Assets included in the archive must be
needed offline or at runtime.

## ZIP profile

Version 1 deliberately uses a small, deterministic subset of ZIP:

- entries use the `store` method with no compression;
- entry names are UTF-8, relative, use `/`, and are sorted bytewise;
- timestamps are fixed to `1980-01-01T00:00:00Z`;
- encryption, data descriptors, ZIP64, symlinks, hard links, and duplicate
  names are forbidden;
- absolute paths, backslashes, empty components, `.` and `..` are forbidden;
- maximum archive size is 2 MiB;
- maximum extracted size is 4 MiB;
- maximum entry count is 64;
- `package.json` and `metadata.json` are each limited to 64 KiB;
- `app.app.elf` is limited to 1 MiB, matching the current ELF loader limit.

The uncompressed profile enables streaming extraction with bounded memory on
ESP32-class devices. A later format version may add a specific compression
method after memory and power measurements.

## `package.json`

The normative schema is
[`schemas/thistle-app-package-v1.schema.json`](../schemas/thistle-app-package-v1.schema.json).

Example:

```json
{
  "schema": "thistle.app.package/v1",
  "type": "app",
  "id": "com.thistle.notes",
  "name": "Notes",
  "version": "1.2.0",
  "release_sequence": 12,
  "author": "ThistleOS",
  "description": "Write and organise plain-text notes.",
  "min_os": "0.3.0",
  "arch": "esp32s3",
  "compatible_boards": [],
  "entry": "app.app.elf",
  "signature": "app.app.elf.sig",
  "permissions": ["storage.read", "storage.write"],
  "background": false,
  "min_memory_kb": 64,
  "icon": "assets/icon-1bit.bin",
  "files": [
    {
      "path": "app.app.elf",
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "size_bytes": 48320
    },
    {
      "path": "app.app.elf.sig",
      "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      "size_bytes": 64
    },
    {
      "path": "assets/icon-1bit.bin",
      "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
      "size_bytes": 128
    }
  ]
}
```

Every extractable file other than `package.json`, `metadata.json`, `LICENSE`,
and `NOTICE` must appear exactly once in `files`. The installer must confirm
that `entry`, `signature`, and `icon` refer to declared files.

`release_sequence` is a positive, monotonically increasing integer scoped to
the app ID. Normal installs cannot replace a greater sequence with a lower one.
Developer mode may expose a clearly labelled downgrade action.

Permissions in the package are the maximum permissions the release may
request. Installation presents this list to the user. The installed grant can
be equal or narrower, never broader.

## `metadata.json`

The normative schema is
[`schemas/thistle-app-metadata-v1.schema.json`](../schemas/thistle-app-metadata-v1.schema.json).

Example:

```json
{
  "schema": "thistle.app.metadata/v1",
  "id": "com.thistle.notes",
  "category": "productivity",
  "summary": "Fast, local notes for colour and e-paper devices.",
  "description": "Create and edit portable plain-text notes stored locally.",
  "license": "BSD-3-Clause",
  "homepage": "https://github.com/wan0net/thistle-os",
  "source": "https://github.com/wan0net/thistle-os/tree/main/apps/notes",
  "releases": [
    {
      "version": "1.2.0",
      "release_sequence": 12,
      "released": "2026-08-29",
      "changes": ["Added folders", "Improved e-paper keyboard focus"]
    },
    {
      "version": "1.1.0",
      "release_sequence": 11,
      "released": "2026-07-10",
      "changes": ["Added search"]
    }
  ]
}
```

The first release entry must describe the package being installed and match its
`id`, `version`, and `release_sequence`. Older entries are an offline snapshot
for display only. They do not grant authority to install older code, and the
catalog remains the source for finding downloadable releases.

Ratings and download counts are catalog-service data and must not be embedded
as package claims.

## Signing and publication

There are two verification layers:

1. The publisher builds the ELF and signs `app.app.elf`. The signature remains
   installed beside the ELF and is checked by the kernel whenever it loads the
   app.
2. The publisher creates the deterministic `.tap` archive and signs the
   complete archive bytes, producing `.tap.sig`. Because `package.json`
   is inside the signed archive, this binds the executable hashes, app ID,
   version, release sequence, architecture, compatibility, and permissions.

Application packages are installed by the kernel package manager and use its
application-publisher keyring. They are not Recovery artifacts. The existing
`THISTLE-ARTIFACT-MANIFEST-V1` contract currently supports firmware, boards,
drivers, and window managers but has no application destination role; v1 must
not pretend that contract authorizes app packages without a separately reviewed
extension.

The catalog is discovery metadata, not an authorization boundary. A v1 catalog
entry for a package must provide:

```json
{
  "id": "com.thistle.notes",
  "type": "app",
  "name": "Notes",
  "version": "1.2.0",
  "release_sequence": 12,
  "arch": "esp32s3",
  "package_url": "https://example.invalid/apps/com.thistle.notes-1.2.0-esp32s3.tap",
  "package_sig_url": "https://example.invalid/apps/com.thistle.notes-1.2.0-esp32s3.tap.sig",
  "package_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
  "package_size_bytes": 61234,
  "publisher_key_id": "thistle-official-apps-2026"
}
```

The catalog may duplicate presentation fields from `metadata.json` so the store
can render its list before downloading a package. The installer must reject any
identity, version, architecture, or sequence disagreement between catalog,
signed artifact manifest, `package.json`, and `metadata.json`.

## Installation transaction

The installer must:

1. download the package and package signature to a staging area;
2. identify an accepted application publisher key and verify the package
   signature, catalog size, and catalog SHA-256;
3. inspect the ZIP directory and enforce all profile limits before extraction;
4. parse and validate both JSON documents;
5. verify identity and compatibility across all signed and contained metadata;
6. stream-extract only declared files and verify size, CRC, and SHA-256;
7. verify `app.app.elf.sig` with an accepted application publisher key;
8. stage the complete app generation without modifying the active generation;
9. atomically activate it and retain the previous generation for rollback;
10. write an install receipt containing source catalog, package digest,
    publisher key ID, granted permissions, version, sequence, and install time;
11. rescan app registrations and expose the app to the launcher, or explicitly
    request a restart if live registration is not yet supported.

Any failure removes the staged generation and leaves the active app untouched.

## Logical installed layout

The package manager owns a storage-neutral logical app root:

```text
/system/apps/com.thistle.notes/
  active -> generations/12
  generations/12/
    package.json
    metadata.json
    app.app.elf
    app.app.elf.sig
    assets/...
  receipt.json
```

The initial implementation may map `/system/apps` to SD or SPIFFS, but app code
and catalog metadata must not depend on the physical backing path. Until the ELF
loader understands generation directories, the package manager may create a
compatibility projection into `/sdcard/apps`; the projection is derived state,
not the installed source of truth.

## Catalog history versus package history

The catalog may retain multiple downloadable releases for an app. A package
contains only one executable release, plus an informational history snapshot.
This avoids bundling old executables into every new download and keeps rollback
an explicit package-manager operation using a previously verified generation.

## Simulator acceptance criteria

- Build the same logical app release into deterministic `host` and `esp32s3`
  packages; repeated builds are byte-identical.
- Reject traversal names, duplicate names, links, ZIP64, compression, undeclared
  files, oversized entries, hash mismatches, invalid signatures, and metadata
  disagreement.
- Browse the local catalog and install a `host` package without internet access.
- Verify Available, Installing, Installed, Update, and Failed states in both WMs.
- Restart the simulator and rediscover the installed app from its receipt and
  active generation.
- Interrupt installation at every transaction boundary and retain the previous
  working generation.
- Remove an app without deleting its private user data unless the user makes a
  separate explicit data-deletion choice.

## Open questions for v1 implementation

- Whether publisher keys are accepted only through the official catalog trust
  root or through a user-managed keyring as well.
- Whether the first implementation activates apps live or requires a simulator
  restart after install.
- How many previous verified generations are retained by default.
- Whether app data migrations need a declarative hook before the first external
  Notes release.
