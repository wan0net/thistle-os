# T-Watch Ultra and Apple ANCS Build Plan

Status: software profile corrected; hardware verification remains pending
Target: the owner's physical LilyGo T-Watch Ultra
Primary milestone: a flashable, recoverable watch build with working display,
touch, power management, and iPhone notifications through Apple ANCS
Production milestone: a Recovery-first, signed, component-aware ThistleOS bundle

## 1. Outcome

Deliver the work in two explicit stages:

1. **Daily-driver bring-up build** — a board-specific ESP32-S3 development image
   that boots reliably, renders on the CO5300 AMOLED, accepts CST9217 touch,
   reports battery state, pairs securely with an iPhone, and displays live ANCS
   notifications. This is the first useful hardware milestone.
2. **Supported ThistleOS build** — the same proven components packaged through
   the Recovery-first architecture as signed board, driver, window-manager, and
   system-app artifacts, with rollback and reproducible release evidence.

The first stage intentionally does not wait for NFC, microphone, GNSS, IMU, SD,
or every radio variant. Those remain follow-on hardware enablement and must not
be reported as working merely because the firmware builds.

## 2. Evidence and existing work

The repository already has a useful foundation:

- GitHub issue #120 tracks production T-Watch Ultra support, with issues
  #121-#127 covering profile/discovery, power, display/touch, RTC/haptics,
  audio, NFC, and radio/peripheral bring-up.
- `sdcard_layout/config/boards/twatch-ultra.json` now records the correct
  component tuple and requires explicit radio selection. It remains a bring-up
  profile until the signed artifact and physical board are verified together.
- Rust prototypes exist for CO5300 and CST9217, but they are kernel-internal,
  are not the signed standalone `.drv.elf` files named by the profile, and are
  not hardware-verified.
- `ble_manager.rs` currently implements a secure NimBLE GATT **server** for the
  Nordic UART Service. The ESP32-S3 configuration enables the peripheral role
  but disables central and observer roles. ANCS instead requires the watch to
  act as a GATT client/notification consumer.
- `notification.rs` provides an in-memory notification model, but it is not yet
  wired into boot or the watch UI.
- The earlier T-Watch profile's incorrect CST9217, RTC, and audio identities
  have been removed. Catalog validation now rejects their reintroduction.
- The current built-in driver fallback only covers the T-Deck Pro display,
  keyboard, and touch path. A board JSON that names nonexistent T-Watch driver
  ELFs will therefore skip the essential watch hardware rather than provide a
  working fallback.
- The earlier Recovery-first design remains partially implemented: the root
  and Recovery partition layouts still need one authoritative definition, and
  a complete flashable Recovery bundle has not yet been proven.

Official LilyGo material identifies a 16 MB ESP32-S3 watch with 8 MB PSRAM and
radio variants including SX1262, SX1280, CC1101, LR1121, and SI4432. The exact
radio fitted to the owner's unit must be recorded before a radio driver is
selected.

Apple's ANCS contract has three characteristics: Notification Source, Control
Point, and Data Source. Access requires authorization. Notifications can arrive
immediately after subscription, attribute responses can span multiple MTU
fragments, ANCS may appear or disappear during a connection, and notification
UIDs are valid only for the current session. These are design requirements, not
edge cases to defer.

## 3. Scope of the first working build

### Required

- ESP32-S3 boot with correct 16 MB flash and 8 MB PSRAM configuration.
- Safe XL9555 and AXP2101 initialization.
- CO5300 410 x 502 display initialization and rendering.
- CST9217 touch at the hardware-verified address and orientation.
- Battery voltage/charge indication and a safe shutdown path.
- USB serial logging and ROM download-mode recovery.
- BLE central/GATT-client support alongside any retained peripheral role.
- Secure iPhone pairing and durable bond storage.
- ANCS add, modify, and remove events.
- Bounded retrieval of app name, title, subtitle, message, date, and the action
  labels that are actually supplied by iOS.
- A notification list/detail UI with touch navigation.
- Reconnection after radio loss, watch reboot, and iPhone reboot.
- A reproducible flashable image plus manifest, checksums, source revision,
  configuration, and hardware test log.

### Deferred from the first build

- NFC and NDEF.
- PDM microphone capture and speaker playback.
- GNSS, IMU, SD-backed `/system`, haptics, and RTC alarm wake, unless one is
  needed to resolve a blocker in the essential path.
- Radio operation. The fitted radio is inventoried now but enabled after the
  core watch and ANCS milestone.
- Support claims for radio SKUs other than the physical unit tested.
- iOS notification actions. Read-only notification mirroring lands first;
  positive/negative actions follow only after labels and user confirmation are
  correctly represented.

## 4. Architecture decisions

### 4.1 Keep the first image board-specific but make the exception explicit

For the first hardware milestone, compile or provision the minimum essential
drivers with a corrected, explicitly selected development profile. Do not use
ambiguous automatic probing and do not silently fall back to another board.

This is a bring-up exception, not the final distribution model. Once verified,
move the drivers into signed `.drv.elf` packages and select them through the
signed board/component catalog described by the Recovery-first design.

### 4.2 Put ANCS in a privileged system service boundary

ANCS is a phone-integration service, not a board driver. Split it into:

- a BLE transport that owns scanning, connecting, GATT discovery, encryption,
  bonding, subscription, and reconnect policy;
- a pure Rust ANCS protocol/state-machine module with no ESP-IDF dependency;
- a bounded bridge into the kernel notification model;
- a notification UI owned by the default window manager or a signed privileged
  system app.

The final design should expose BLE through the appropriate HAL/service boundary
rather than add more direct NimBLE assumptions to otherwise hardware-independent
kernel logic. For the first build, extending the existing NimBLE manager is
acceptable only if the ANCS parser remains transport-independent and the
hardware-specific calls stay isolated behind a narrow adapter.

### 4.3 Keep iPhone content bounded and ephemeral by default

- Cap every received attribute before allocation and rendering.
- Correctly reassemble fragmented Data Source responses with a total-size cap,
  timeout, and one well-defined outstanding-request policy.
- Validate UTF-8 and replace invalid sequences safely.
- Clear ANCS UIDs and session mappings on disconnect because Apple defines them
  as session-scoped.
- Do not persist notification bodies by default.
- Redact notification contents from normal logs; log lifecycle and sizes, not
  private text.
- Never guess what a positive or negative action means. Use the action labels
  supplied by iOS, and require an explicit touch confirmation before sending
  an action.

## 5. Work plan

### Phase 0 — Preserve and identify the physical unit

1. Record the exact product label, PCB/revision marking, radio option, flash
   identity, MAC, and detected serial port.
2. Run LilyGo's matching factory test firmware before modifying the device and
   retain display, touch, PMU, battery, and radio results.
3. Read and archive the full 16 MB factory flash, calculate its SHA-256, and
   prove it can at least be parsed into the expected partitions.
4. Capture a non-destructive I2C scan and compare it with the expected tuple:
   CST9217 `0x1A`, XL9555 `0x20`, BHI260AP `0x28`, AXP2101 `0x34`, PCF85063A
   `0x51`, and DRV2605 `0x5A`.
5. Do not infer the radio from the board name. Use the order/SKU, physical
   marking, signed provisioning selection, or a safe driver-specific identity
   check.

Gate: the original firmware is recoverable, the watch revision is recorded,
and the essential I2C evidence is consistent enough to proceed.

### Phase 1 — Establish a clean, reproducible build target

1. Work from current `main` in a clean worktree or branch so the existing local
   changes are not mixed into the port.
2. Add a named T-Watch Ultra build preset using isolated SDK configuration and
   build directories. Reuse the ESP32-S3 defaults, then explicitly verify:
   16 MB flash, 8 MB PSRAM mode, USB CDC on boot, NimBLE, BLE central/observer,
   GATT client, security, NVS bond persistence, and required connection limits.
3. Correct the board profile and represent the actual radio as a separate
   component/SKU selection.
4. Add a deterministic development payload step so the selected board profile
   is actually present on first boot. A checked-in JSON file alone is not a
   flashable filesystem.
5. Emit distinctly named outputs: bootloader, partition table, application,
   filesystem/system payload, merged development image, and checksum manifest.

Gate: a clean checkout produces byte-identifiable artifacts and the merged
image contains the selected profile rather than relying on unsafe fallback.

### Phase 2 — Bring up power before the display

1. Implement/adapt XL9555 through the shared I2C service.
2. Implement/adapt AXP2101 with conservative battery charging and explicit rail
   ownership/ref-counting.
3. Enable only the rails needed for display and touch; keep radio, audio, NFC,
   GNSS, and other unused rails off.
4. Make initialization failure-safe and idempotent, with serial logs for every
   rail and expander transition.
5. Verify the power button, battery voltage, charging state, reboot, and ROM
   download mode before adding the display workload.

Gate: repeated cold boots and resets retain recovery access, report plausible
battery state, and leave unused peripherals powered down.

### Phase 3 — Bring up AMOLED and touch

1. Choose one CO5300 implementation path by comparing the local Rust prototype
   with the pinned `decaday/display-driver` panel implementation. Avoid two
   diverging drivers.
2. Implement the ESP32-S3 QSPI transport, reset/power order, clipping, RGB565
   transfers, rotation, sleep/wake, and bounded DMA/PSRAM buffer ownership.
3. Fix CST9217 to the verified `0x1A` address; route reset through XL9555 and
   implement down, move, and up events with bounds checks.
4. Add fake-bus tests for initialization sequences, clipped areas, malformed
   touch reports, coordinate transforms, timeouts, and unload/reload.
5. On hardware, test full-screen color bars, partial regions, all four edges,
   repeated sleep/wake, and 30 minutes of continuous UI updates.

Gate: the Thistle launcher is readable, touch matches display orientation over
the whole panel, and ten consecutive cold boots plus sleep/wake cycles pass.

### Phase 4 — Add the ANCS transport and parser

1. Extend the BLE abstraction with central and observer state while preserving
   authenticated handling for the existing server role.
2. Implement pairing UX on the watch: show the passkey or confirmation prompt,
   display connected/authorized state, allow forgetting the iPhone, and never
   print long-lived keys.
3. Persist bonds in NVS and handle repeat pairing, changed iPhone identity, bond
   exhaustion, and explicit user-initiated bond deletion.
4. Discover GATT and ANCS dynamically after encryption. Subscribe to GATT
   Service Changed because Apple does not guarantee ANCS is always published.
5. Subscribe to Data Source before Notification Source so the parser is ready
   before events can arrive.
6. Implement the pure Rust ANCS parser/state machine:
   - add/modify/remove event parsing;
   - bounded attribute requests and response reassembly across MTU fragments;
   - app-name caching bounded by count and bytes;
   - request timeout, malformed length, unexpected UID, disconnect, and service
     disappearance handling;
   - session reset and reconnect backoff.
7. Add host tests using captured/synthetic byte streams, including every split
   point across a fragmented response and hostile lengths.

Gate: a real iPhone pairs, authorizes ANCS, reconnects without re-pairing, and
the watch receives add/modify/remove events without leaking notification text
to serial output.

### Phase 5 — Connect ANCS to Thistle notifications and UI

1. Give the kernel a single live notification-manager instance at boot and a
   safe event API for system services.
2. Map ANCS session UID to a Thistle notification ID. Update on modified events
   and dismiss locally on removed events; never treat the two IDs as globally
   interchangeable.
3. Extend the model only where needed for source app, subtitle, date, ANCS
   category, and optional action labels. Keep storage and rendering limits.
4. Add a touch-driven notification list and detail view. New notifications may
   show a bounded preview; privacy mode shows only app name/category.
5. Add user settings for ANCS enablement, preview policy, haptic/display wake,
   reconnect, and forget-phone. Default to no persistent content history.
6. Defer actionable responses until the read-only path is stable; then render
   the exact iOS-supplied labels and require explicit confirmation.

Gate: Messages, Mail, Calendar, and an incoming/missed-call case render with
correct add/modify/remove behavior, long and non-ASCII text remains safe, and
privacy mode suppresses content previews.

### Phase 6 — Power, coexistence, and reliability

1. Measure idle, screen-on, BLE-connected, reconnecting, and notification-burst
   current draw.
2. Enable BLE modem sleep/tickless behavior only after wake and reconnection are
   verified on this board.
3. Exercise Wi-Fi plus BLE coexistence, rapid notification bursts, out-of-range
   recovery, iPhone Bluetooth toggles, watch reboot, iPhone reboot, and bond
   removal from either side.
4. Run soak tests with bounded heap/PSRAM telemetry and assert that notification
   count, cache size, outstanding requests, and reconnect retries remain capped.

Gate: an eight-hour connected soak and a repeated disconnect/reconnect test show
no unbounded memory growth, crash loop, UI lockup, or loss of recovery access.

### Phase 7 — Convert the proven build into the supported package model

1. Resolve the Recovery/kernel partition-table conflict and define one
   authoritative 16 MB layout.
2. Build and sign standalone XL9555, AXP2101, CO5300, and CST9217 drivers, plus
   the selected window manager/system app package if applicable.
3. Sign the corrected board profile and component catalog; install packages
   transactionally onto storage-neutral `/system`.
4. Make Recovery the supported user-flashed artifact. Recovery installs the
   architecture-compatible kernel; the kernel resolves the signed T-Watch
   profile and its confirmed radio variant.
5. Test corrupted, missing, wrong-architecture, wrong-radio, and downgraded
   package cases and prove rollback without losing serial/ROM recovery.

Gate: a blank watch can be restored from the documented Recovery image and
reach the same hardware-verified ANCS experience without a developer-only
board image.

### Phase 8 — Complete remaining hardware without inflating the first claim

Continue issues #124-#127 in dependency order:

1. PCF85063A RTC and DRV2605 haptics.
2. MAX98357A speaker and T3902 PDM microphone.
3. GNSS, BHI260AP, and MicroSD through `/system`.
4. The radio fitted to this physical unit.
5. ST25R3916 NFC and shared-SPI coexistence.
6. Additional radio SKUs, each tracked and hardware-verified separately.

## 6. Verification matrix

| Level | Evidence required |
| --- | --- |
| Declared | Correct signed board/component profile and manifest schema |
| Built | Clean ESP32-S3 build, size report, artifact hashes, no suppressed failures |
| Host-tested | Driver fake-bus tests, ANCS parser fragmentation/fuzz cases, notification mapping tests |
| Flash-tested | Merged image flashes from a blank/recoverable device and produces serial boot evidence |
| Hardware-verified | Power, display, touch, BLE pairing, ANCS lifecycle, reconnect, sleep/wake, and soak logs |
| Release-ready | Recovery installer, signatures, catalog compatibility, rollback, documented contents/exclusions |

The build is not a release merely because the application binary compiles or
boots. The release claim requires flashable artifacts, signed/verified package
metadata, and physical evidence for the exact watch SKU.

## 7. New work items

Keep #120-#127. Add independently scoped ANCS work rather than hiding it inside
the hardware tracker:

1. **BLE central/client transport and secure iPhone bonding**
2. **Pure Rust Apple ANCS protocol parser and session state machine**
3. **ANCS-to-Thistle notification bridge and watch UI**
4. **T-Watch Ultra build preset, payload packaging, and hardware V&V**

The first item can share implementation with other future BLE-client services;
the ANCS parser should not own NimBLE directly.

## 8. Principal risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Wrong radio or board revision | Inventory the physical unit; explicit signed SKU selection; never default silently |
| Existing profile damages or fails to power hardware | Correct identities first; power only essential rails; verify sequencing from official source and factory test |
| QSPI transport or large framebuffer exhausts internal RAM | Use bounded DMA tiles and PSRAM; retain internal memory for task stacks and BLE |
| BLE server and central roles conflict or exhaust memory | Define one connection policy, size NimBLE limits deliberately, and measure heap under coexistence |
| Pairing UX cannot satisfy the configured security mode | Hardware-test the iPhone pairing flow early; support the actual NimBLE/ANCS authorization path instead of assuming the current display-only policy works |
| ANCS fragmentation corrupts state | Pure parser, total-size limits, one request policy, timeouts, split-point and malformed-input tests |
| Private notification data leaks | Ephemeral default, preview setting, bounded memory, log redaction, no body persistence |
| Architecture work blocks a first usable image | Maintain the explicit bring-up milestone, then package the proven behavior through Recovery |
| A successful boot is mistaken for support | Use the declared/built/flash-tested/hardware-verified/release-ready evidence ladder |

## 9. Immediate next execution slice

The first implementation iteration should stop after producing hard evidence,
not after attempting every phase:

1. Create a clean port branch/worktree and preserve the current dirty checkout.
2. Back up and inventory the physical watch, including the radio SKU.
3. Correct and provision the development board profile.
4. Add the T-Watch build preset and central/GATT-client configuration.
5. Bring up XL9555 and AXP2101, then CO5300 and CST9217.
6. Flash and verify the launcher plus touch on hardware.
7. Only then layer in ANCS pairing, protocol parsing, and UI integration.

That sequence gives useful failure isolation: power, panel transport, touch,
BLE transport, ANCS protocol, and UI can each be proven separately.

## 10. Sources

- Apple ANCS specification:
  <https://developer.apple.com/library/archive/documentation/CoreBluetooth/Reference/AppleNotificationCenterServiceSpecification/Specification/Specification.html>
- Apple Bluetooth developer resources:
  <https://developer.apple.com/bluetooth/>
- LilyGo T-Watch Ultra quick start and radio variants:
  <https://github.com/Xinyuan-LilyGO/LilyGoLib/blob/master/docs/lilygo-t-watch-ultra.md>
- LilyGo T-Watch Ultra hardware reference:
  <https://github.com/Xinyuan-LilyGO/LilyGoLib/blob/master/docs/hardware/lilygo-t-watch-ultra.md>
- ESP-IDF NimBLE central example:
  <https://github.com/espressif/esp-idf/tree/master/examples/bluetooth/nimble/blecent>
- Existing ThistleOS architecture records:
  `docs/recovery-first-installation.md`, `docs/display-driver-architecture.md`,
  `docs/rust-driver-inventory.md`, and `docs/architecture-conversation-record.md`
