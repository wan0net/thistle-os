# ThistleOS

<p align="center">
  <img src="docs/thistle-logo.svg" alt="ThistleOS Logo" width="200">
</p>

<p align="center">
  <strong>A portable, open-source operating system for ESP32 devices</strong><br>
  One kernel. Any hardware. Apps and drivers delivered over the air.
</p>

<p align="center">
  <a href="#why-thistleos">Why</a> •
  <a href="#how-it-works">How It Works</a> •
  <a href="#the-driver-model">Drivers</a> •
  <a href="#the-app-store">App Store</a> •
  <a href="#getting-started">Get Started</a> •
  <a href="#license">License</a>
</p>

---

> **Alpha Software** — ThistleOS is under active development. The e-paper display renders on the T-Deck Pro but the full UI pipeline is still being tuned. Expect rough edges.

## Why ThistleOS

The ESP32 ecosystem is full of great hardware — T-Deck, T-Beam, M5Stack, Heltec, custom boards — but every project starts from scratch. Different pin assignments, different displays, different radios, all requiring custom firmware.

ThistleOS separates the **kernel** from the **hardware**. The kernel runs the same on every ESP32-S3 device. Drivers are loaded at boot from the SD card. Apps are downloaded from an online store. Update your OS by dropping a file on the SD card or tapping "Update" in Settings.

**The goal:** Flash ThistleOS once. The device figures out the rest.

## How It Works

```
┌─────────────────────────────────────────────┐
│         APPS (.app.elf from SPIFFS/SD)     │
│  Messenger • Reader • Navigator • ...      │
├─────────────────────────────────────────────┤
│         WINDOW MANAGER (.wm.elf)           │
│  Status bar • Launcher • Theme engine      │
│  Widget toolkit (LVGL, Rust UI, terminal)  │
├─────────────────────────────────────────────┤
│         DISPLAY SERVER (kernel)            │
│  Surfaces • Input routing • Compositor     │
├─────────────────────────────────────────────┤
│         KERNEL (100% Rust, immutable)      │
│  App Manager • IPC • Permissions • Events  │
│  Signing • Manifest • Crypto • Syscall table│
├─────────────────────────────────────────────┤
│         HAL (vtable interfaces)            │
│  Display • Input • Radio • GPS • Audio     │
│  Power • IMU • Storage • Network • Crypto  │
├─────────────────────────────────────────────┤
│         DRIVERS (.drv.elf from SPIFFS/SD)  │
│  e-paper • LCD • SX1262 • TCA8418 ...     │
├─────────────────────────────────────────────┤
│         ESP-IDF + FreeRTOS + Hardware      │
└─────────────────────────────────────────────┘
```

ThistleOS uses a three-tier immutable trust chain:

1. **Recovery OS** (Rust, ota_0) — immutable root of trust. Verifies the kernel's Ed25519 signature before booting it.
2. **Kernel** (100% Rust, ota_1) — immutable. Contains the display server, app/driver lifecycle, IPC, permissions, signing, crypto, and HAL. The kernel is hardware-independent: it never calls ESP-IDF hardware APIs directly. All hardware interaction goes through HAL driver vtables. Reads `board.json` from SPIFFS to initialize hardware buses and load drivers.
3. **Everything else** (SPIFFS + SD card) — updateable. Apps, drivers, window managers, and themes are all loaded at runtime. Update any component by replacing its file — no firmware reflash needed.

The kernel never talks to hardware directly. It talks through **HAL vtables** — C structs of function pointers. Drivers are loaded from SPIFFS/SD as `.drv.elf` files and register themselves with the HAL at boot.

**Window managers are swappable** — like Linux desktop environments. The default `lvgl-wm` uses LVGL 9, but you can install a Rust-based WM, a terminal-only WM, or build your own. The display server in the kernel manages surfaces and input routing; the WM draws the UI.

## The Driver Model

Every piece of hardware is abstracted behind a vtable interface:

```c
// The kernel sees this:
const hal_display_driver_t *display = hal_get_registry()->display;
display->flush(area, pixels);  // Works whether it's e-paper, LCD, or OLED

// A driver implements this:
static const hal_display_driver_t my_driver = {
    .init = my_init,
    .flush = my_flush,
    .sleep = my_sleep,
    .width = 320, .height = 240,
    .name = "My Display",
};
```

**Drivers can be compiled into firmware** (for the reference boards) or **loaded from SD card at boot** as `.drv.elf` files. The app store delivers driver updates alongside app updates.

**To support a new board:**
1. Create a board definition (pin assignments, I2C addresses, SPI buses)
2. Pick which drivers to wire up
3. That's it — the kernel, apps, and UI work unchanged

### Supported HAL interfaces

| Interface | What it abstracts | Example drivers |
|-----------|------------------|-----------------|
| `hal_display_driver_t` | Any screen | E-paper (GDEQ031T10), LCD (ST7789 via esp_lcd) |
| `hal_input_driver_t` | Keyboards, touch, trackballs | TCA8418 I2C keypad, CST328 capacitive touch |
| `hal_radio_driver_t` | LoRa, Sub-GHz radios | SX1262 (RadioLib) |
| `hal_gps_driver_t` | Position receivers | U-blox MIA-M10Q (NMEA) |
| `hal_audio_driver_t` | DACs, speakers | PCM5102A (I2S) |
| `hal_power_driver_t` | Battery, charging | TP4065B + ADC |
| `hal_imu_driver_t` | Motion, environment | BHI260AP |
| `hal_storage_driver_t` | SD cards, flash | SDSPI + FATFS |
| `hal_net_driver_t` | Internet connectivity | WiFi, 4G PPP (esp_modem), simulator host |
| `hal_crypto_driver_t` | Crypto acceleration | ESP32-S3 hardware AES/SHA, software fallback |

### Official FOSS upstream drivers

Where possible, ThistleOS wraps official Espressif and community libraries behind HAL vtables rather than rolling custom implementations:

| Library | License | Wraps |
|---------|---------|-------|
| `esp_lcd` (Espressif, built-in) | Apache-2.0 | ST7789 LCD |
| `esp_modem` (Espressif) | Apache-2.0 | A7682E 4G with PPP networking |
| `RadioLib` (jgromes) | MIT | SX1262 LoRa |

## The App Store

ThistleOS has a built-in app store that downloads apps, firmware updates, and drivers from a remote HTTPS catalog.

```
Device                          GitHub Pages (or any HTTPS host)
  │                                │
  ├─ Fetch catalog.json ──────────►│ { entries: [ {type:"app", url:...}, ... ] }
  │                                │
  ├─ Download .app.elf ◄───────────│ Binary + SHA-256 hash
  │                                │
  ├─ Verify signature              │
  ├─ Save to /sdcard/apps/         │
  └─ Launch via ELF loader         │
```

**Three entry types:**
- **Apps** (`.app.elf`) → loaded into PSRAM, runs via ELF loader
- **Firmware** (`.bin`) → flashed to OTA partition, reboots
- **Drivers** (`.drv.elf`) → loaded at boot, registers with HAL

Every download is verified with SHA-256 hash integrity and cryptographic signature. Invalid signatures are rejected — the file is deleted. Unsigned apps run in restricted mode (limited permissions).

**No server infrastructure needed.** The reference catalog is a static JSON file on GitHub Pages: https://wan0net.github.io/thistle-apps/catalog.json

Anyone can host their own catalog by pointing `appstore.json` at a different URL.

## Currently Supported Hardware

### LilyGo T-Deck Pro (primary target)

| Component | Chip | Interface |
|-----------|------|-----------|
| MCU | ESP32-S3FN16R8 (dual-core 240MHz, 16MB flash, 8MB PSRAM) | — |
| Display | 3.1" GDEQ031T10 e-paper (320×240) | SPI |
| Touch | CST328 | I2C |
| Keyboard | TCA8418 matrix scanner | I2C |
| LoRa | SX1262 (868/915 MHz, +22 dBm) | SPI |
| GPS | U-blox MIA-M10Q | UART |
| Audio | PCM5102A I2S DAC | I2S |
| Battery | TP4065B charger + ADC | GPIO |
| IMU | Bosch BHI260AP | I2C |
| Storage | MicroSD | SPI |
| 4G (optional) | Simcom A7682E LTE Cat-1 | UART |
| Connectivity | WiFi 4 + BLE 5.0 (on-chip) | — |

### LilyGo T-Deck (LCD variant)
Same as T-Deck Pro but with ST7789 320×240 TFT LCD instead of e-paper. Different board definition, same kernel.

### Adding your own board
See [CLAUDE.md](CLAUDE.md) for the developer guide. The short version: create a `board_yourdevice/` component with pin definitions and register the appropriate drivers.

## Built-in Apps

ThistleOS ships with 14 apps that demonstrate the platform. All are built-in but follow the same patterns as external apps — they use the syscall table, HAL vtables, and permissions system.

| App | What it does |
|-----|-------------|
| **Launcher** | Home screen with favorites dock + full app drawer |
| **Settings** | WiFi, Bluetooth, Appearance (themes), Drivers (live HAL status), About |
| **File Manager** | SD card browser with directory navigation |
| **Reader** | Plain text ebook reader with pagination |
| **Messenger** | Multi-transport chat (LoRa broadcast, SMS, BLE relay, Internet) |
| **Navigator** | GPS dashboard with GPX track recording |
| **Notes** | Text editor with auto-save |
| **Assistant** | AI chat interface (API integration planned) |
| **App Store** | Browse, download, install apps/firmware/drivers |
| **WiFi Scanner** | Network scanner with signal strength + channel analysis |
| **Flashlight** | Full-screen white + SOS Morse code pattern |
| **Weather** | IMU sensor dashboard (barometer, temperature) |
| **Terminal** | System console with built-in diagnostic commands |
| **Vault** | AES-256 encrypted password manager (PBKDF2 key derivation) |

## Themes

Themes are JSON files on the SD card. Switch instantly in Settings → Appearance.

```json
{
    "name": "link42",
    "colors": {
        "primary": "#2563EB",
        "bg": "#111110",
        "text": "#EDEDED",
        "surface": "#1C1C1B"
    }
}
```

Included themes: Default (monochrome for e-paper), Dark, link42 (dark), link42 Light.

## Network Abstraction

Apps don't call WiFi or 4G directly. They call `net_is_connected()` and `net_get_ip()`. The network manager routes through whichever transport is available:

| Transport | Status | When it's used |
|-----------|--------|---------------|
| WiFi | Working | Primary internet |
| 4G PPP (esp_modem) | Working | Cellular fallback |
| BLE tether | Planned | Phone relay |
| Simulator host | Working | Desktop development |

## Display Server

ThistleOS includes a kernel-level display server that decouples the window manager from the hardware:

```
App → Window Manager → Display Server → HAL → Hardware
```

| Layer | Responsibility | Swappable? |
|-------|---------------|------------|
| **Display Server** | Surface allocation, dirty region tracking, compositor, input routing | No (kernel) |
| **Window Manager** | Status bar, launcher, theme engine, widget toolkit | Yes (.wm.elf) |
| **Apps** | Application UI built on WM's toolkit | Yes (.app.elf) |

The WM is selected in Settings or during first-boot setup. Downloaded from the app store like any other module.

## Security & Chain of Trust

Signing and verification at every level — from boot to apps:

```
┌─────────────────────────────────────────────┐
│  eFuse (optional, production only)          │
│  Burns Recovery public key hash — permanent │
├─────────────────────────────────────────────┤
│  Recovery OS (ota_0) — IMMUTABLE            │
│  Holds Ed25519 public key                   │
│  Verifies ThistleOS firmware signature      │
│  before allowing it to boot                 │
├─────────────────────────────────────────────┤
│  ThistleOS (ota_1) — SIGNED                │
│  Verifies app + driver ELF signatures       │
│  before loading into PSRAM                  │
├─────────────────────────────────────────────┤
│  Apps & Drivers — SIGNED                    │
│  SHA-256 integrity hash verified on download│
│  Signature verified before execution        │
│  Unsigned code runs in restricted sandbox   │
├─────────────────────────────────────────────┤
│  Permissions                                │
│  Signed apps: full permissions              │
│  Unsigned apps: PERM_IPC only               │
│  Each app declares required capabilities    │
└─────────────────────────────────────────────┘
```

| Layer | What's verified | What happens on failure |
|-------|----------------|----------------------|
| **Recovery → Firmware** | Ed25519 signature on `thistle_os.bin` | Refuses to boot; enters recovery mode |
| **Firmware → Apps** | SHA-256 hash + signature on `.app.elf` | Invalid sig = file deleted. Missing sig = restricted mode |
| **Firmware → Drivers** | SHA-256 hash + signature on `.drv.elf` | Invalid sig = refused. Missing sig = warning + load |
| **App Store → Downloads** | SHA-256 verified during download stream | Mismatch = download deleted, never installed |
| **OTA Updates** | Signature checked before writing to flash | Invalid = update rejected |
| **Password Vault** | AES-256-CBC + HMAC-SHA256 integrity | Tampered vault = decryption fails |

**Key management:**
- The **developer** holds the Ed25519 private key (never on-device)
- The **device** holds only the public key (embedded in Recovery firmware)
- The device **cannot forge signatures** — it can only verify them
- Cryptography uses **ed25519-dalek** (Rust) for signing and **mbedtls** (ESP-IDF) for TLS; symmetric crypto (AES-256, HMAC-SHA256, PBKDF2) goes through the kernel crypto module

**Kernel crypto module:**
The kernel contains a platform-independent crypto layer (`components/kernel_rs/src/crypto.rs`). It dispatches through the `hal_crypto_driver_t` vtable first — on ESP32-S3 this can use the hardware AES and SHA accelerators. When no hardware crypto driver is registered (simulator, WASM, or boards without hardware crypto), it falls back to pure Rust software implementations transparently. The Vault app uses this kernel crypto on all platforms, including the SDL2 simulator and the planned WASM web simulator.

**eFuse burning is NEVER done by default.** It's an optional, irreversible step for production devices only. Software-only signing provides strong security without hardware lock-in.

## Recovery OS (WIP)

A minimal Rust firmware for ota_0 that provides unbreakable recovery:

1. Checks ota_1 → boots if valid
2. Checks SD card → flashes firmware if found
3. Starts WiFi hotspot → user connects phone → captive portal web UI
4. Downloads firmware from app store → flashes → reboots

Written in Rust using `esp-idf-hal` + `esp-idf-svc`. Works on any ESP32-S3 — no board-specific drivers needed (WiFi is on-chip).

## Simulator

Develop and test without hardware:

```bash
cd simulator && mkdir build && cd build
cmake .. && make -j8 && ./thistle_sim
```

The simulator runs the **real kernel and app code** in an SDL2 window with:
- Fake WiFi networks (scan + connect)
- BLE state machine
- SD card mapped to local filesystem
- Real HTTP via libcurl (app store works!)
- Host system clock

## Getting Started

### Prerequisites
- [ESP-IDF v5.5](https://docs.espressif.com/projects/esp-idf/en/latest/esp32s3/get-started/)

### Build & Flash
```bash
git clone https://github.com/wan0net/thistle-os.git
cd thistle-os

. ~/esp/esp-idf/export.sh
idf.py set-target esp32s3
idf.py build
idf.py -p /dev/ttyACM0 flash monitor
```

### Run Simulator (macOS/Linux)
```bash
brew install sdl2 pkg-config  # macOS
cd simulator && mkdir build && cd build
cmake .. && make -j8 && ./thistle_sim
```

## Project Stats

| Metric | Value |
|--------|-------|
| C source code | ~15,000 lines |
| Rust kernel code | ~3,500 lines |
| Source files | 160+ |
| Built-in apps | 14 |
| HAL drivers | 12 |
| Board configs | 2 (T-Deck Pro, T-Deck) |
| Unit tests | 80+ (C) + 66 (Rust) |
| Firmware binary | ~1.6 MB |
| Commits | 75+ |
| License | BSD 3-Clause |
| Dependencies | All BSD/MIT/Apache-2.0 (no GPL) |

## Contributing

Contributions welcome — especially:
- **New board definitions** (your ESP32 device)
- **New drivers** (implement a HAL vtable)
- **New apps** (use the app SDK)
- **Bug reports** and security reviews

See [CLAUDE.md](CLAUDE.md) for architecture details and coding conventions.

## Roadmap

### Completed
- [x] Ed25519 asymmetric signing (Monocypher)
- [x] Recovery OS (Rust, compiles clean)
- [x] 100% Rust kernel (20 modules, 66 tests)
- [x] Unified manifest system for apps, drivers, firmware
- [x] Boot-from-JSON (board.json driven hardware init)
- [x] Display server with swappable window managers
- [x] Driver SDK (C and Rust templates)
- [x] Expanded syscall table (45 ESP-IDF APIs for runtime drivers)
- [x] Hardware bringup on T-Deck Pro (e-paper, keyboard, touch working)
- [x] App loading infrastructure (SPIFFS + SD card scanner)
- [x] Widget API syscall table (31 functions)

### In Progress
- [ ] Compile existing drivers as standalone .drv.elf files
- [ ] Move built-in apps to .app.elf on SPIFFS
- [ ] Wire display server into boot sequence
- [ ] E-paper display rotation and LVGL rendering

### Planned
- [ ] First-boot setup wizard (board detection, WM selection, WiFi)
- [ ] LVGL window manager as loadable .wm.elf
- [ ] Rust window manager (embedded-graphics based)
- [ ] Permission enforcement at syscall boundary
- [ ] Claude API integration in AI assistant
- [ ] Hardware auto-detection bootloader
- [ ] More board support (T-Beam, M5Stack, Heltec)
- [ ] WASM web simulator with terminal + app store
- [ ] Async event dispatch and per-app IPC queues

## Dependencies

All dependencies are permissively licensed. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

| Dependency | License | Role |
|-----------|---------|------|
| ESP-IDF | Apache-2.0 | Platform SDK |
| LVGL 9 | MIT | UI framework |
| RadioLib | MIT | LoRa radio |
| esp_modem | Apache-2.0 | 4G PPP |
| esp_lcd | Apache-2.0 | LCD display |
| NimBLE | Apache-2.0 | BLE |
| ed25519-dalek | BSD-3-Clause/MIT | Ed25519 signing |
| sha2 | MIT/Apache-2.0 | SHA-256 hashing |
| mbedtls | Apache-2.0 | TLS, AES, hashing |
| aes | MIT/Apache-2.0 | Rust software AES-256 |
| hmac | MIT/Apache-2.0 | Rust software HMAC |
| pbkdf2 | MIT/Apache-2.0 | Rust software PBKDF2 |
| getrandom | MIT/Apache-2.0 | Rust CSPRNG entropy |
| FreeRTOS | MIT | RTOS kernel |
| SDL2 | zlib | Simulator |
| libcurl | MIT | Simulator HTTP |

## License

BSD 3-Clause License. See [LICENSE](LICENSE).

---

<p align="center">
  <em>ThistleOS is named after the thistle — Scotland's national flower. Tough, resilient, and thriving everywhere.</em>
</p>
