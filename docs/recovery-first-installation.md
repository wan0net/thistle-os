# Recovery-first installation and packaging

Status: proposed

This document defines the installation experience in which Recovery OS is the
only image a user initially flashes. Recovery then obtains network access,
downloads and boots a signed compatible kernel, and hands provisioning to that
kernel. The kernel builds the external-volume driver and application system. This
document is intended to remain editable while the packaging and boot flow are
implemented.

The full discussion history and Rust driver reuse audit are recorded in
[`architecture-conversation-record.md`](architecture-conversation-record.md)
and [`rust-driver-inventory.md`](rust-driver-inventory.md).

## Desired user experience

1. The user flashes one recovery installer image over USB.
2. Recovery starts immediately and writes useful progress to the serial console.
3. If WiFi credentials were provisioned during flashing, or previously saved,
   Recovery attempts that network for a bounded period.
4. If no credentials exist or the connection fails, Recovery creates an
   encrypted `ThistleOS-Recovery-XXXX` access point and prints its generated
   password and `http://192.168.4.1` on serial.
5. The web UI lets the user connect Recovery to WiFi, review the
   architecture-specific kernel installation plan, and install.
6. Recovery downloads and verifies only the compatible kernel and the minimum
   bootstrap state required to hand off safely.
7. Recovery boots the new kernel. The kernel mounts or prepares the configured
   system volume, downloads and verifies the selected drivers, window manager,
   and apps, activates that package generation, and runs it. The initial backend
   is SD; future boards may use NVMe or another storage device.
8. A failed or unconfirmed kernel returns to a known-good recovery path without
   requiring another USB flash.

The public installer must not contain a WiFi password, signing private key, or
device-specific secret.

## Current implementation

Recovery already provides a strong base:

- Rust recovery firmware with serial logging.
- An encrypted AP with a random per-boot SSID suffix and password.
- A web UI for WiFi connection, board selection, dry-run planning, download,
  progress, and reboot.
- Architecture filtering, signed artifact manifests, SHA-256 verification,
  anti-rollback state, and journaled bundle installation.
- SD-card installation fallback.

The current code does not yet produce the recovery-first product:

- The root and recovery projects use different partition tables, even though
  every image installed on the device must agree on the deployed layout. The
  current standalone Recovery build is actually using ESP-IDF's default 1 MB
  factory layout rather than `recovery/partitions.csv`.
- The ordinary merged build flashes the kernel into `factory`; it does not
  contain Recovery OS.
- Recovery is currently described as `ota_0` and only manages a kernel in
  `ota_1`, leaving no conventional kernel A/B pair.
- Recovery starts its AP unconditionally and does not first attempt provisioned
  or saved WiFi credentials.
- The WiFi configuration submitted through the recovery web UI is not explicitly
  persisted in a Recovery-owned NVS namespace.
- HTTP captive-portal probe routes exist, but a DNS redirector is still needed
  for a complete captive-portal experience.
- CI does not currently publish a merged recovery installer.
- Recovery currently downloads and transactionally installs the complete board
  bundle. That responsibility must move to the kernel; Recovery should stop
  after installing and selecting a verified compatible kernel.
- Recovery currently exposes board selection. The architecture-specific flow
  removes that step from Recovery; board identification begins in the kernel.

## Partition layout

### Recommended layout

Use the ESP-IDF `factory` application slot for immutable Recovery OS and reserve
both OTA slots for kernels:

| Name | Offset | Size | Purpose |
| --- | ---: | ---: | --- |
| `nvs` | `0x009000` | `0x006000` | Recovery network settings and anti-rollback state |
| `otadata` | `0x00F000` | `0x002000` | Selected kernel slot and rollback state |
| `phy_init` | `0x011000` | `0x001000` | Radio calibration data |
| `factory` | `0x020000` | `0x200000` | Recovery OS; never updated by normal OTA |
| `ota_0` | `0x220000` | `0x580000` | Kernel A |
| `ota_1` | `0x7A0000` | `0x580000` | Kernel B |
| `bootstrap` + unallocated | `0xD20000` | `0x2D0000` | Provisional ceiling, not a filesystem allocation |
| `coredump` | `0xFF0000` | `0x010000` | Crash diagnostics |

This candidate fits exactly in 16 MB. It gives each kernel 5.5 MB and leaves a
2.8125 MB region whose final split is deliberately undecided. The bootstrap
state should receive only its measured requirement; the remainder can enlarge
the OTA slots or remain reserved. ESP-ROM serial download mode remains the
last-resort reflashing path and does not depend on any application partition.

Calling Recovery “immutable” means no normal OTA or catalog operation may write
the `factory` partition. Serial flashing can still replace it deliberately.

A fresh release build of the current Recovery source produces a 1,300,288-byte
application image. It uses about 62% of the proposed 2 MB Recovery partition,
leaving 796,864 bytes for growth. The current kernel is about 2.2 MB, leaving
more than 3 MB of space in either proposed kernel slot.

The internal-storage size remains provisional until CI builds every required
driver and window-manager ELF and measures a complete minimum boot generation
plus its rollback copy. The checked-in `sdcard_layout` is currently only about
128 KiB, but it contains mostly manifests and configuration rather than the final
ELF payloads, so that number is not sufficient sizing evidence.

### Alternative retained for comparison

Keeping a baseline kernel in `factory`, Recovery in `ota_0`, and the current
kernel in `ota_1` preserves ESP-IDF's conventional factory-reset destination but
leaves only one updateable kernel slot. It is no longer recommended because ROM
serial boot is an acceptable last resort and kernel A/B rollback provides more
day-to-day value.

## Installer artifacts

Release one generic installer per chip architecture, not per board:

```text
thistle-recovery-installer-esp32s3-vX.Y.Z.bin
thistle-recovery-installer-esp32s3-vX.Y.Z.sha256
thistle-recovery-installer-esp32s3-vX.Y.Z.manifest
thistle-recovery-installer-esp32s3-vX.Y.Z.manifest.sig
```

The merged installer contains:

- bootloader at `0x0000`;
- unified partition table at `0x8000`;
- empty/default OTA metadata at `0xF000`;
- Recovery OS in `factory` at `0x20000`;
- optionally, an initial NVS image at `0x9000` generated locally for that one
  device.

Both kernel slots and SPIFFS remain empty in the generic installer. Empty OTA
metadata makes the stock ESP-IDF bootloader select `factory`, so Recovery starts
without custom first-boot metadata. Recovery fills one kernel slot from the
signed kernel catalog. After boot, that kernel provisions the system volume.

### WiFi-provisioned installer

The same release binary should support an optional local personalization step:

```text
thistle-flash recovery-installer.bin
  -> prompts for serial port
  -> optionally prompts for WiFi SSID and password without echo
  -> optionally stores a board hint for the kernel without Recovery interpreting it
  -> generates an NVS image locally
  -> flashes the generic segments plus that initial NVS image
  -> opens the serial log
```

Credentials must not be accepted as normal command-line arguments because they
can be retained in shell history and process listings. A future browser flasher
can collect the same values locally and write the generated NVS partition over
Web Serial.

The NVS initializer is for first installation only. Recovery and kernel updates
must never replace the full NVS partition because it also holds the monotonic
anti-rollback floor. Recovery should own a distinct `thistle_net` namespace and
store only validated SSID, password, and an enabled flag there.

## Recovery network state machine

```text
boot
  -> start serial logging
  -> load provisioned/saved Recovery WiFi profile
     -> credentials present: try STA for 15 seconds
        -> connected: start web server on the LAN address
        -> failed: start encrypted AP + web server
     -> credentials absent: start encrypted AP + web server
  -> if an established STA connection remains down for 30 seconds,
     restore the recovery AP without rebooting
```

When AP fallback is active, Recovery should run AP+STA mode. The portal can then
accept new credentials without disconnecting the user's browser before the
result is known. Credentials are persisted only after the station obtains an IP.

The AP uses WPA2, a random per-boot password, and a random SSID suffix. The AP
password and URL are printed on the physical serial console. STA passwords,
catalog authorization values, session tokens, and signing material are never
logged.

## Serial logging contract

Serial output is a supported recovery interface, not incidental debug output.
The default is 115200 baud and logs at least:

- recovery version, build ID, reset reason, chip, flash size, and MAC suffix;
- partition table validation and the state/version of both kernel slots;
- network credential source (`provisioned`, `saved`, or `none`), without secrets;
- station connection attempts, timeout/reason, assigned IP, and portal URL;
- fallback AP SSID, password, address, and captive-portal status;
- detected architecture;
- catalog fetch, signed-manifest verification, artifact name, byte progress, and
  final digest result;
- target kernel slot, activation decision, reboot, rollback, and fatal errors.

Messages should have stable stage identifiers so a flashing tool can display
progress without scraping prose, for example `RECOVERY stage=wifi_sta result=ok`.

## Recovery-to-kernel handoff and system provisioning

Recovery detects only the chip architecture and selects one signed kernel for
that architecture, such as `esp32s3` or `esp32c3`. It does not select a board,
driver, window manager, or app.

After boot, the kernel resolves board identity, in priority order:

1. a previously confirmed signed board profile;
2. a safe hardware fingerprint that uniquely identifies one catalog profile;
3. a valid board hint provisioned during flashing;
4. explicit selection through a kernel-hosted provisioning interface or serial.

The board hint is not a trust decision. Signed manifest compatibility remains
authoritative.

The kernel already contains an I2C fingerprint fallback that probes known device
addresses over several candidate SDA/SCL pin pairs when `board.json` is missing.
This is useful evidence, not yet a safe identity authority. The current matcher:

- uniquely identifies T-Deck Pro from its keyboard and BHI260AP response on
  SDA 13 / SCL 14;
- identifies the shared T-Deck/T-Deck Plus keyboard and touch fingerprint as
  ordinary T-Deck, so it cannot currently distinguish T-Deck Plus;
- embeds and writes a complete unsigned `board.json` rather than resolving a
  signed catalog profile;
- assumes RAK3312 when no known I2C device responds.

The production flow should retain only safe, read-only fingerprint evidence,
resolve that evidence to signed catalog candidates, and persist a confirmed
signed profile. Ambiguous or empty results must remain unknown and request user
selection; they must never silently select a board. Once confirmed, normal boots
use the saved profile and do not probe pins again unless rediscovery is requested.

Board family and fitted capabilities are separate discovery results. In
particular, T-Deck Pro-family units may contain the audio codec or the A7682E 4G
modem, while the MAX contains both. A match for the common T-Deck Pro hardware
therefore selects only the base profile. Additional safe capability probes add
`audio` and/or `cellular` to the hardware inventory, and the package resolver
installs the corresponding drivers. It must not assume audio merely because the
base board is T-Deck Pro, as the current embedded fallback and
`tdeck-pro.json` do.

The signed catalog should express this as a base profile plus matched component
identities rather than duplicating a complete board profile for every
combination. High-level capabilities are derived from those identities; they are
not sufficient to select a driver because the audio-only Pro and MAX use
different audio devices:

```text
family:     tdeck-pro
components: [audio/pcm512a]                  # audio variant
            [cellular/a7682e]                # 4G variant
            [audio/es8311, cellular/a7682e]  # MAX
```

Passive I2C evidence should be preferred. A modem probe that requires changing
power pins or sending UART commands is a later, board-scoped probe and must only
run after the common family is known. If an option cannot be distinguished
safely, provisioning asks the user and records the selection as a hint until a
driver successfully confirms the component.

Recovery's installation plan contains only:

- one kernel compatible with the chip architecture;
- its signed manifest and signature;
- the minimum shared network/bootstrap state required for handoff.

After boot, the kernel owns the system plan. It uses the selected board profile
to resolve, download, verify, and transactionally activate on the system volume:

- detailed signed driver ELFs;
- the selected signed window manager ELF;
- all signed application ELFs, including launcher and settings;
- themes and other selected system assets.

The kernel must be self-bootstrapping before those packages exist. Its image must
provide serial diagnostics, internal configuration access, WiFi, bootstrap
storage mounting,
HTTPS, signature verification, and the package-generation engine. These may be
separate internal modules invoked through HAL boundaries, but they ship as part
of the kernel distribution rather than being fetched from the system volume.

The kernel verifies the complete system-volume generation before changing the
active generation pointer. A bad download cannot replace the last confirmed
system generation.

## Storage and package management

### Decision: minimum bootstrap only, external system volume

Internal flash is not a miniature application installation. It contains only:

- Recovery OS;
- Kernel A and Kernel B;
- the selected signed board profile;
- only the code and state strictly required to reach WiFi and mount the configured
  system volume;
- the minimum trust, network, and boot-selection metadata required to do that
  safely.

All applications live on the system volume, including launcher, settings,
package manager, and other system-facing apps. Detailed peripheral drivers,
window managers, themes, media, documents, maps, models, and other user content
also live there. On current T-Deck-family hardware the system volume is SD. A
future board may provide NVMe, eMMC, or another block-storage backend without
changing the package model.

The kernel itself supplies the lifecycle, HAL registry, driver loader, package
resolution, networking, verification, and recovery coordination needed before
any app starts. It can therefore boot headlessly, report status over serial, read
its bootstrap state, and attempt to make the external-volume system available.

The current firmware does not yet enforce this boundary. Fourteen apps are still
compiled into the kernel firmware, so they consume space in both OTA slots. All
of them must become signed `.app.elf` packages installed on the system volume
unless a capability is proven to be a kernel service rather than an application.

### Bootstrap dependency

The kernel cannot load its first storage or network driver from a system volume
it has not mounted. The architecture-specific kernel distribution must therefore
be self-bootstrapping. It supplies an architecture-level WiFi/network path and a
generic bootstrap storage path that accepts runtime board configuration. The
kernel core may still reach these through HAL boundaries; the bootstrap
implementations ship with the kernel distribution rather than being selected by
Recovery.

On first provisioning, the kernel obtains the selected signed board profile over
the network, persists only the minimum bus/power/storage configuration required
for the next boot, and uses it to configure the generic bootstrap storage path.
All more detailed drivers remain system-volume packages.

On kernel boot:

1. Initialize the bootstrap facilities shipped with the kernel distribution.
2. Attempt to mount the configured system volume and validate its active package
   generation.
3. If valid, load detailed drivers, the selected window manager, and apps from
   system volume.
4. If the system volume is absent, corrupt, or incomplete, use saved network
   configuration to locate and download the missing signed generation when
   writable external storage is available. A component required only for the
   current rescue session may be
   streamed into RAM after verification rather than persisted internally.
5. If neither the system volume nor network is available, remain headless with serial status and
   offer an explicit reboot to Recovery.

WiFi credentials are shared bootstrap state needed by both Recovery and kernels.
They may ultimately live in NVS, a very small filesystem, or another internal
configuration store. That choice follows the data and security requirements; it
does not justify a general-purpose internal application filesystem. Passwords
must not be stored as unprotected plaintext in a public or signed configuration.

The partition table should reserve bytes, but it should not hard-code the policy
for every file. A small storage/package manager should present logical roles over
internal flash and the configured external storage backend:

- **bootstrap**: only the board/network/storage facts and code required to reach
  the configured system volume or network. These remain available internally.
- **installed**: all apps, detailed drivers, window managers, themes, and content.
  These belong on the system volume and are unavailable when it is absent.
- **staging**: downloads and the next generation. This belongs on the system
  volume. Temporary
  rescue downloads should use RAM where possible and must not silently create a
  persistent internal package cache.
- **cache**: catalogs, thumbnails, and redownloadable content. This is always
  evictable.

Each installed generation should be an immutable manifest referring to
content-addressed artifacts by digest. Activation changes one small generation
pointer only after all artifacts verify. Garbage collection removes unreferenced
objects only after the new kernel and filesystem generation are confirmed
healthy. The manager must reserve enough free space for its journal and rollback
rather than filling the filesystem to its nominal capacity.

The current implementation hard-codes package discovery, downloads, and
transaction backups under `/sdcard`. That is a backend-specific detail that must
move behind the system-volume abstraction. The no-volume path must not pretend to
install a complete application system internally. It can update the kernel and
minimum bootstrap state, report over serial, and wait for usable external
storage. Every package plan must be rejected before downloading if its live,
staging, rollback, and reserve bytes do not fit on the selected system volume.

This storage manager is separate from a RAM memory manager. Internal flash and
the configured system volume hold persistent objects; SRAM and PSRAM hold running tasks,
surfaces, relocated ELF segments, stacks, and buffers. They need coordinated
budgets and observability, but they have different allocators and failure modes.

## Boot and rollback rules

- A blank device with empty `otadata` boots `factory` Recovery.
- Recovery installs the first kernel into `ota_0`, marks it pending, and selects
  it for the next boot.
- The kernel marks itself valid only after its required health checks complete.
- Kernel updates are written to the inactive kernel slot and never overwrite the
  running or last-confirmed kernel.
- If a pending kernel fails to confirm, ESP-IDF rollback returns to the previous
  valid kernel. Recovery must retain the previous filesystem generation until
  the new kernel is confirmed healthy.
- Recovery is entered on first install, when the kernel explicitly requests it,
  or through a separately defined physical recovery gesture. The physical
  gesture may require bootloader work and must be tested per board.

## Delivery phases

### Phase 1: unify boot and packaging

- Adopt one partition table shared by Recovery and kernel builds.
- Build Recovery for the `factory` offset and enforce its 2.5 MB size limit.
- Update Recovery to inspect and install both kernel OTA slots.
- Add a reproducible merged-installer packaging command.
- Publish checksums, signed manifests, build provenance, and exact flash command.

Acceptance criteria:

- A blank T-Deck Pro boots Recovery after flashing one merged image at `0x0`.
- Recovery and kernel builds both validate against the same partition table.
- CI fails if any artifact exceeds its partition.
- No kernel, optional app, or WiFi secret is required in the generic installer.

### Phase 2: network bootstrap and serial contract

- Add Recovery-owned NVS credential persistence.
- Try provisioned/saved STA credentials with bounded timeouts.
- Add encrypted AP fallback and reconnect fallback.
- Add DNS interception for actual captive-portal discovery.
- Emit stable serial stage events and retain readable log messages.

Acceptance criteria:

- Valid provisioned credentials reach the web UI on the LAN without starting an
  AP.
- Missing, invalid, and unreachable credentials all produce a usable recovery AP.
- A failed station attempt never blocks the web UI indefinitely.
- Serial logs are sufficient to diagnose every network and install failure and
  never contain the STA password.

### Phase 3: signed kernel handoff and system-volume provisioning

- Restrict Recovery to installing and selecting the signed kernel.
- Move driver, WM, app, and asset resolution into the kernel package manager.
- Install and activate the complete signed generation on the system volume.
- Exercise power loss at every journal state and download boundary.
- Verify new-kernel confirmation, kernel rollback, and filesystem rollback.

Acceptance criteria:

- Wrong-architecture, wrong-board, unsigned, expired, replayed, truncated, and
  hash-mismatched artifacts are rejected before activation.
- Loss of power at any tested boundary returns to Recovery or a confirmed kernel.
- A failed kernel can neither strand the board nor delete the last healthy bundle.

### Phase 4: user-facing flashers and release publication

- Provide a small desktop/CLI flasher and a browser Web Serial flasher.
- Support local, non-echoed WiFi personalization and optional board hints.
- Publish generic recovery installers for every supported architecture.
- Add release documentation for first install, recovery entry, offline install,
  serial diagnosis, and checksum verification.

## Open decisions

1. Do we need a physical gesture to return to Recovery, or are a kernel “reboot
   to Recovery” action plus ESP-ROM serial boot sufficient?
2. Should a successful STA connection suppress the AP entirely, or keep a timed
   maintenance AP available?
3. Should the first flasher implementation be a Python CLI, a browser Web Serial
   page, or both in sequence?
4. Should Recovery accept multiple saved networks, or keep a single deliberately
   simple profile?
5. Which optional apps, if any, are selected by default after the minimum system
   becomes bootable?
