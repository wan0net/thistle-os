# ThistleOS Changelog

## v0.5.2 - 2026-05-03

Recovery trust-root hardening release.

- Removed the blocking UART console from the minimal recovery control loop so web-triggered WiFi and bundle installs run without serial input.
- Made board catalog/config selection authoritative for recovery bundle matching; generic board pin probing is disabled in recovery.
- Recovery now verifies catalog SHA-256 hashes and Ed25519 signatures before flashing firmware, installing executable drivers/window managers, or applying SD-card firmware updates.
- Bundle firmware entries now flash directly to `ota_1` and set the boot partition during install instead of staging an update file on SD.

## v0.5.1 - 2026-05-03

Patch release from the watch compatibility and build-stabilisation work.

- Added recovery board catalog support so board lists can be served through the recovery web interface.
- Integrated watch compatibility and e-paper bring-up work, including Waveshare 2.06 Watch and T-Watch Ultra board data.
- Integrated PR #24 e-paper bring-up and input/launcher fixes.
- Fixed online simulator and WASM builds with missing platform and board helper stubs.
- Quieted residual firmware build warnings and removed stale LTO sdkconfig noise.
- Verified GitHub Actions for firmware, recovery, Rust kernel tests, security scans, and Pages/WASM simulator.

## Unreleased

(Tracking changes from the autonomous development loop.)

### Release hardening — 2026-05-03
- Added release flashing docs with checksum verification and slot offsets.
- Added a Pages-published recovery board catalog and CI validation for board configs.
- Split recovery board-list catalog from bundle catalog and added a dry-run install plan endpoint.
- Hardened recovery install flow: web requests no longer block behind UART input, board configs drive bundle selection, and catalog SHA-256 hashes are verified before firmware/files are installed.
- Added partition-size checks for firmware and recovery artifacts.
- Added a manual release workflow that publishes versioned assets and `SHA256SUMS`.

### Iteration 1 — 2026-03-26
- **Added:** `gps_track` kernel module (`components/kernel_rs/src/gps_track.rs`)
  - GPS track recording with point and waypoint management
  - GPX 1.1 XML export (metadata, waypoints, track segments, ISO 8601 timestamps)
  - Haversine distance calculation for total track distance
  - Bounds computation (bounding box of all points)
  - 7 C FFI exports for syscall table integration
  - 44 tests (all passing)

### Iteration 2 — 2026-03-26
- **Added:** `data_logger` kernel module (`components/kernel_rs/src/data_logger.rs`)
  - Structured data logging with typed columns (Int, Float, Text, Bool)
  - Schema locking after first row insertion
  - CSV export with ISO 8601 timestamps, proper quoting/escaping
  - Column statistics (min, max, mean) for numeric columns
  - Row-builder FFI pattern (begin_row/set_*/commit_row)
  - 14 C FFI exports for syscall table integration
  - 42 tests (all passing)

### Iteration 3 — 2026-03-26
- **Added:** `sos_beacon` kernel module (`components/kernel_rs/src/sos_beacon.rs`)
  - SOS emergency beacon protocol with 109-byte fixed-size packets
  - 6 status modes: Active, Moving, Immobile, Medical, Cancel, Test
  - CRC-16/CCITT checksum for packet integrity
  - Serialize/deserialize with magic validation and checksum verification
  - Configurable transmission intervals per status (10s-120s)
  - 64-byte text message field for distress details
  - 9 C FFI exports for syscall table integration
  - 48 tests (all passing)

### Iteration 4 — 2026-03-26
- **Added:** `secure_wipe` kernel module (`components/kernel_rs/src/secure_wipe.rs`)
  - Secure data destruction with 5 overwrite patterns (Zeros, Ones, Random, DoD 3-pass, Gutmann 35-pass)
  - Priority-based wipe ordering (Critical, High, Normal, Low)
  - Wipe plan state machine with lifecycle tracking
  - Default ThistleOS sensitive path targets
  - Xorshift64 PRNG for random overwrite generation
  - Byte-based progress tracking
  - 12 C FFI exports for syscall table integration
  - 44 tests (all passing)

### Iteration 5 — 2026-03-26
- **Added:** `notification` kernel module (`components/kernel_rs/src/notification.rs`)
  - System-wide notification queue with priority levels (Low, Normal, High, Urgent)
  - 5 categories: Message, System, App, Alert, Progress
  - Progress tracking notifications with update support
  - Auto-expiry, dismissal, and capacity-based eviction
  - Per-app filtering and unread counts
  - 10 C FFI exports for syscall table integration
  - 38 tests (all passing)

### Iteration 6 — 2026-03-26
- **Added:** `contact_manager` kernel module (`components/kernel_rs/src/contact_manager.rs`)
  - Address book: name, callsign, device ID (LoRa), phone (SMS), BLE address, Ed25519 public key
  - JSON persistence to `/sdcard/data/contacts.json` (manual serialization)
  - vCard 3.0 import/export with FN, NICKNAME, TEL, NOTE, KEY fields
  - Minimal base64 encode/decode for public key serialization
  - Contact search by name, device ID, phone number; emergency contact list
  - 16 C FFI exports for syscall table integration
  - 65 tests (all passing)

### Iteration 7 — 2026-03-27
- **Added:** `ble_scanner` kernel module (`components/kernel_rs/src/ble_scanner.rs`)
  - BLE device discovery with passive/active scan modes
  - Advertising data TLV parser: names, 16/128-bit UUIDs, manufacturer data, flags
  - Device storage for up to 64 discovered devices with auto-update on re-discovery
  - RSSI and name prefix filtering
  - Sort by signal strength, find by MAC address or name substring
  - Scan statistics (device count, total advertisements, signal range)
  - NimBLE `ble_gap_disc()` integration for ESP-IDF targets
  - 13 C FFI exports for syscall table integration
  - 45 tests (all passing)

### Iteration 8 — 2026-03-27
- **Added:** `burn_timer` kernel module (`components/kernel_rs/src/burn_timer.rs`)
  - Per-message burn timers with configurable duration
  - Per-conversation burn policies (auto-burn all new messages)
  - Monotonic time model — caller provides clock via tick()
  - Expired queue with drain semantics for messenger integration
  - Countdown remaining query per message
  - Circular buffer awareness (slot reuse replaces old timer)
  - 12 C FFI exports for syscall table integration
  - 47 tests (all passing)

### Iteration 9 — 2026-03-27
- **Added:** `msg_queue` kernel module (`components/kernel_rs/src/msg_queue.rs`)
  - Store-and-forward message queue with exponential backoff retry
  - Priority ordering: Urgent > High > Normal
  - Configurable TTL and max retries per message
  - JSON persistence to `/sdcard/data/msg_queue.json` with base64-encoded payloads
  - tick()/get_ready()/mark_sent()/mark_failed() lifecycle for transport integration
  - Purge completed entries, cancel individual or all messages
  - 15 C FFI exports for syscall table integration
  - 53 tests (all passing)

### Iteration 10 — 2026-03-27
- **Added:** `msg_crypto` kernel module (`components/kernel_rs/src/msg_crypto.rs`)
  - End-to-end message encryption with AES-256-CTR + HMAC-SHA256 (encrypt-then-MAC)
  - Per-contact encrypted channels with PBKDF2-derived master keys (10000 iterations)
  - Per-message key derivation via HMAC for forward-secrecy-like properties
  - Wire format: [version | nonce | ciphertext | hmac] — 49 bytes overhead
  - Constant-time HMAC comparison to prevent timing attacks
  - Key zeroization on channel destruction
  - 12 C FFI exports for syscall table integration
  - 48 tests (all passing)

### Iteration 11 — 2026-03-27
- **Added:** `driver_reload` kernel module (`components/kernel_rs/src/driver_reload.rs`)
  - Driver hot-reload lifecycle: register → load → start → stop → unload → reload
  - State machine with 5 states: Empty, Loaded, Running, Stopped, Error
  - Auto-stop on reload from Running state, recovery reload from Error state
  - HAL type tracking (10 types: display, input, radio, GPS, audio, power, IMU, storage, crypto, RTC)
  - Platform abstraction: no-op stubs for test, real ESP-IDF calls for target
  - Reload by ID or by file path, version tracking, load count
  - 16 C FFI exports for syscall table integration
  - 54 tests (all passing)

### Iteration 12 — 2026-03-28
- **Added:** 30 new syscalls — crypto (SHA-256, HMAC, AES-256-CBC, AES-128-ECB, PBKDF2, Ed25519, X25519, RNG) and mesh service (15 functions)
- **Added:** `drv_crypto_mbedtls` HAL crypto driver — hardware-accelerated SHA-256, AES, HMAC, RNG via ESP-IDF mbedtls
- **Added:** AES-128-ECB hardware acceleration — extended HAL crypto vtable, wired through mbedtls driver
- **Added:** Real Ed25519/X25519 cryptography — replaced insecure MeshCore stubs with ed25519-dalek/x25519-dalek
- **Added:** Messenger integration — wired contact manager, message encryption, burn timer, and message queue into messenger app with periodic tick, contact resolution, and queue retry
- **Fixed:** MeshCore Ed25519 stubs — all identity operations now use real cryptography
- **Fixed:** SHA256.h buffer overflow — fail-safe instead of silent truncation
- **Fixed:** `hmac_verify` — routes through hardware dispatch, returns ESP_FAIL on mismatch, constant-time comparison
- **Fixed:** Mesh init race conditions — lock ordering prevents deadlock, callbacks registered before radio starts
- **Fixed:** App SDK ABI mismatches — `thistle_fs_open` (was POSIX flags, now fopen mode), `thistle_log` (was variadic, now 2-arg)
- **Fixed:** Dev/production signing keys — now distinct to prevent cross-environment acceptance
- **Implemented:** BHI260AP IMU driver — I2C init, chip ID verification, FIFO read, virtual sensor configuration
- **Tagged:** v1.0.0-alpha
