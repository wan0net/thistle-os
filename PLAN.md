# ThistleOS Implementation Plan

## IMMEDIATE PRIORITY: Flash on T-Deck Pro

### Step 1: Fix partition table for first flash
- Current table has ota_0 + ota_1 but no factory partition
- For first flash without Recovery OS: flash to ota_1 directly
- Or: temporarily add factory partition for initial bring-up
- **Command**: `idf.py -p /dev/ttyACM0 flash monitor`

### Step 2: Verify kernel boot on hardware
- Rust kernel calls board_config_init() → falls back to board_init()
- board_init() initializes SPI/I2C buses, registers T-Deck Pro drivers
- hal_registry_start_all() calls each driver's init()
- **Verify**: serial monitor shows driver init logs

### Step 3: Verify display
- E-paper driver (GDEQ031T10) initializes via SPI
- LVGL renders splash screen → launcher
- **Verify**: see "ThistleOS" on e-paper

### Step 4: Verify input
- TCA8418 keyboard (I2C) + CST328 touch (I2C)
- **Verify**: type on keyboard, tap screen → apps respond

### Step 5: Verify peripherals
- WiFi scanner shows networks
- File manager shows SD card
- GPS/LoRa/audio stubs don't crash
- **Verify**: basic functionality across all apps

### Known risks for first flash
- kernel_shims.c WiFi init may need ESP-IDF event loop tuning
- BLE NimBLE shims are stubs — BLE won't work yet
- E-paper refresh timing may need calibration
- kernel_run() uses std::thread::sleep — should use vTaskDelay on ESP
- Signing key was rotated — built-in apps don't need signing (compiled in)
- widget_shims.c needs display_server_get_active_wm() — verify linkage

---

## Backlog (after first successful flash)

### High Priority
- [ ] Toybox integration (BSD shell for GhostTerm)
- [ ] GhostTerm: wire libghostty-vt for real terminal emulation
- [ ] BLE: implement real NimBLE advertising/GATT (not stubs)
- [ ] WiFi: wire ESP-IDF event handler to Rust wifi_manager
- [ ] Fix remaining HIGH security items
- [ ] Deploy WASM simulator to GitHub Pages

### Medium Priority
- [ ] Compile all drivers as standalone .drv.elf
- [ ] Move built-in apps to .app.elf on SPIFFS
- [ ] Hardware crypto driver (ESP32-S3 AES/SHA acceleration)
- [ ] Encrypted SPIFFS (full disk encryption)
- [ ] Settings as part of WM, not standalone app
- [ ] LVGL WM as loadable .wm.elf

### Stretch Goals
- [ ] Rust window manager (embedded-graphics)
- [ ] Terminal WM (headless/serial)
- [ ] Claude API in AI assistant
- [ ] More boards (T-Beam, M5Stack, Heltec)
- [ ] WASM enhancements (fetch API, IndexedDB)
- [ ] First-boot setup wizard
- [ ] Hardware auto-detection
- [ ] Encrypted LoRa messaging

---

## What's Done
- [x] 100% Rust kernel (20 modules, 66 tests)
- [x] Widget API (toolkit-agnostic, WM-swappable)
- [x] LVGL WM with full widget implementation
- [x] Crypto HAL (software fallback + hardware acceleration support)
- [x] Kernel crypto module (SHA-256, HMAC, AES-256-CBC, PBKDF2, CSPRNG)
- [x] Ed25519 signing chain (Recovery → Kernel → Apps/Drivers)
- [x] Security audit: 4 CRITICAL + 3 HIGH fixed, key rotated
- [x] Display server with swappable WM vtable
- [x] Boot-from-JSON (board.json → bus init → driver loading)
- [x] Driver SDK (C + Rust), 2 standalone .drv.elf binaries
- [x] App SDK, 3 standalone .app.elf binaries (hello, flashlight, ghostterm)
- [x] Desktop simulator (SDL2, all 14 apps)
- [x] WASM browser simulator (Emscripten, interactive)
- [x] CI: 4 jobs (firmware, Rust tests, Semgrep, Trivy)
- [x] Recovery OS (Rust, compiles clean)
- [x] GhostTerm scaffolded (libghostty-vt built)
- [x] Vault uses kernel crypto on all platforms
