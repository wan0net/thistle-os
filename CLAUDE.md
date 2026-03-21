# ThistleOS Development Guide

## Project Overview
ThistleOS is a portable ESP32-S3 operating system with an immutable kernel, loadable drivers and apps, swappable window managers, and Ed25519 signing at every level. Targets LilyGo T-Deck Pro (e-paper) and T-Deck (LCD).

## Build System
- **Firmware**: ESP-IDF v5.5 with CMake. `idf.py build`, `idf.py flash monitor`
- **Simulator**: SDL2 desktop build. `cd simulator/build && cmake .. && make -j8 && ./thistle_sim`
- **Rust kernel**: `cargo +esp check` in `components/kernel_rs/`. Tests: `cargo test --target aarch64-apple-darwin -- --test-threads=1` (37 tests)
- **Recovery OS**: `cargo +esp check` in `recovery/`
- **CI**: GitHub Actions — firmware build (espressif/idf:v5.5), Semgrep SAST, Trivy security scan

## Architecture (three-tier immutable trust chain)

```
Recovery (Rust, ota_0, 1MB) — immutable, root of trust
  ↓ Ed25519 verify
Kernel (100% Rust, ota_1, 4.5MB) — immutable, hardware-independent
  ├── Display Server (surfaces, compositor, input routing)
  ├── Kernel modules (app manager, IPC, events, permissions, signing, manifest, crypto)
  ├── Syscall table (45+ ESP-IDF APIs exported to loaded ELFs)
  └── HAL registry (bus handles, driver vtables)
         ↓ loads from
SPIFFS (10.5MB) + SD card — updateable
  ├── config/board.json (hardware pins, buses, driver list)
  ├── config/system.json (user prefs, WM selection, WiFi)
  ├── drivers/*.drv.elf (hardware drivers)
  ├── apps/*.app.elf (all apps)
  ├── wm/*.wm.elf (window managers)
  └── themes/*.json (UI themes)
```

### Hardware independence principle

The kernel is **100% Rust and hardware-independent**. It never calls ESP-IDF hardware APIs directly. All hardware interaction goes through HAL driver vtables:

- **Display, Input, Radio, GPS, Audio, Power, IMU, Storage, Network** — all accessed exclusively via their respective `hal_*_driver_t` vtables.
- **Crypto** — the `hal_crypto_driver_t` vtable allows hardware acceleration (e.g. ESP32-S3 hardware AES/SHA engines) when a crypto driver is registered. If no hardware crypto driver is present, the kernel's built-in Rust software implementation is used as a transparent fallback.
- The kernel itself does not know or care which physical chip is present — that detail lives entirely in the loaded `.drv.elf` files and `board.json`.

### Key components
- **HAL** (`components/thistle_hal/`): Pure vtable interfaces. Bus handle sharing (SPI/I2C). HAL interfaces: display, input, radio, GPS, audio, power, IMU, storage, network, crypto.
- **Drivers** (`components/drv_*/`): Currently compiled-in, migrating to standalone `.drv.elf`.
- **Board config**: `board.json` on SPIFFS defines pins, buses, and which drivers to load. Replaces compiled `board_*` components.
- **Kernel (Rust)** (`components/kernel_rs/`): 100% Rust. Manifest parser, permissions, IPC, events, app manager, version, crypto. 37 tests.
- **Display Server** (`components/kernel/src/display_server.c`): Surface management, compositor, WM vtable interface.
- **UI** (`components/ui/`): Current LVGL-based WM. Being refactored into a loadable `.wm.elf`.
- **Apps** (`components/apps_builtin/`): 14 apps, migrating to `.app.elf` on SPIFFS.
- **Recovery** (`recovery/`): Rust, WiFi AP + captive portal, OTA flashing.
- **Simulator** (`simulator/`): SDL2, real kernel code, fake WiFi/BLE, libcurl HTTP.

## Key Conventions
- License: BSD 3-Clause. No GPL dependencies.
- HAL interfaces: C structs of function pointers (vtables).
- Apps/drivers communicate via syscall table (C ABI).
- Drivers get bus handles via `hal_bus_get_spi()`/`hal_bus_get_i2c()`.
- Manifests: `manifest.json` alongside every `.app.elf` / `.drv.elf`.
- Signing: Ed25519 (Monocypher). Signed = full permissions. Unsigned = IPC only.
- Rust kernel modules expose `rs_*` C FFI functions matching the C API.
- All public headers use `#pragma once`.
- Error handling: `esp_err_t` return codes (C), i32 ESP error codes (Rust FFI).
- Logging: `ESP_LOG*` macros (C). Rust logging TBD.

## Adding a New Driver
**As standalone .drv.elf** (preferred):
1. Use `driver_sdk/` — include `thistle_driver.h`
2. Implement `driver_init(const char *config_json)`
3. Get bus handles: `hal_bus_get_i2c(0)`, `hal_bus_get_spi(0)`
4. Register vtable: `hal_*_register(&my_driver, NULL)`
5. Build with `thistle_driver()` CMake function or Rust cdylib
6. Add entry to `board.json` with pin config

**As compiled-in** (fallback):
1. Create `components/drv_<name>/`
2. Implement HAL vtable
3. Add to board definition

## Adding a New Board
1. Create `config/board.json` with pin assignments, bus configs, driver list
2. Place on SPIFFS or SD card
3. Kernel reads it at boot, initializes buses, loads drivers
4. No recompilation needed

## Adding a New Window Manager
1. Implement `display_server_wm_t` vtable (init, render, on_input, etc.)
2. Build as `.wm.elf`
3. Set `"window_manager"` in `system.json`
4. Or: register compiled-in via `display_server_register_wm()`
