# ThistleOS

<p align="center">
  <img src="docs/thistle-logo.svg" alt="ThistleOS Logo" width="200">
</p>

<p align="center">
  <strong>An open-source operating system for ESP32-S3 devices</strong><br>
  Built for the LilyGo T-Deck Pro &bull; E-paper &amp; LCD &bull; LoRa mesh &bull; 4G &bull; GPS
</p>

<p align="center">
  <a href="#features">Features</a> &bull;
  <a href="#getting-started">Getting Started</a> &bull;
  <a href="#simulator">Simulator</a> &bull;
  <a href="#architecture">Architecture</a> &bull;
  <a href="#apps">Apps</a> &bull;
  <a href="#contributing">Contributing</a> &bull;
  <a href="#license">License</a>
</p>

---

## Features

- **14 built-in apps**: Launcher, Settings, File Manager, Ebook Reader, Messenger (multi-transport: LoRa/SMS/BLE), GPS Navigator, Notes, AI Assistant, App Store, WiFi Scanner, Flashlight/SOS, Weather Station, Terminal, Password Vault
- **HAL vtable architecture**: Hardware-agnostic kernel with swappable drivers
- **Dynamic app/driver loading**: ELF files loaded from SD card at runtime
- **Live app store**: HTTPS catalog fetch, download with SHA-256 + signature verification
- **Multi-board support**: T-Deck Pro (e-paper) and T-Deck (LCD)
- **SDL2 simulator**: Full desktop simulation with WiFi/BLE/SD card emulation
- **Theme engine**: JSON themes from SD card (includes dark, default, link42 themes)
- **OTA updates**: Dual partition with SD card and HTTP firmware updates
- **Security**: App signing (HMAC-SHA256, Ed25519 upgrade planned), permissions system
- **Recovery OS**: Rust-based recovery firmware with WiFi captive portal (WIP)
- **Network abstraction**: Transport-agnostic internet (WiFi, 4G PPP, simulator host)
- **Runtime driver loading**: `.drv.elf` files from SD card register with HAL

## Hardware Support

### Primary: LilyGo T-Deck Pro

| Component | Chip | Driver |
|-----------|------|--------|
| MCU | ESP32-S3FN16R8 (dual-core 240 MHz) | — |
| Display | 3.1" GDEQ031T10 e-paper (320x240) | `drv_epaper_gdeq031t10` |
| Touch | CST328 capacitive | `drv_touch_cst328` |
| Keyboard | TCA8418 matrix scanner | `drv_kbd_tca8418` |
| LoRa | SX1262 (RadioLib) | `drv_radio_sx1262` |
| GPS | U-blox MIA-M10Q | `drv_gps_mia_m10q` |
| Audio | PCM5102A I2S DAC | `drv_audio_pcm5102a` |
| Battery | TP4065B + ADC | `drv_power_tp4065b` |
| IMU | Bosch BHI260AP | `drv_imu_bhi260ap` |
| Storage | MicroSD (SPI) | `drv_sdcard` |
| 4G | Simcom A7682E (esp_modem) | `drv_modem_a7682e` |
| Ambient light | LTR-553ALS | `drv_light_ltr553` |
| Connectivity | WiFi 4, BLE 5.0 (NimBLE) | Built-in |

### Secondary: LilyGo T-Deck (LCD)

| Component | Chip | Driver |
|-----------|------|--------|
| Display | ST7789 320x240 TFT (esp_lcd) | `drv_lcd_st7789` |

## Getting Started

### Prerequisites

- [ESP-IDF v5.3+](https://docs.espressif.com/projects/esp-idf/en/latest/esp32s3/get-started/)
- [Rust + esp-rs](https://esp-rs.github.io/book/) (for Recovery OS only)

### Build and Flash

```bash
# Clone
git clone https://github.com/wan0net/thistle-os.git
cd thistle-os

# Set up ESP-IDF environment
. ~/esp/esp-idf/export.sh

# Configure target and build
idf.py set-target esp32s3
idf.py build

# Flash and monitor
idf.py -p /dev/ttyACM0 flash monitor
```

### Run Unit Tests

Uncomment `CONFIG_THISTLE_RUN_TESTS=y` in `sdkconfig.defaults`, build, and flash. The device runs all 76 Unity tests on boot and prints results to serial.

### Build Recovery OS (Rust)

```bash
cd recovery
. ~/export-esp.sh
export RUSTUP_TOOLCHAIN=esp
cargo build --release --target xtensa-esp32s3-espidf -Zbuild-std=std,panic_abort
```

## Simulator

Run ThistleOS on your desktop with SDL2:

```bash
# Install SDL2
brew install sdl2 pkg-config   # macOS
# apt install libsdl2-dev      # Linux

# Build and run
cd simulator
mkdir build && cd build
cmake .. && make -j8
./thistle_sim
```

The simulator provides:
- SDL2 display (640x480, 2x scaled)
- Keyboard and mouse input mapped to HAL events
- Fake WiFi networks (scan and connect)
- BLE state machine simulation
- SD card emulation via `/tmp/thistle_sdcard` symlink
- Real HTTP via libcurl (app store works)
- Host system clock for NTP

## Architecture

```
+---------------------------------------------+
|         APPS (14 built-in + ELF)            |
+---------------------------------------------+
|              SYSCALL TABLE                   |
+---------------------------------------------+
|                  KERNEL                      |
|  App Manager | Driver Mgr | UI/LVGL 9       |
|  IPC/Events  | ELF Loader | Net Manager     |
|  OTA/Signing | Permissions| App Store Client|
+---------------------------------------------+
|          HAL (vtable interfaces)            |
|  Display | Input | Radio | GPS | Audio      |
|  Power   | IMU   | Storage | Network        |
+---------------------------------------------+
|    DRIVERS (compiled-in + SD card ELF)      |
+---------------------------------------------+
|      ESP-IDF + FreeRTOS + Hardware          |
+---------------------------------------------+
```

### Component Layout

```
components/
  thistle_hal/          HAL vtable interface definitions (no implementations)
  kernel/               App manager, driver manager, IPC, event bus, syscall table
  ui/                   LVGL 9 window manager, theme engine, status bar, toast
  board_tdeck_pro/      T-Deck Pro pin config, driver wiring
  board_tdeck/          T-Deck (LCD) pin config, driver wiring
  apps_builtin/         All 14 built-in app implementations
  drv_*/                Individual driver components (one per chip)
  shim/                 MeshCore compatibility shim layer
  test_thistle/         Unity test suite (76 tests, 14 files)
```

### HAL Design

Drivers are registered as vtable structs. Swapping hardware means changing the board definition — the kernel and apps are unaffected. Third-party drivers can be distributed as `.drv.elf` files and loaded from SD card at boot.

### Permissions

Each app is granted a bitmask of permissions at registration time:

| Permission | Grants access to |
|------------|-----------------|
| `PERM_RADIO` | LoRa, BLE transmit |
| `PERM_GPS` | GPS location data |
| `PERM_STORAGE` | SD card read/write |
| `PERM_NETWORK` | WiFi, 4G data |
| `PERM_AUDIO` | Speaker, microphone |
| `PERM_SYSTEM` | Display backlight, power control |
| `PERM_IPC` | Inter-app messaging |
| `PERM_ALL` | All of the above |

Unsigned ELF apps loaded from SD card receive only `PERM_IPC` until the user grants additional permissions.

## Apps

| App | Bundle ID | Description |
|-----|-----------|-------------|
| **Launcher** | `com.thistle.launcher` | BlackBerry-style home with favorites dock and app drawer |
| **Settings** | `com.thistle.settings` | WiFi, Bluetooth, Appearance (themes), Drivers (HAL detail), About |
| **File Manager** | `com.thistle.filemgr` | SD card browser with file type indicators |
| **Reader** | `com.thistle.reader` | .txt ebook reader with pagination |
| **Messenger** | `com.thistle.messenger` | Multi-transport chat (LoRa, SMS, BLE, Internet) |
| **Navigator** | `com.thistle.navigator` | GPS dashboard with GPX track recording |
| **Notes** | `com.thistle.notes` | Text editor with auto-save |
| **Assistant** | `com.thistle.assistant` | AI chat interface (Claude API placeholder) |
| **App Store** | `com.thistle.appstore` | Live HTTPS catalog, download + verify + install |
| **WiFi Scanner** | `com.thistle.wifiscanner` | Network scanner with channel utilization |
| **Flashlight** | `com.thistle.flashlight` | Full-screen white + SOS Morse code |
| **Weather** | `com.thistle.weather` | IMU sensor dashboard |
| **Terminal** | `com.thistle.terminal` | System console with built-in commands |
| **Vault** | `com.thistle.vault` | AES-256 encrypted password manager |

## App Store

Catalog hosted at: https://wan0net.github.io/thistle-apps/

Apps, firmware updates, and drivers can be downloaded directly to the device over WiFi or 4G. All downloads are verified with SHA-256 hash and signature before installation.

## Security

- App and driver signature verification (HMAC-SHA256; Ed25519 asymmetric upgrade planned)
- Fine-grained permission system enforced at syscall boundary
- Unsigned apps run in restricted mode (`PERM_IPC` only)
- OTA updates verified before flashing to inactive partition
- Password Vault uses AES-256-CBC + PBKDF2-SHA256 key derivation
- Recovery OS (`ota_0`) verifies main OS signature before booting

## Flash Layout

The 16 MB flash is partitioned as follows:

| Partition | Type | Offset | Size | Purpose |
|-----------|------|--------|------|---------|
| `nvs` | data/nvs | 0x9000 | 24 KB | Non-volatile settings |
| `otadata` | data/ota | 0xF000 | 8 KB | OTA state |
| `phy_init` | data/phy | 0x11000 | 4 KB | RF calibration |
| `ota_0` | app/ota_0 | 0x20000 | 3.5 MB | Recovery OS (Rust) |
| `ota_1` | app/ota_1 | 0x3A0000 | 3.5 MB | Main OS |
| `storage` | data/spiffs | 0x720000 | 8.9 MB | Internal storage |
| `coredump` | data/coredump | 0xFF0000 | 64 KB | Crash dumps |

## Project Stats

- **~29,000 lines of code** across 189 source files
- **76 unit tests** across 14 test files (Unity framework)
- **BSD 3-Clause license** — no GPL dependencies
- 14 driver components, 2 board definitions, 14 built-in apps

## Contributing

Contributions are welcome. Please:

1. Fork the repo
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Follow existing code style: C11, `esp_err_t` returns, `ESP_LOG*` macros, `#pragma once` headers
4. Add tests for new kernel features in `components/test_thistle/`
5. Ensure no GPL-licensed dependencies are introduced
6. Submit a pull request

### Adding a Driver

1. Create `components/drv_<name>/` with `include/`, `src/`, `CMakeLists.txt`
2. Implement the relevant HAL vtable interface from `components/thistle_hal/include/`
3. Register the driver in a board definition (`components/board_*/`)

### Adding an App

1. Add a directory under `components/apps_builtin/`
2. Implement `app_register()`, `app_launch()`, `app_destroy()` entry points
3. Choose a bundle ID in reverse-domain format (`com.yourname.appname`)
4. Call `permissions_grant()` in `main/main.c` with the minimum required permissions

## Roadmap

- [ ] Ed25519 asymmetric signing (replace HMAC-SHA256)
- [ ] Recovery OS build completion (3 Rust type errors remaining)
- [ ] Incremental Rust kernel migration, starting with `app_manager`
- [ ] Full Claude API integration in AI Assistant
- [ ] Online app store with live download progress UI
- [ ] Hardware auto-detection bootloader

## Dependencies and Licenses

See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for full details.

| Dependency | License | Usage |
|------------|---------|-------|
| ESP-IDF | Apache-2.0 | Build system, WiFi, BLE, drivers |
| LVGL 9.2 | MIT | UI framework |
| esp_lvgl_port | Apache-2.0 | LVGL + ESP-IDF integration |
| RadioLib | MIT | SX1262 LoRa driver |
| esp_modem | Apache-2.0 | A7682E 4G PPP networking |
| esp_lcd | Apache-2.0 | ST7789 LCD driver |
| elf_loader | Apache-2.0 | Dynamic ELF app loading |
| NimBLE | Apache-2.0 | BLE stack |
| mbedtls | Apache-2.0 | Cryptography (signing, vault, HTTPS) |
| SDL2 | zlib | Simulator display/input |
| libcurl | MIT/X | Simulator HTTP client |
| FreeRTOS | MIT | Real-time OS kernel |
| esp-idf-hal | MIT/Apache-2.0 | Rust HAL (Recovery OS) |
| esp-idf-svc | MIT/Apache-2.0 | Rust services (Recovery OS) |

## License

BSD 3-Clause License. See [LICENSE](LICENSE).

Copyright (c) 2026, ThistleOS Contributors
