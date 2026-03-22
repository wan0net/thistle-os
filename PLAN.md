# ThistleOS Implementation Plan

## Backlog

### High Priority
- [ ] Toybox integration (BSD shell for GhostTerm)
- [ ] GhostTerm: wire libghostty-vt for real terminal emulation
- [ ] BLE: implement real NimBLE advertising/GATT (not stubs)
- [ ] WiFi: wire ESP-IDF event handler to Rust wifi_manager
- [ ] Deploy WASM simulator to GitHub Pages

### Medium Priority
- [ ] Compile all drivers as standalone .drv.elf (currently compiled-in with Rust replacements)
- [ ] Move built-in C apps to Rust .app.elf on SPIFFS
- [ ] Hardware crypto driver (ESP32-S3 AES/SHA acceleration)
- [ ] Encrypted SPIFFS (full disk encryption)
- [ ] LVGL WM as loadable .wm.elf
- [ ] ILI9341 LCD driver (for CYD board)
- [ ] XPT2046 resistive touch driver (for CYD board)
- [ ] SX1262 radio driver in Rust (currently C++/RadioLib)

### Stretch Goals
- [ ] Terminal WM (headless/serial)
- [ ] Claude API in AI assistant
- [ ] First-boot setup wizard
- [ ] Encrypted LoRa messaging
- [ ] WASM app store web interface
- [ ] More boards (T-Beam, M5Stack, Heltec)
- [ ] WASM enhancements (fetch API, IndexedDB)

---

## What's Done

- [x] Pure Rust kernel (42 modules, 515 tests)
- [x] HAL registry in Rust (hal_registry.rs)
- [x] kernel_shims.c reduced to Rust (57 LOC weak link stubs only)
- [x] tk_wm_shims.c reduced to Rust (HAL bridges moved, 123 LOC remaining)
- [x] 14 Rust hardware drivers (e-paper, LCD, OLED, keyboard, touch ×2, GPS, accelerometer, power, audio, RTC/PCF8563, SD card, QMI8658C 6-axis IMU, light sensor stub)
- [x] Multi-board support (6 boards: T-Deck Pro, T-Deck, T-Display-S3, T3-S3, CYD, C3-Mini)
- [x] Multi-arch ESP32 support (ESP32, S2, S3, C3, C6, H2 — chip detection, catalog arch filtering)
- [x] Recovery hardware scanning (I2C/SPI/UART component-level detection)
- [x] Recovery web UI with 3-step provisioning flow
- [x] App store with rich metadata (ratings, categories, download counts, changelogs)
- [x] App store Rust app (tk_appstore.rs) — browsable UI with categories
- [x] Component-level driver detection and matching (arch-aware installs)
- [x] C test suite migrated to Rust #[cfg(test)]
- [x] RTC HAL interface and PCF8563 driver
- [x] QMI8658C real 6-axis IMU driver
- [x] Documentation: devices page, updated architecture/recovery/app-store docs
- [x] Hardware auto-detection (I2C/SPI/UART scanning in Recovery)
- [x] thistle-tk WM (default window manager, Rust + embedded-graphics)
- [x] 100% Rust kernel (board_config, driver_manager, driver_loader in Rust)
- [x] Widget API (toolkit-agnostic, WM-swappable)
- [x] LVGL WM with full widget implementation
- [x] Crypto HAL (software fallback + hardware acceleration support)
- [x] Kernel crypto module (SHA-256, HMAC, AES-256-CBC, PBKDF2, CSPRNG)
- [x] Ed25519 signing chain (Recovery → Kernel → Apps/Drivers)
- [x] Security audit: 4 CRITICAL + 3 HIGH fixed, key rotated
- [x] Display server with swappable WM vtable
- [x] Boot-from-JSON (board.json → bus init → driver loading)
- [x] Driver SDK (C + Rust), standalone .drv.elf binaries
- [x] App SDK, standalone .app.elf binaries (hello, flashlight, ghostterm)
- [x] Desktop simulator (SDL2, all 14 apps)
- [x] WASM browser simulator (Emscripten, interactive)
- [x] CI: 4 jobs (firmware, Rust tests, Semgrep, Trivy)
- [x] Recovery OS (Rust, compiles and flashes clean)
- [x] GhostTerm scaffolded (libghostty-vt built)
- [x] Vault uses kernel crypto on all platforms
- [x] T-Deck Pro hardware bringup — device booting and running
