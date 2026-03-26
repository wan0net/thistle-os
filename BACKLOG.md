# ThistleOS Backlog

## Iteration 1 — 2026-03-26

### Priority 1: GPS Track Logger & GPX Export Module
**Status:** DONE (unmerged, branch: feat/gps-track-gpx)
**Personas:** Cairn (SAR coordinate sharing), Ember (field transect logging)
**Description:** Add a `gps_track` kernel module that records GPS positions over time and exports them as GPX XML. Enables track recording, waypoint management, and GPX file generation for SD card export. Pure Rust, no hardware dependency beyond existing GPS HAL.

### Priority 2: Complete Power Driver (TP4065B) — Real ADC Battery Readings
**Status:** PENDING (local changes in progress on main)
**Personas:** ALL (battery monitoring is universal)
**Description:** Replace stub/TODO ADC readings in drv_power_tp4065b.rs with real ESP-IDF ADC calls. Provide accurate battery percentage, voltage, and charging status.

### Priority 3: Messenger Internet Transport (WiFi/WebSocket)
**Status:** PENDING (local changes in progress on main)
**Personas:** Cairn, Thorn, Ember
**Description:** Implement the WiFi/WebSocket transport path in messenger_transport.c. Currently stubbed with TODO, returns ESP_ERR_NOT_SUPPORTED.

### Priority 4: BLE Scanner App
**Status:** PENDING
**Personas:** Spark (primary), Thorn (security awareness)
**Description:** New app that discovers and displays nearby BLE devices, their services, RSSI, and manufacturer data. Builds on the pure Rust BLE manager.

### Priority 5: SOS Beacon Mode
**Status:** PENDING
**Personas:** Cairn (emergency), Ember (safety)
**Description:** Emergency mode that broadcasts GPS position over LoRa at regular intervals with SOS flag. Minimal UI, maximum battery conservation.

### Priority 6: Device Wipe / Panic Button
**Status:** PENDING
**Personas:** Thorn (journalist safety)
**Description:** Secure wipe of SD card contents and SPIFFS user data on long-press of a configurable key combination.

### Priority 7: Structured Data Logger
**Status:** DONE (unmerged, branch: feat/data-logger)
**Personas:** Ember (field research), Fern (sensor monitoring)
**Description:** Generic CSV/JSON data logging module that apps can use to record timestamped sensor readings to SD card.

### Priority 8: Driver Hot-Reload
**Status:** PENDING
**Personas:** Fern (development workflow)
**Description:** Ability to unload and reload .drv.elf files without rebooting. Requires driver lifecycle management in driver_manager.
