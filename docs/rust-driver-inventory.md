# Rust driver inventory and reuse assessment

Status: living inventory

Last reviewed: 2026-08-02

This document records the Rust drivers that exist inside ThistleOS and the
permissively licensed Rust implementations that may be reused when drivers are
moved into signed standalone `.drv.elf` packages. It also records gaps and
rejected sources so future work does not repeat the same search or accidentally
introduce an incompatible dependency.

Crate versions, repository state, and licenses below are a point-in-time
snapshot. Recheck all of them, pin exact versions or Git revisions, inspect
transitive dependencies, and perform device-level validation before adoption.

## Meaning of the classifications

- **Local substantial** means the repository contains meaningful protocol or
  hardware logic and host tests. It does not mean the code has been packaged as
  a standalone driver or verified on every named board.
- **Local prototype** means useful bring-up code exists but has narrow tests or
  incomplete lifecycle/integration work.
- **Local stub** means the public shape exists but real hardware operations are
  deliberately unimplemented.
- **Primary candidate** means a permissively licensed Rust project appears to
  fit the device and should be evaluated before maintaining a parallel local
  implementation.
- **Generic transport only** means the physical device needs little or no
  controller-specific crate, but Thistle still needs safe I2S, PDM, UART, ADC,
  GPIO, DMA, power, or bus integration.
- **No candidate confirmed** means no reusable permissively licensed pure-Rust
  implementation was found in this review. It is not proof that none exists.

“Rust driver” is intentionally split into two notions:

1. Rust source that calls ESP-IDF through FFI, which describes most current
   Thistle drivers.
2. Portable `no_std` Rust built on `embedded-hal` or equivalent traits, which is
   preferred for standalone driver protocol logic.

The latter still needs a Thistle adapter. It must not bypass kernel ownership of
SPI, QSPI, I2C, UART, GPIO, DMA, power rails, or interrupts.

## Current ThistleOS Rust modules

All modules in this table currently live in `components/kernel_rs/src/`. They
are compiled into the kernel and expose the current C-compatible HAL vtables.
They are not, merely by existing here, the signed standalone `.drv.elf` files
named by board manifests.

The test counts are source-level `#[test]` counts observed on 2026-08-02. Host
tests commonly use deterministic ESP-IDF stubs, so they do not constitute
hardware verification.

| Module | Device / role | Assessment | Source tests | Reuse direction |
| --- | --- | --- | ---: | --- |
| `drv_accel_qmi8658.rs` | QMI8658C accelerometer/gyro | Local substantial | 32 | Compare with `ph-qmi8658`; retain tested calibration and HAL behaviour |
| `drv_audio_pcm5102a.rs` | PCM5102A I2S DAC | Local substantial | 29 | Controller needs generic I2S rather than a complex device crate |
| `drv_display_co5300.rs` | CO5300 QSPI AMOLED | Local prototype | 2 | Replace/converge on `decaday/display-driver`; do not maintain two stacks |
| `drv_epaper_gdeq031t10.rs` | GDEQ031T10/UC8253 e-paper | Local substantial | 20 | Keep separate e-paper model; assess `epd-waveshare` only for reusable pieces |
| `drv_gps_mia_m10q.rs` | u-blox MIA-M10Q GNSS | Local substantial | 42 | Compare parsing/configuration with `ublox`; retain board power/UART integration |
| `drv_imu_bhi260ap.rs` | Bosch BHI260AP smart IMU | **Local stub** | 13 | Real implementation still required; no permissive Rust candidate confirmed |
| `drv_kbd_cardkb.rs` | CardKB I2C keyboard | Local substantial | 16 | Simple protocol; local implementation may remain smaller than a dependency |
| `drv_kbd_tca8418.rs` | TCA8418 keypad scanner | Local substantial | 12 | Evaluate `tca8418` and preserve Thistle event/lifecycle semantics |
| `drv_lcd_ili9341.rs` | ILI9341 SPI LCD | Local substantial | 23 | Move to the new colour stack using `mipidcs` or another compatible panel implementation |
| `drv_lcd_st7789.rs` | ST7789 SPI LCD | Local substantial | 20 | Use as first migration board; upstream `display-driver-st7789` is the preferred panel layer |
| `drv_light_ltr553.rs` | LTR-553ALS light/proximity | Local substantial | 13 | No external candidate confirmed; extract local protocol logic from ESP-IDF FFI |
| `drv_oled_ssd1306.rs` | SSD1306 monochrome OLED | Local substantial | 22 | Evaluate the mature `ssd1306` crate; keep monochrome ABI separate initially |
| `drv_power_tp4065b.rs` | TP4065B charger/battery sensing | Local substantial | 27 | Generic ADC/GPIO implementation is adequate; no complex controller crate needed |
| `drv_rtc_pcf8563.rs` | PCF8563 RTC | Local substantial | 25 | Evaluate `pcf8563`; do not confuse it with T-Watch Ultra's PCF85063A |
| `drv_sdcard.rs` | SPI MicroSD and FAT mount | Local substantial | 21 | Decide whether ESP-IDF VFS or `embedded-sdmmc` owns filesystem duties |
| `drv_touch_cst328.rs` | CST328 capacitive touch | Local substantial | 10 | Evaluate `cst328`; retain board calibration and event mapping |
| `drv_touch_cst816.rs` | CST816S capacitive touch | Local substantial | 14 | Evaluate `cst816s` or `cst816s-async` |
| `drv_touch_cst9217.rs` | CST9217 capacitive touch | Local prototype | 2 | Evaluate `cst92xx`; fix T-Watch Ultra address and verify packets on hardware |
| `drv_touch_ft3x68.rs` | FT3x68/FT3168 capacitive touch | Local prototype | 1 | No exact external candidate selected; expand protocol and lifecycle tests |
| `drv_touch_xpt2046.rs` | XPT2046 resistive touch | Local substantial | 31 | Evaluate `xpt2046`/`xpt2046-async`; retain calibration and filtering policy |

### Important local caveats

- Most local modules call ESP-IDF functions directly. That is acceptable as
  bring-up evidence but conflicts with the target in which the kernel is
  hardware-independent and loaded drivers consume kernel-owned bus services.
- CO5300 and CST9217 are substantial enough to compare against upstream code,
  but too narrow to call finished. They each had only two source tests at this
  review.
- BHI260AP's tests validate its stub contract. Real operations intentionally
  return `ESP_ERR_NOT_SUPPORTED`; test count must not be mistaken for device
  support.
- Several C driver components remain as fallbacks or parallel implementations.
  Migration should select one implementation path per device and then remove
  duplicates after hardware parity.

## External Rust candidates

### Colour display controllers

| Device | Candidate | Reviewed version | License | Assessment |
| --- | --- | ---: | --- | --- |
| CO5300 | [`display-driver-co5300`](https://github.com/decaday/display-driver) | 0.1.1 | Apache-2.0 | Primary panel candidate; contains the 410 x 502 T-Watch Ultra panel specification |
| ST7789 | [`display-driver-st7789`](https://github.com/decaday/display-driver) | 0.1.0 | Apache-2.0 | Primary candidate and simpler first migration path |
| ILI9341 | `decaday/display-driver` `mipidcs` plus a new panel specification | Git snapshot | Apache-2.0 | Preferred architectural fit; implementation still required |
| ST7735 / GC9A01 | `decaday/display-driver` panel crates | 0.1-era | Apache-2.0 | Available upstream; adopt only for an actual supported board |

`decaday/display-driver` is adopted as the direction for colour LCD/AMOLED
drivers, subject to the design and maturity conditions in
[`display-driver-architecture.md`](display-driver-architecture.md). It does not
currently provide the complete ESP32-S3 QSPI/DMA transport.

### E-paper and monochrome displays

| Device | Candidate | Reviewed version | License | Assessment |
| --- | --- | ---: | --- | --- |
| GDEQ031T10 / UC8253 | [`epd-waveshare`](https://github.com/Caemor/epd-waveshare) | 0.6.0 | ISC | Evaluate controller/panel coverage; do not assume exact GDEQ031T10 support |
| SSD1306 | [`ssd1306`](https://github.com/rust-embedded-community/ssd1306) | 0.10.0 | MIT OR Apache-2.0 | Strong reusable candidate; separate buffered monochrome lifecycle remains a Thistle decision |

E-paper is deliberately not folded into the colour-display framework merely to
reduce the number of interfaces. Its explicit refresh modes, busy waits,
waveforms, ghosting policy, and possible previous-frame storage are different
requirements.

### Touch and keyboard input

| Device | Candidate | Reviewed version | License | Assessment |
| --- | --- | ---: | --- | --- |
| TCA8418 | [`tca8418`](https://github.com/LarsBollmann/tca8418) | 0.2.2 | MIT OR Apache-2.0 | Primary candidate for keypad register logic |
| CardKB | Local protocol | n/a | BSD-3-Clause | One-byte I2C protocol is simple; external dependency is not required |
| CST816S | [`cst816s`](https://github.com/tstellanova/cst816s) | 1.0.1 | BSD-3-Clause | Primary blocking candidate; async alternative also exists |
| CST328 | [`cst328`](https://github.com/cmumford/cst328) | 1.0.4 | MIT | Primary candidate; compare coordinate packet and reset behaviour with local code |
| CST9217/CST9220 | [`cst92xx`](https://github.com/ScripTerasu/cst92xx-touch-driver) | 0.1.0 | MIT | Primary candidate, but its default `0x5A` must not override T-Watch hardware evidence at `0x1A` |
| XPT2046 | [`xpt2046`](https://github.com/VersBinarii/xpt2046) | 0.3.0 | MIT OR Apache-2.0 | Candidate protocol layer; Thistle retains calibration/filtering and display transform |
| FT3x68/FT3168 | No exact candidate selected | n/a | n/a | Continue local implementation or perform a focused compatibility search |

Touch drivers report controller coordinates; the display/input transform and
orientation agreement remain Thistle policy. An upstream crate must not embed a
board-specific rotation that disagrees with the compositor.

### IMU, light, RTC, power, and haptics

| Device | Candidate | Reviewed version | License | Assessment |
| --- | --- | ---: | --- | --- |
| QMI8658C | [`ph-qmi8658`](https://github.com/photon-circus/ph-qmi8658-imu) | 0.1.1 | MIT | Async `no_std` candidate; currently declares Rust 1.92, so toolchain compatibility must be checked |
| BHI260AP | No permissive reusable implementation confirmed | n/a | n/a | Major remaining implementation gap; includes firmware upload and Bosch host protocol work |
| LTR-553ALS | No reusable candidate confirmed | n/a | n/a | Extract and test the local register logic |
| PCF8563 | [`pcf8563`](https://github.com/nebelgrau77/pcf8563-rs) | 0.2.1 | MIT OR Apache-2.0 | Candidate for existing boards |
| PCF85063A | [`pcf85063a`](https://github.com/tweedegolf/pcf85063a) | 0.1.1 | MIT OR Apache-2.0 | Correct RTC candidate for T-Watch Ultra |
| AXP2101 | [`axp2101-embedded`](https://github.com/trevorflahardy/axp2101-embedded) | 0.3.0 | MIT | Primary broad PMU candidate |
| AXP2101 | [`axp2101-dd`](https://github.com/okhsunrog/axp2101-dd) | 0.2.3 | MIT OR Apache-2.0 | Alternative using `device-driver`; compare API coverage and generated-code cost |
| XL9555 | [`xl9555-hal`](https://github.com/mo7984130/xl9555-hal) | 0.3.0 | MIT OR Apache-2.0 | Primary GPIO-expander candidate |
| DRV2605 | [`drv2605-async`](https://github.com/AtovProject/drv2605-async) | 0.1.0 | MIT OR Apache-2.0 | Preferred async haptic candidate; Thistle still enforces effect and duration limits |
| TP4065B | Generic ADC/GPIO | n/a | n/a | Device does not justify a protocol crate; retain safe local measurements and policy |

The `agrucza/uhrwerk-rs` project contains useful-looking pure-Rust drivers for a
similar Waveshare AMOLED device, including BHI260AP-related and PDM register
work, but the repository is GPL-3.0. It may be used only as evidence that an
approach exists, not copied, adapted, linked, or used as a dependency under
ThistleOS's no-GPL policy.

### Radio

| Device | Candidate | Reviewed version | License | Assessment |
| --- | --- | ---: | --- | --- |
| SX1261/SX1262 | [`sx126x`](https://github.com/tweedegolf/sx126x-rs) | 0.3.0 | MIT OR Apache-2.0 | Primary focused candidate |
| SX1261/SX1262 | [`sx1262`](https://github.com/BroderickCarlin/SX1261) | 0.3.0 | MIT OR Apache-2.0 | Alternative; compare completeness and maintenance |
| SX126x and SX128x | [`semtech_radios`](https://github.com/David-OConnor/semtech-radios) | 0.1.6 | MIT | Attractive common implementation; assess platform coupling and feature coverage |
| SX1280 | [`radio-sx128x`](https://github.com/rust-iot/rust-radio-sx128x) | 0.19.0 | MPL-2.0 | Available but less preferred than a simpler MIT/Apache candidate and defaults need review |
| CC1101 | [`cc1101`](https://github.com/dsvensson/cc1101) | 0.1.3 | Apache-2.0 | Viable focused candidate for that radio variant |
| LR1120/LR1121 | [`lr11xx`](https://github.com/quartiq/lr11xx) | 0.1.0 | MIT OR Apache-2.0 | Viable early candidate; hardware scope and completeness require audit |
| SI4432 | No current crates.io candidate found | n/a | n/a | Focused search or local implementation required |

Radio selection is component-specific, not merely board-name-specific. T-Watch
Ultra and related LilyGo devices ship in multiple radio variants. Detection,
signed provisioning metadata, or explicit confirmed selection must choose the
matching package; a failed probe must not silently fall back to SX1262.

### GNSS and storage

| Device / role | Candidate | Reviewed version | License | Assessment |
| --- | --- | ---: | --- | --- |
| u-blox MIA-M10Q | [`ublox`](https://github.com/ublox-rs/ublox) | 0.10.0 | MIT | Strong UBX protocol/parser candidate; declared Rust 1.88 must match the ESP toolchain |
| MicroSD over SPI | [`embedded-sdmmc`](https://github.com/rust-embedded-community/embedded-sdmmc-rs) | 0.9.0 | MIT OR Apache-2.0 | Strong portable block/filesystem candidate, but ESP-IDF VFS integration may still be preferable initially |

The storage driver should mount the selected external system volume at a stable
logical location such as `/system`. SD is the first backend, not part of the
kernel contract; a future NVMe or other block device can provide the same role.

### Audio

| Device | Candidate | Assessment |
| --- | --- | --- |
| PCM5102A | Generic I2S output | No controller protocol crate is required; configure I2S format, DMA, volume policy, and power safely |
| MAX98357A | Generic I2S output | Same principle; it is not PCM5102A and needs the correct speaker rail and gain policy |
| T3902 microphone | Generic PDM capture, but no complete Thistle-ready Rust path confirmed | Requires a safe ESP32 PDM input transport, DMA ownership, backpressure, bounded capture, and unload semantics |

No device-specific MAX98357A or T3902 crate was found in this review. That is not
necessarily a blocker: the amplifier and microphone expose standard serial
audio transports. The blocker is a production-quality Rust/Thistle transport,
especially PDM input. The reviewed pure-Rust `esp-hal` I2S path did not provide
the required PDM capture support at the time of the discussion, so this must be
rechecked rather than assumed solved.

### Cellular modem

| Device | Candidate | Assessment |
| --- | --- | --- |
| SIMCom A7682E / A76xx family | `a76xx` 0.0.0 | Reserved/embryonic crate, not a production driver |
| Generic AT command handling | [`atat`](https://github.com/BlackbirdHQ/atat) | Useful Rust parser/framework candidate, but Thistle still needs A7682E commands, UART ownership, state machine, networking integration, SMS/call policy, and recovery behaviour |

The current A7682E path relies on `esp_modem` and C/C++ integration. There is no
confirmed production-ready pure-Rust replacement in this inventory. T-Deck
variants must model the modem and audio hardware as components: a Plus variant
may contain one of those options, while a Max variant can contain both. Do not
infer capabilities from a marketing family name alone.

### NFC

| Device | Candidate | Version | License | Assessment |
| --- | --- | ---: | --- | --- |
| ST25R3916 | [`embassy-rs/rnfc`](https://github.com/embassy-rs/rnfc), crate `rnfc-st25r39` | Git 0.1.0 | MIT OR Apache-2.0 at crate level | Primary candidate; `no_std`, embedded-hal 1.0, ST25R3916 register identity, and ISO14443-A lower layer |
| ISO14443/NDEF higher layer | Thistle code or permissive rnfc-compatible crate | undecided | Must be permissive | NDEF support and the narrow app-facing policy may still need implementation |

`rnfc-st25r39` is not a stable crates.io release; the published name is only a
`0.0.0` reservation. Pin an audited Git commit or vendor it reproducibly.

The crates.io [`iso14443`](https://github.com/Foundation-Devices/iso14443-rs)
crate is GPL-3.0-or-later and is excluded. It must not be linked, copied, or used
as the implementation basis for ThistleOS.

## Board-focused coverage

### T-Deck Pro

The current profile needs these components:

- GDEQ031T10 e-paper: substantial local Rust implementation; keep separate from
  the new colour stack.
- TCA8418 keyboard: local Rust plus permissive upstream candidate.
- CST328 touch: local Rust plus MIT upstream candidate.
- SX1262 radio: external pure-Rust candidates exist; current Thistle radio path
  still needs standalone packaging and hardware verification.
- MIA-M10Q GNSS: substantial local Rust; `ublox` is reusable.
- PCM5102A audio: substantial local Rust over generic I2S.
- BHI260AP: local stub and no confirmed permissive reusable Rust driver.
- TP4065B: local Rust using ADC/GPIO.
- MicroSD: local Rust/ESP-IDF path and portable `embedded-sdmmc` alternative.

### T-Deck, Plus, and Max variants

The base colour display/input/radio/GNSS pieces overlap with the inventory
above. Variant modelling must be component-based:

- a cellular-equipped variant needs the A7682E modem package;
- an audio-equipped variant needs its actual codec/amplifier package;
- a Max configuration can require both;
- board detection or explicit signed provisioning must distinguish the fitted
  components without destructive probing.

The exact commercial naming and fitted hardware must be verified against the
specific revision before a manifest is signed.

### T-Watch Ultra

The production tracker is [issue #120](https://github.com/wan0net/thistle-os/issues/120),
with implementation issues [#121](https://github.com/wan0net/thistle-os/issues/121)
through [#127](https://github.com/wan0net/thistle-os/issues/127). Its baseline
component set and Rust direction are:

| Component | Rust direction |
| --- | --- |
| CO5300 | Adopt `decaday/display-driver` CO5300 panel; build Thistle ESP32-S3 QSPI transport |
| CST9217 | Evaluate `cst92xx`; verify address `0x1A` and packet behaviour |
| XL9555 | Adopt/evaluate `xl9555-hal` behind the shared I2C service |
| AXP2101 | Adopt/evaluate `axp2101-embedded`; coordinate rail ownership |
| PCF85063A | Adopt/evaluate `pcf85063a`; do not reuse the PCF8563 identity |
| DRV2605 | Adopt/evaluate `drv2605-async`; add bounded haptic HAL and permissions |
| MAX98357A | Generic I2S output plus correct rail/gain sequencing |
| T3902 | Implement/obtain PDM input transport and bounded capture ABI |
| BHI260AP | New permissive Rust implementation required unless a suitable project appears |
| MIA-M10Q | Reuse local logic and assess `ublox` |
| ST25R3916 | Pin/audit `embassy-rs/rnfc`; add bounded NFC/NDEF policy |
| MicroSD | Storage-neutral system-volume driver over shared SPI |
| Radio variants | Select SX1262, SX1280, CC1101, LR1121, or other fitted component explicitly; use matching permissive crate where viable |

The checked-in T-Watch profile remains bring-up data. At this review it still
contains known inaccuracies, including CST9217 address `0x5A`, PCF8563 instead
of PCF85063A, and PCM5102A instead of MAX98357A/T3902. The open issues, not that
profile, record the intended correction.

## Reference sources that are not Rust dependencies

### LilyGoLib

[`Xinyuan-LilyGO/LilyGoLib`](https://github.com/Xinyuan-LilyGO/LilyGoLib) is an
MIT-licensed C project. It is valuable for:

- board schematics and documented pin assignments;
- power-rail and reset sequences;
- I2C addresses and component identities;
- controller initialization commands;
- understanding LilyGo board and radio variants.

It is not a Rust driver library and is not the implementation foundation chosen
for the Rust-only driver work. Facts derived from it should be checked against
schematics, datasheets, and hardware before becoming signed board policy.

### C/C++ libraries already used or considered

- RadioLib is C++ and therefore not a Rust-only driver candidate.
- `esp_modem` is C/C++ infrastructure and does not satisfy the pure-Rust modem
  goal, although the current fallback may continue until a replacement exists.
- Vendor Arduino display and board code is bring-up evidence, not code to place
  in the Rust driver layer.

## Adoption checklist

Before adding any external Rust driver:

1. Confirm the exact chip and board revision from authoritative hardware
   evidence.
2. Verify license and every transitive dependency against the BSD-3-Clause and
   no-GPL policy.
3. Pin the exact crate version or Git commit and record source provenance.
4. Confirm `no_std`, target architecture, Rust version, and ESP toolchain
   compatibility.
5. Audit register operations, bounds, timeouts, reset state, and error paths.
6. Adapt through kernel-owned bus handles; never let the crate independently
   initialize or reconfigure shared peripherals.
7. Wrap it in the versioned Thistle HAL/driver ABI without exposing Rust traits
   across the ELF boundary.
8. Add host tests with a recording fake bus and malformed-input cases.
9. Build and sign the standalone `.drv.elf` and manifest.
10. Verify initialization, normal operation, device loss, sleep/wake, unload,
    reload, shared-bus contention, and power failure on hardware.
11. Record support honestly as declared, built, simulator-tested, or
    hardware-verified.

## Remaining focused research

- Find or implement a permissive BHI260AP host protocol and firmware loader.
- Recheck ESP32 Rust PDM input support and design the T3902 capture path.
- Decide the A7682E strategy: `atat`-based Rust state machine, a new A76xx crate,
  or a temporary C/C++ compatibility package outside the Rust-only goal.
- Search specifically for a permissive SI4432 driver if that hardware variant
  remains supported.
- Verify exact GDEQ031T10/UC8253 coverage in Rust e-paper crates before replacing
  the substantial local implementation.
- Choose one candidate per overlapping device and remove duplicate protocol
  implementations after parity.
