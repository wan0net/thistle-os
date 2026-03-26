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
