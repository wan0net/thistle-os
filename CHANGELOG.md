# ThistleOS Changelog

## Unreleased

(Tracking changes from the autonomous development loop.)

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
