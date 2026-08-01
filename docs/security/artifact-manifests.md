# Signed artifact manifests

Recovery installs only artifacts described by a canonical
`THISTLE-ARTIFACT-MANIFEST-V1` manifest signed by the Recovery Ed25519 root.
The signature binds the payload digest and size together with its type, ID,
version, monotonic security version, architecture, compatible boards,
destination, and download URL.

Catalog entries are discovery pointers only. Each installable entry must expose
`manifest_url` and `manifest_sig_url`; legacy `sha256`, `url`, and `sig_url`
fields are not an authorization boundary and cannot make an artifact
installable. Recovery rejects legacy-only entries.

Generate a manifest and signature with:

```console
python3 tools/sign.py manifest path/to/payload \
  --key private.key \
  --type driver \
  --id qmi8658c \
  --version 1.0.0 \
  --security-version 1 \
  --arch esp32s3 \
  --compatible-board tdeck-pro \
  --url https://example.invalid/qmi8658c.drv.elf
```

This creates `payload.manifest` and `payload.manifest.sig`. Board, driver, and
window-manager manifests are retained beside the active artifact. The selected
board profile also retains manifest evidence beside `config/board.json`.
Firmware evidence is retained as `config/firmware.manifest` and
`config/firmware.manifest.sig` for boot-health and anti-rollback processing.

## Migration

1. Publish the payload, canonical manifest, and manifest signature.
2. Add both manifest URLs to the catalog entry.
3. Keep the legacy fields only for older clients during the transition.
4. Confirm the signed ID, destination, chip, board list, version, and security
   version before publishing.
5. Never decrease or reuse a firmware security version for different bytes.

Unsigned board profiles and raw payload signatures are intentionally rejected
by the new Recovery installer.
