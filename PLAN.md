# ThistleOS Implementation Plan

## Goal
End-to-end stable architecture with a working simulator. Every layer functional: Recovery → Kernel → Display Server → Window Manager → Apps, with all components loadable from SPIFFS/SD.

## Current State (what works)
- Kernel boots, 14 built-in apps run in simulator
- Ed25519 signing chain (Recovery → Kernel → Apps/Drivers)
- Rust kernel: 6 modules (manifest, permissions, IPC, events, app manager, version) — 28 tests
- Boot-from-JSON: board.json → bus init → driver loading
- Display server: surface management, compositor, input routing, WM vtable
- Driver SDK: C and Rust templates for .drv.elf
- Expanded syscall table: 45 ESP-IDF APIs exported
- Recovery OS: Rust, compiles clean, WiFi captive portal
- CI: firmware build + Semgrep + Trivy, all passing

## Phase 1: Wire the New Architecture (simulator first)
Priority: Get the display server + WM working in the simulator.

### 1.1 Create lvgl-wm as a WM module
- Extract current ui/ component (manager.c, statusbar.c, theme.c, app_switcher.c) into a display_server_wm_t implementation
- The WM's init() creates LVGL display, registers surfaces with display server
- The WM's render() calls lv_timer_handler()
- The WM's on_input() feeds events to LVGL indev
- This is a refactor, not a rewrite — same code, new interface
- **Deliverable**: Simulator boots through display server → lvgl-wm → apps

### 1.2 Wire display server into kernel boot
- kernel.c: after board_config_init(), call display_server_init()
- Load WM name from system.json
- If "lvgl-wm": register the compiled-in LVGL WM
- If path to .wm.elf: load via ELF loader (future)
- Hook HAL input callbacks to display_server's input handler
- **Deliverable**: Same visual result as today, but through the display server path

### 1.3 Update simulator for display server
- sim_display.c: flush callback goes through display server composite
- sim_input.c: events routed through display server
- Verify all 14 apps still work in simulator
- **Deliverable**: Simulator fully functional with new architecture

## Phase 2: Make Drivers Loadable
Priority: Compile at least one real driver as .drv.elf and load it.

### 2.1 Update driver entry point
- Change driver_init(void) to driver_init(const char *config_json) in:
  - thistle_driver.h (SDK)
  - driver_loader.c (pass config from board.json)
  - All existing drv_* components (accept and parse config_json)
- Add JSON config parsing helpers to driver SDK
- **Deliverable**: Existing compiled-in drivers accept JSON config

### 2.2 Build one driver as standalone .drv.elf
- Start with drv_kbd_tca8418 (simple I2C driver, well understood)
- Use driver_sdk CMake to compile as .drv.elf
- Place on simulated SPIFFS/SD
- Load via board.json → driver_loader
- Verify keyboard works in simulator
- **Deliverable**: First real loadable driver

### 2.3 Build remaining drivers as .drv.elf
- Compile each drv_* as standalone .drv.elf
- Test each in simulator
- Keep compiled-in versions as fallback
- **Deliverable**: All drivers available as .drv.elf

## Phase 3: Make Apps Loadable
Priority: Move built-in apps to .app.elf on SPIFFS.

### 3.1 Update ELF loader for new manifest system
- Parse .thistle_app section from ELF OR read companion manifest.json
- Pass manifest info to app_manager on registration
- Check arch + min_os compatibility before loading
- **Deliverable**: ELF-loaded apps have proper manifests

### 3.2 Build one app as standalone .app.elf
- Start with flashlight (simplest app)
- Use app_sdk CMake to build .app.elf
- Create manifest.json
- Place on simulated SPIFFS
- Load at boot, verify it works
- **Deliverable**: First real loadable app

### 3.3 Build remaining apps as .app.elf
- Compile each built-in app as .app.elf
- Create manifests for each
- Launcher discovers and lists apps from SPIFFS/SD
- Keep compiled-in registration as fallback
- **Deliverable**: All apps loadable from SPIFFS

## Phase 4: Switch Kernel to Rust
Priority: Replace C kernel calls with Rust rs_* implementations.

### 4.1 Switch one module at a time
- Order: permissions → event → ipc → manifest → app_manager
- For each: replace C function calls with rs_* equivalents in kernel.c
- Remove C source file from CMakeLists
- Verify tests still pass, simulator still works
- **Deliverable**: Each module switched individually

### 4.2 Add Rust logging
- Wire the `log` crate to ESP-IDF's esp_log
- Replace ESP_LOGI/LOGW/LOGE with log::info!/warn!/error!
- **Deliverable**: Rust modules produce visible log output

### 4.3 Port display server to Rust
- The display server is new code in C — port to Rust
- Surface management, compositor, input routing all in Rust
- Expose C FFI for WM to call
- **Deliverable**: Display server is Rust

## Phase 5: First-Boot and User Experience

### 5.1 First-boot setup wizard
- Detect if system.json exists
- If not: launch setup app (not launcher)
- Setup flow: select language → connect WiFi → choose WM → done
- Writes system.json + board auto-detection results
- **Deliverable**: Clean first-run experience

### 5.2 App store integration for WMs
- WMs appear in app store catalog as type "wm"
- Download .wm.elf to SPIFFS
- Settings → Appearance → Window Manager picker
- **Deliverable**: Users can switch WMs from the app store

### 5.3 Hardware auto-detection
- At boot, probe I2C/SPI for known device signatures
- Generate board.json from detected hardware
- Or: load board.json from a known board database on SPIFFS
- **Deliverable**: Plug in any supported board, it just works

## Phase 6: Stretch Goals

### 6.1 Rust window manager
- Implement display_server_wm_t using embedded-graphics (MIT)
- Minimal: status bar + app list + text rendering
- Optimized for e-paper (1-bit, dirty regions)
- **Deliverable**: Alternative WM for low-resource boards

### 6.2 Terminal-only WM
- Text-mode WM using serial/VT100
- No display hardware needed
- Useful for headless boards, debugging, SSH access
- **Deliverable**: ThistleOS runs on any ESP32 with a serial port

### 6.3 WASM web simulator
- Compile kernel + display server to WASM (emscripten)
- HTML5 Canvas replaces SDL2
- Browser keyboard/mouse for input
- App store works via fetch()
- **Deliverable**: Try ThistleOS in your browser

### 6.4 Claude API integration
- AI assistant app connects to Claude API
- Chat interface with context about the device
- Voice input via audio HAL (future)
- **Deliverable**: On-device AI assistant

## Verification Checkpoints

After each phase, verify:
1. Simulator builds and runs (`cmake && make && ./thistle_sim`)
2. CI passes (firmware build + Semgrep + Trivy)
3. All Rust tests pass (`cargo test -- --test-threads=1`)
4. Existing functionality not broken (14 apps, themes, app store)

## Architecture Diagram (target state)

```
Recovery OS (Rust, ota_0, 1MB)
  ↓ Ed25519 verify
Kernel (Rust, ota_1, 4.5MB) — IMMUTABLE
  ├── Display Server (surfaces, compositor, input)
  ├── App Manager (lifecycle, LRU eviction)
  ├── IPC + Event Bus (message passing, pub/sub)
  ├── Permissions (enforced at syscall boundary)
  ├── Signing (Ed25519 via Monocypher)
  ├── Manifest Parser (JSON, version/arch checks)
  ├── Driver Loader (ELF from SPIFFS/SD)
  ├── App Loader (ELF from SPIFFS/SD)
  ├── Syscall Table (45+ ESP-IDF APIs)
  └── HAL Registry (bus handles, driver vtables)
         ↓
SPIFFS (10.5MB internal flash) + SD Card — UPDATEABLE
  ├── config/board.json (hardware pin config)
  ├── config/system.json (user preferences, WM selection)
  ├── drivers/*.drv.elf (hardware drivers)
  ├── apps/*.app.elf (all apps including launcher, settings)
  ├── wm/*.wm.elf (window managers)
  └── themes/*.json (UI themes)
```

## File Count Estimate (target)
| Component | Current | Target |
|-----------|---------|--------|
| Kernel (Rust) | 2,800 lines | ~5,000 lines |
| Kernel (C) | 4,548 lines | ~1,000 lines (boot + glue) |
| Display server | 525 lines | ~800 lines (Rust) |
| WM (LVGL) | 1,386 lines | ~1,500 lines (.wm.elf) |
| Drivers | 3,675 lines | Same (now .drv.elf) |
| Apps | 13,070 lines | Same (now .app.elf) |
| Driver SDK | 270 lines | ~400 lines |
| App SDK | 148 lines | ~300 lines |
