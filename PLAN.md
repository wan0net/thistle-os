# ThistleOS Implementation Plan

## What's Done

### Architecture (complete)
- [x] 3-tier immutable trust chain: Recovery → Kernel → Apps/Drivers
- [x] 100% Rust kernel (19 modules, 66 tests)
- [x] Display server with swappable WM vtable
- [x] LVGL window manager (compiled-in, registered via display server)
- [x] Boot-from-JSON (board.json → bus init → driver loading)
- [x] HAL vtables: display, input, radio, GPS, audio, power, IMU, storage, network, crypto
- [x] Shared bus handles (SPI/I2C in HAL registry)
- [x] Expanded syscall table (45+ ESP-IDF APIs)
- [x] Driver SDK (C + Rust templates)
- [x] Manifest system (apps, drivers, firmware)
- [x] Recovery OS (Rust, compiles clean)

### Security (complete)
- [x] Ed25519 signing (ed25519-dalek, pure Rust)
- [x] Unsigned ELFs refused in production builds
- [x] Manifest ID used for permission identity (not file path)
- [x] Signing key rotated, dev key invalidated
- [x] App store aborts on signature download failure
- [x] Path traversal sanitization in board.json
- [x] File size limits in signature verification
- [x] 4 CRITICAL + 3 HIGH vulnerabilities fixed

### Crypto (complete)
- [x] Kernel crypto module: SHA-256, HMAC-SHA256, AES-256-CBC, PBKDF2, CSPRNG
- [x] Crypto HAL driver vtable (hardware acceleration with software fallback)
- [x] Vault uses kernel crypto on all platforms (no more mbedtls in apps)
- [x] Pure Rust crypto — works on ESP32, desktop, WASM

### Simulators (complete)
- [x] Desktop simulator (SDL2, all 14 apps)
- [x] WASM browser simulator (Emscripten, canvas, input, timers)
- [x] WASM shell with console, WiFi, BLE, hardware panels
- [x] Vault encrypted in WASM (same AES-256 as firmware)

### CI (needs fixing)
- [x] GitHub Actions: firmware build + Semgrep SAST + Trivy
- [ ] CI currently failing — Rust ESP toolchain + shim issues

---

## Phase 1: Stabilize CI (immediate)

### 1.1 Fix firmware build
- Fix remaining linker errors from Rust kernel + C shims
- Verify all 3 CI jobs pass (firmware, Semgrep, Trivy)
- **Deliverable**: Green CI on every push

### 1.2 Add Rust tests to CI
- Add a `cargo test` job that runs the 66 Rust kernel tests
- Run on ubuntu-latest (host target, no ESP toolchain needed)
- **Deliverable**: Rust tests run on every PR

### 1.3 Add WASM build to CI
- Add an Emscripten build job
- Deploy WASM simulator to GitHub Pages (try.thistleos.org?)
- **Deliverable**: Browser simulator auto-deployed on push

---

## Phase 2: Make Everything Loadable

### 2.1 Compile one real driver as .drv.elf
- Pick drv_kbd_tca8418 (simple I2C driver)
- Build as standalone .drv.elf using driver SDK
- Place on simulated SPIFFS, load via board.json
- **Deliverable**: First hardware driver loaded at runtime

### 2.2 Compile one app as .app.elf
- Pick flashlight (simplest app)
- Build as standalone .app.elf using app SDK
- Create manifest.json, place on SPIFFS
- Load at boot, verify in simulator
- **Deliverable**: First app loaded from storage

### 2.3 Move all built-in apps to .app.elf
- Compile each of the 14 apps as standalone ELFs
- Launcher discovers apps from SPIFFS/SD manifests
- **Deliverable**: Immutable kernel with all apps on storage

---

## Phase 3: Hardware Crypto Driver

### 3.1 ESP32-S3 crypto driver
- Create drv_crypto_esp32s3 wrapping ESP-IDF hardware:
  - `esp_sha()` for SHA-256
  - `esp_aes_crypt_cbc()` for AES-256
  - `esp_random()` for true hardware RNG
- Register via `hal_crypto_register()`
- Kernel auto-dispatches to hardware when available
- **Deliverable**: 10x faster crypto on ESP32-S3

### 3.2 Encrypted SPIFFS (stretch)
- Use kernel crypto to encrypt/decrypt SPIFFS contents
- Key derived from device-specific secret (eFuse or NVS)
- All user data at rest is encrypted
- **Deliverable**: Full disk encryption for user partition

---

## Phase 4: Window Manager Ecosystem

### 4.1 LVGL WM as loadable .wm.elf
- Extract current ui/ component into a standalone WM binary
- Load from SPIFFS, register via display_server_register_wm()
- **Deliverable**: WM is an updateable module, not baked in

### 4.2 Rust window manager
- Implement display_server_wm_t using embedded-graphics (MIT)
- Minimal: status bar + app list + text rendering
- Optimized for e-paper (1-bit, dirty regions)
- **Deliverable**: Alternative lightweight WM

### 4.3 Terminal WM
- Text-mode WM for headless/serial use
- No display hardware needed
- **Deliverable**: ThistleOS on any ESP32 with a serial port

---

## Phase 5: User Experience

### 5.1 First-boot setup wizard
- Detect if system.json exists
- Setup flow: language → WiFi → WM selection → done
- **Deliverable**: Clean first-run experience

### 5.2 App store integration
- Wire WASM simulator to use browser fetch() for HTTP
- App store catalog browsable in browser demo
- Download + install apps in the simulator
- **Deliverable**: Try → download → run cycle in browser

### 5.3 Hardware auto-detection
- Probe I2C/SPI for known device signatures at boot
- Generate or select board.json automatically
- **Deliverable**: Plug in any supported board, it works

---

## Phase 6: New Features

### 6.1 Claude API in AI assistant
- Wire assistant app to Claude API
- Chat interface with device context
- **Deliverable**: On-device AI assistant

### 6.2 More board support
- T-Beam (GPS + LoRa, no display)
- M5Stack (LCD, buttons)
- Heltec (OLED, LoRa)
- Custom boards via board.json
- **Deliverable**: Multi-board ecosystem

### 6.3 Encrypted messaging
- Use kernel crypto for end-to-end encrypted LoRa messages
- Key exchange via BLE or QR code
- **Deliverable**: Secure mesh messaging

### 6.4 WASM web simulator enhancements
- Connect WiFi/BLE panels to actual kernel state
- Emulate SPIFFS with IndexedDB persistence
- App store via fetch() API
- Share simulator state via URL
- **Deliverable**: Full-featured browser demo

---

## Remaining Security Items

| Issue | Severity | Description |
|-------|----------|-------------|
| HIGH-2 | HIGH | JSON parser prefix confusion — need proper parser |
| HIGH-3 | HIGH | ELF task raw pointer race — need slot index pattern |
| HIGH-4 | HIGH | Driver loader TOCTOU on slot — need single critical section |
| HIGH-6 | HIGH | HTTP client struct padding — need bindgen or C shim |

---

## Architecture Principles

1. **Kernel is hardware-independent** — all hardware interaction through HAL drivers
2. **Everything is loadable** — apps, drivers, WMs loaded from SPIFFS/SD
3. **One codebase, three targets** — ESP32 firmware, desktop simulator, WASM browser
4. **Rust for safety** — kernel is 100% Rust, C only for LVGL UI and HAL vtable implementations
5. **Crypto as a service** — kernel provides crypto primitives, hardware accelerated when available
6. **Signed trust chain** — Recovery → Kernel → everything else, Ed25519 at every level
