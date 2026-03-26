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
