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

> **Beta Software** — Rust migration complete. 100% Rust kernel (40+ modules, 26,000+ LOC, 489+ tests), 14 Rust hardware drivers, multi-board support (T-Deck Pro, T-Deck, T-Display-S3, T3-S3, CYD, C3-Mini), and multi-arch builds (ESP32, S2, S3, C3, C6, H2). Recovery auto-detects hardware via I2C/SPI/UART scanning.

## Why ThistleOS

The ESP32 ecosystem is full of great hardware — T-Deck, T-Beam, M5Stack, Heltec, custom boards — but every project starts from scratch. Different pin assignments, different displays, different radios, all requiring custom firmware.

ThistleOS separates the **kernel** from the **hardware**. The kernel runs the same on every ESP32 device — ESP32, S2, S3, C3, C6, H2. Drivers are loaded at boot from the SD card. Apps are downloaded from an online store. Update your OS by dropping a file on the SD card or tapping "Update" in Settings.

**The goal:** Flash ThistleOS once. The device figures out the rest.

## How It Works

```
┌─────────────────────────────────────────────┐
│         APPS (.app.elf from SPIFFS/SD)     │
│  Messenger • Reader • Navigator • ...      │
├─────────────────────────────────────────────┤
│         WINDOW MANAGER (.wm.elf)           │
│  Status bar • Launcher • Theme engine      │
│  thistle-tk (Rust, default) | LVGL (C, LCD fallback)  │
├─────────────────────────────────────────────┤
│         DISPLAY SERVER (kernel)            │
│  Surfaces • Input routing • Compositor     │
├─────────────────────────────────────────────┤
│         KERNEL (100% Rust, immutable)      │
│  App Manager • IPC • Permissions • Events  │
│  Signing • Manifest • Crypto • Syscall table│
│  40+ modules • 26,000+ LOC • 489+ tests    │
├─────────────────────────────────────────────┤
│         HAL (vtable interfaces)            │
│  Display • Input • Radio • GPS • Audio     │
│  Power • IMU • Storage • Network • Crypto  │
│  RTC                                       │
├─────────────────────────────────────────────┤
│         DRIVERS (.drv.elf from SPIFFS/SD)  │
│  14 Rust drivers: e-paper • LCD • OLED     │
│  TCA8418 • CST328/816 • SX1262 • GPS       │
│  IMU • Power • Audio • RTC • SD card       │
├─────────────────────────────────────────────┤
│         ESP-IDF + FreeRTOS + Hardware      │
│  ESP32 • ESP32-S2 • ESP32-S3 (Xtensa)     │
│  ESP32-C3 • ESP32-C6 • ESP32-H2 (RISC-V) │
└─────────────────────────────────────────────┘
```

ThistleOS uses a three-tier immutable trust chain:

1. **Recovery OS** (Rust, ota_0) — immutable root of trust. Verifies the kernel's Ed25519 signature before booting it.
2. **Kernel** (100% Rust, ota_1) — immutable. Contains the display server, app/driver lifecycle, IPC, permissions, signing, crypto, and HAL. The kernel is hardware-independent: it never calls ESP-IDF hardware APIs directly. All hardware interaction goes through HAL driver vtables. Reads `board.json` from SPIFFS to initialize hardware buses and load drivers.
3. **Everything else** (SPIFFS + SD card) — updateable. Apps, drivers, window managers, and themes are all loaded at runtime. Update any component by replacing its file — no firmware reflash needed.

The kernel never talks to hardware directly. It talks through **HAL vtables** — C structs of function pointers. Drivers are loaded from SPIFFS/SD as `.drv.elf` files and register themselves with the HAL at boot.

**Window managers are swappable** — like Linux desktop environments. The default WM uses **thistle-tk** (pure Rust, embedded-graphics) for e-paper, with LVGL 9 available as a fallback for LCD. You can also install a terminal-only WM or build your own. The display server in the kernel manages surfaces and input routing; the WM draws the UI.

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
| `hal_display_driver_t` | Any screen | E-paper (GDEQ031T10), LCD (ST7789), OLED (SSD1306) |
| `hal_input_driver_t` | Keyboards, touch, trackballs | TCA8418 I2C keypad, CST328/CST816S capacitive touch |
| `hal_radio_driver_t` | LoRa, Sub-GHz radios | SX1262 (RadioLib) |
| `hal_gps_driver_t` | Position receivers | U-blox MIA-M10Q (NMEA) |
| `hal_audio_driver_t` | DACs, speakers | PCM5102A (I2S) |
| `hal_power_driver_t` | Battery, charging | TP4065B + ADC |
| `hal_imu_driver_t` | Motion, environment | QMI8658C 6-axis IMU, BHI260AP sensor hub |
| `hal_storage_driver_t` | SD cards, flash | SDSPI + FATFS |
| `hal_net_driver_t` | Internet connectivity | WiFi, 4G PPP (esp_modem), simulator host |
| `hal_crypto_driver_t` | Crypto acceleration | ESP32-S3 hardware AES/SHA, software fallback |
| `hal_rtc_driver_t` | Real-time clock | PCF8563 RTC |

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

## Supported Devices

See the full [Supported Devices](https://wan0net.github.io/esp32-os/docs/devices.html) page for component-level hardware details and driver tables.

| Device | Chip | Display | Status |
|--------|------|---------|--------|
| LilyGo T-Deck Pro | ESP32-S3 | 3.1" GDEQ031T10 e-paper (240×320) | Primary target |
| LilyGo T-Deck | ESP32-S3 | ST7789 LCD (320×240) | Supported |
| LilyGo T-Display-S3 | ESP32-S3 | ST7789 LCD (170×320) | Supported |
| LilyGo T3-S3 | ESP32-S3 | SSD1306 OLED (128×64) | Supported |
| CYD ESP32-2432S028 | ESP32 | ILI9341 LCD (320×240) | Supported |
| ESP32-C3 SuperMini | ESP32-C3 | SSD1306 OLED (128×64) | Supported |

Multi-arch: ESP32 (Xtensa), ESP32-S2/S3 (Xtensa LX7), ESP32-C3/C6/H2 (RISC-V). One firmware binary per architecture. Hardware drivers auto-detected and downloaded by Recovery OS.

### LilyGo T-Deck Pro (primary target)

| Component | Chip | Interface |
|-----------|------|-----------|
| MCU | ESP32-S3FN16R8 (dual-core 240MHz, 16MB flash, 8MB PSRAM) | — |
| Display | 3.1" GDEQ031T10 e-paper (240×320) | SPI |
| Touch | CST328 capacitive | I2C |
| Keyboard | TCA8418 matrix scanner | I2C |
| LoRa | SX1262 (868/915 MHz, +22 dBm) | SPI |
| GPS | U-blox MIA-M10Q GNSS | UART |
| Audio | PCM5102A I2S DAC | I2S |
| Battery | TP4065B charger + ADC | GPIO/ADC |
| IMU | QMI8658C 6-axis + BHI260AP hub | I2C |
| Light | LTR-553 ambient light sensor | I2C |
| RTC | PCF8563 real-time clock | I2C |
| Storage | MicroSD | SPI |
| 4G (optional) | Simcom A7682E LTE Cat-1 | UART |
| Connectivity | WiFi 4 + BLE 5.0 (on-chip) | — |

### Adding your own board
See the [Board Support](https://wan0net.github.io/esp32-os/docs/board-support.html) docs. Create a `board.json` with pin assignments and a driver list — the kernel reads it at boot. No recompilation needed for new boards.

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

## thistle-tk

**thistle-tk** is the default window manager for e-paper displays. It is a pure Rust widget toolkit built on **embedded-graphics** with zero C dependencies.

- **Repo:** https://github.com/wan0net/thistle-tk
- Works on both 1-bit e-paper (`BinaryColor`) and RGB565 LCD (`Rgb565`)
- Apps use semantic widgets (`Container`, `Label`, `Button`, `TextInput`) and theme colors
- Layout engine with flexbox-like positioning
- The kernel's `tk_wm.rs` integrates thistle-tk as a display server window manager, and `tk_launcher.rs` implements the home screen launcher on top of it

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

## Recovery OS

A minimal Rust firmware for ota_0 that provides unbreakable recovery and hardware self-provisioning:

1. Detects chip type (ESP32/S3/C3/etc.)
2. Checks ota_1 → boots if valid
3. Checks SD card → flashes firmware if found
4. Enables power rails → scans I2C bus (0x08–0x77), probes SPI/UART devices
5. Matches detected components to catalog — downloads matching drivers + firmware + WM
6. Starts WiFi hotspot → user connects phone → 3-step captive portal web UI
7. Reboots into a fully provisioned ThistleOS

Written in Rust using `esp-idf-hal` + `esp-idf-svc`. Works on any ESP32 variant — no board-specific drivers needed (WiFi is on-chip).

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
- Rust toolchain with `xtensa-esp32s3-espidf` target (for Recovery OS): `cargo install espup && espup install`

### Build & Flash

**Option A — Flash Recovery OS and let it self-provision:**
```bash
cd recovery
cargo build --release
espflash flash target/xtensa-esp32s3-espidf/release/thistleos-recovery
# Connect to "ThistleOS-Recovery" WiFi → follow captive portal
```

**Option B — Build the full firmware directly:**
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
| Rust kernel code | 26,000+ lines |
| Kernel modules | 40+ |
| Kernel tests | 489+ |
| Rust drivers | 14 |
| Built-in apps | 14 |
| HAL interfaces | 11 (display, input, radio, GPS, audio, power, IMU, storage, net, crypto, RTC) |
| Supported boards | 6 (T-Deck Pro, T-Deck, T-Display-S3, T3-S3, CYD, C3-Mini) |
| Supported architectures | 6 (ESP32, S2, S3, C3, C6, H2) |
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
- [x] Recovery OS (Rust) with hardware auto-detection (I2C/SPI/UART scanning)
- [x] 100% Rust kernel (40+ modules, 26,000+ LOC, 489+ tests)
- [x] 14 Rust hardware drivers
- [x] Multi-board support (T-Deck Pro, T-Deck, T-Display-S3, T3-S3, CYD, C3-Mini)
- [x] Multi-arch builds (ESP32, S2, S3, C3, C6, H2)
- [x] Unified manifest system for apps, drivers, firmware
- [x] Boot-from-JSON (board.json driven hardware init)
- [x] Display server with swappable window managers
- [x] Driver SDK (C and Rust templates)
- [x] Expanded syscall table (45 ESP-IDF APIs for runtime drivers)
- [x] Hardware bringup on T-Deck Pro (e-paper, keyboard, touch working)
- [x] App loading infrastructure (SPIFFS + SD card scanner)
- [x] App store with ratings, categories, download counts
- [x] thistle-tk: Pure Rust widget toolkit replacing LVGL for e-paper
- [x] Rust launcher app running on thistle-tk WM
- [x] RTC HAL interface (PCF8563)
- [x] Component-level driver detection (not board-level)

### In Progress
- [ ] Port remaining apps from C/LVGL to Rust/thistle-tk
- [ ] Compile existing drivers as standalone .drv.elf files
- [ ] Move built-in apps to .app.elf on SPIFFS
- [ ] Wire display server into boot sequence

### Planned
- [ ] Permission enforcement at syscall boundary
- [ ] Claude API integration in AI assistant
- [ ] WASM web simulator with terminal + app store
- [ ] Async event dispatch and per-app IPC queues
- [ ] More board support (T-Beam, M5Stack, Heltec)

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
| embedded-graphics | MIT/Apache-2.0 | Rust 2D graphics primitives |
| thistle-tk | BSD-3-Clause | Widget toolkit (embedded-graphics based) |
| FreeRTOS | MIT | RTOS kernel |
| SDL2 | zlib | Simulator |
| libcurl | MIT | Simulator HTTP |

## License

BSD 3-Clause License. See [LICENSE](LICENSE).

---

<p align="center">
  <em>ThistleOS is named after the thistle — Scotland's national flower. Tough, resilient, and thriving everywhere.</em>
</p>
