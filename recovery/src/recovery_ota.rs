// SPDX-License-Identifier: BSD-3-Clause
// Recovery OTA — check/flash firmware from SD card or HTTP

use ed25519_dalek::{Signature, VerifyingKey};
use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use esp_idf_sys::*;
use log::*;
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::artifact_manifest::{ArtifactManifest, ArtifactType};
use crate::bounded_io;
use crate::bundle_transaction::{self, Boundary, BundleFile, RecoveryAction};
use crate::catalog_path::validate_catalog_id;

// ---------------------------------------------------------------------------
// Chip detection
// ---------------------------------------------------------------------------

/// Detect the ESP32 chip variant from the ROM at runtime.
/// Returns a slug like "esp32", "esp32s2", "esp32s3", "esp32c3", "esp32c6", "esp32h2".
///
/// Uses `esp_chip_info()` which is available on all ESP32 variants via ROM.
/// The `model` field maps to the `esp_chip_model_t` enum in esp_system.h.
pub fn detect_chip() -> &'static str {
    unsafe {
        #[repr(C)]
        struct EspChipInfo {
            model: u32,
            features: u32,
            revision: u16,
            cores: u8,
        }

        extern "C" {
            fn esp_chip_info(info: *mut EspChipInfo);
        }

        let mut info = EspChipInfo {
            model: 0,
            features: 0,
            revision: 0,
            cores: 0,
        };
        esp_chip_info(&mut info);

        match info.model {
            1 => "esp32",    // CHIP_ESP32
            2 => "esp32s2",  // CHIP_ESP32S2
            9 => "esp32s3",  // CHIP_ESP32S3
            5 => "esp32c3",  // CHIP_ESP32C3
            13 => "esp32c6", // CHIP_ESP32C6
            16 => "esp32h2", // CHIP_ESP32H2
            _ => "unknown",
        }
    }
}

/// Returns a human-readable architecture family for display purposes.
/// Xtensa cores: esp32, esp32s2, esp32s3.
/// RISC-V cores: esp32c3, esp32c6, esp32h2.
pub fn chip_arch_family(chip: &str) -> &'static str {
    match chip {
        "esp32" | "esp32s2" | "esp32s3" => "xtensa",
        "esp32c3" | "esp32c6" | "esp32h2" => "riscv32",
        _ => "unknown",
    }
}

/// Check whether a catalog entry's `arch` field matches the detected chip.
///
/// Rules:
/// - Entry has no `arch` field → universal, always matches.
/// - Entry `arch` is empty string → universal, always matches.
/// - Entry `arch` matches `chip` exactly → matches.
/// - Otherwise → does not match.
pub fn catalog_entry_arch_matches(obj: &str, chip: &str) -> bool {
    match json_extract_string(obj, "arch") {
        Some(arch) if !arch.is_empty() => arch == chip,
        _ => true, // no arch field or empty = universal
    }
}

const SD_FIRMWARE_PATH: &str = "/sdcard/update/thistle_os.bin";
const BUNDLE_DOWNLOAD_DIR: &str = "/sdcard/.thistle-bundle-download";
const MAX_FIRMWARE_SIZE: usize = 4 * 1024 * 1024; // 4MB
const MAX_CATALOG_SIZE: usize = 256 * 1024;
const MAX_BOARD_PROFILE_SIZE: usize = 64 * 1024;
const MAX_EXECUTABLE_SIZE: usize = 2 * 1024 * 1024;
const MAX_SIGNATURE_SIZE: usize = 64;
const MAX_ARTIFACT_MANIFEST_SIZE: usize = 4096;
const REQUIRE_EXECUTABLE_SIGNATURES: bool = true;

#[cfg(not(debug_assertions))]
const RECOVERY_SIGNING_KEY: [u8; 32] = [
    0xeb, 0x7b, 0xc6, 0x5c, 0x1e, 0x3f, 0xfc, 0x49, 0x96, 0x1c, 0xa8, 0x15, 0xdb, 0x34, 0x37, 0x58,
    0x34, 0x6d, 0xbe, 0x80, 0x50, 0x38, 0xbc, 0xd4, 0x49, 0x5a, 0x7a, 0x01, 0x66, 0x5e, 0x60, 0x89,
];

#[cfg(debug_assertions)]
const RECOVERY_SIGNING_KEY: [u8; 32] = [
    0xa1, 0x3e, 0x7b, 0x54, 0x02, 0xd8, 0xf1, 0x6c, 0x89, 0x45, 0xbb, 0x0a, 0xe7, 0x33, 0x9d, 0x5f,
    0x12, 0xc4, 0x68, 0xae, 0x7d, 0x01, 0xf5, 0x92, 0xb6, 0x3a, 0xde, 0x84, 0x50, 0xc7, 0x1b, 0xe9,
];

/// Global progress counter (0-100) for the active bundle download.
/// Updated by `recovery_download_board_bundle_for`; read by the web handler.
pub static BUNDLE_PROGRESS: AtomicU8 = AtomicU8::new(0);

#[derive(Debug)]
pub enum Ota1State {
    Valid,
    PendingVerify,
    Invalid,
    NotFound,
}

/// Check the state of the ota_1 partition
pub fn check_ota1() -> Ota1State {
    unsafe {
        let part = esp_ota_get_next_update_partition(std::ptr::null());
        if part.is_null() {
            return Ota1State::NotFound;
        }

        let mut state: esp_ota_img_states_t = 0;
        let ret = esp_ota_get_state_partition(part, &mut state);

        if ret != ESP_OK as i32 {
            return Ota1State::Invalid;
        }

        match state {
            x if x == esp_ota_img_states_t_ESP_OTA_IMG_VALID => Ota1State::Valid,
            x if x == esp_ota_img_states_t_ESP_OTA_IMG_PENDING_VERIFY => Ota1State::PendingVerify,
            _ => Ota1State::Invalid,
        }
    }
}

/// Set ota_1 as boot partition and restart
pub fn boot_ota1() -> anyhow::Result<()> {
    unsafe {
        let part = esp_ota_get_next_update_partition(std::ptr::null());
        if part.is_null() {
            anyhow::bail!("No ota_1 partition");
        }
        let ret = esp_ota_set_boot_partition(part);
        if ret != ESP_OK as i32 {
            anyhow::bail!("Failed to set boot partition: {}", ret);
        }
        esp_restart();
    }
}

/// Check if firmware file exists on SD card
pub fn check_sd_firmware() -> bool {
    std::path::Path::new(SD_FIRMWARE_PATH).exists()
}

/// Flash firmware from SD card to ota_1
pub fn apply_sd_firmware() -> anyhow::Result<()> {
    info!("Applying firmware from SD: {}", SD_FIRMWARE_PATH);

    let mut firmware = std::fs::File::open(SD_FIRMWARE_PATH)?;
    let firmware_size = usize::try_from(firmware.metadata()?.len())
        .map_err(|_| anyhow::anyhow!("Firmware size cannot be represented"))?;
    if firmware_size == 0 || firmware_size > MAX_FIRMWARE_SIZE {
        anyhow::bail!("Invalid firmware size: {} bytes", firmware_size);
    }
    let sig = read_signature_file(Path::new(&format!("{}.sig", SD_FIRMWARE_PATH)))
        .map_err(|_| anyhow::anyhow!("Missing or invalid SD firmware signature"))?;
    verify_ed25519_reader(&mut firmware, &sig)?;
    firmware.rewind()?;

    write_ota1_reader(&mut firmware, firmware_size)?;
    select_ota1_for_boot()?;
    Ok(())
}

/// Download firmware from app store catalog and flash to ota_1
pub fn download_and_flash(catalog_url: &str) -> anyhow::Result<()> {
    info!("Fetching catalog: {}", catalog_url);

    // Fetch catalog JSON
    let catalog_json = http_get_string(catalog_url, MAX_CATALOG_SIZE)?;

    // Find the firmware entry (type = "firmware")
    let fw_entry = find_catalog_entry_by_type(&catalog_json, "firmware")
        .ok_or_else(|| anyhow::anyhow!("No firmware entry in catalog"))?;
    let fw_url = json_extract_string(fw_entry, "url")
        .ok_or_else(|| anyhow::anyhow!("Firmware entry missing url"))?;
    let expected_sha = json_extract_string(fw_entry, "sha256")
        .ok_or_else(|| anyhow::anyhow!("Firmware entry missing sha256"))?;
    let sig_url = json_extract_string(fw_entry, "sig_url");

    info!("Downloading firmware: {}", fw_url);
    println!("Downloading: {}", fw_url);

    let download_stage = DownloadStage::new(Path::new(BUNDLE_DOWNLOAD_DIR))?;
    let firmware_path = download_stage.path("firmware.bin")?;
    download_catalog_entry_to_file(
        &fw_url,
        Some(&expected_sha),
        sig_url.as_deref(),
        REQUIRE_EXECUTABLE_SIGNATURES,
        MAX_FIRMWARE_SIZE,
        &firmware_path,
    )?;
    let firmware_size = usize::try_from(std::fs::metadata(&firmware_path)?.len())?;
    info!("Downloaded {} bytes", firmware_size);
    println!("Downloaded {} bytes. Flashing...", firmware_size);

    write_ota1_file(&firmware_path)?;
    select_ota1_for_boot()?;

    info!("Firmware flashed successfully");
    Ok(())
}

fn write_ota1_file(path: &Path) -> anyhow::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let size = usize::try_from(file.metadata()?.len())
        .map_err(|_| anyhow::anyhow!("Firmware size cannot be represented"))?;
    write_ota1_reader(&mut file, size)
}

fn write_ota1_reader(reader: &mut impl Read, total: usize) -> anyhow::Result<()> {
    if total == 0 || total > MAX_FIRMWARE_SIZE {
        anyhow::bail!("Invalid firmware size: {} bytes", total);
    }
    unsafe {
        let part = esp_ota_get_next_update_partition(std::ptr::null());
        if part.is_null() {
            anyhow::bail!("No OTA update partition");
        }

        let mut handle: esp_ota_handle_t = 0;
        let ret = esp_ota_begin(part, total, &mut handle);
        if ret != ESP_OK as i32 {
            anyhow::bail!("esp_ota_begin failed: {}", ret);
        }

        let mut chunk = [0u8; 4096];
        let mut written = 0;
        while written < total {
            let wanted = chunk.len().min(total - written);
            let count = match reader.read(&mut chunk[..wanted]) {
                Ok(count) => count,
                Err(error) => {
                    esp_ota_abort(handle);
                    anyhow::bail!("Firmware read failed at offset {}: {}", written, error);
                }
            };
            if count == 0 {
                esp_ota_abort(handle);
                anyhow::bail!("Firmware ended at {} of {} bytes", written, total);
            }
            let ret = esp_ota_write(handle, chunk[..count].as_ptr() as *const _, count);
            if ret != ESP_OK as i32 {
                esp_ota_abort(handle);
                anyhow::bail!("esp_ota_write failed at offset {}: {}", written, ret);
            }
            written += count;

            // Progress every 10%
            let pct = written * 100 / total;
            if pct % 10 == 0 {
                print!("\r  Progress: {}%", pct);
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
        }
        println!("\r  Progress: 100%");

        let ret = esp_ota_end(handle);
        if ret != ESP_OK as i32 {
            anyhow::bail!("esp_ota_end failed: {}", ret);
        }

        info!("OTA flash complete; boot partition remains unchanged");
        Ok(())
    }
}

/// Select the already-written OTA partition. Bundle installs call this only
/// after the complete filesystem generation is active and recoverable.
fn select_ota1_for_boot() -> anyhow::Result<()> {
    unsafe {
        let part = esp_ota_get_next_update_partition(std::ptr::null());
        if part.is_null() {
            anyhow::bail!("No OTA update partition");
        }
        let ret = esp_ota_set_boot_partition(part);
        if ret != ESP_OK as i32 {
            anyhow::bail!("esp_ota_set_boot_partition failed: {}", ret);
        }
        info!("OTA boot partition selected");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Component-level hardware detection
// ---------------------------------------------------------------------------

/// A single hardware component detected via bus probing.
#[derive(Debug, Clone)]
pub struct DetectedComponent {
    /// Bus type: "i2c", "spi", or "uart".
    pub bus: String,
    /// I2C: 7-bit device address. SPI: CS GPIO pin number. UART: port number.
    pub address: u16,
    /// Chip ID read from a register probe, if available.
    pub chip_id: Option<u16>,
}

// ---------------------------------------------------------------------------
// T-Deck Pro pin definitions (from board_tdeck_pro.h)
// ---------------------------------------------------------------------------

/// I2C bus
const PROBE_I2C_SDA: i32 = 13;
const PROBE_I2C_SCL: i32 = 14;

/// 1.8V power rail enable — must be asserted before scanning I2C/SPI devices
const PROBE_1V8_EN: u32 = 38;

/// E-paper display (GDEQ031T10)
const PROBE_EPAPER_CS: u32 = 34;
const PROBE_EPAPER_BUSY: u32 = 37; // active LOW when panel is busy
                                   // RST is not connected on T-Deck Pro (BOARD_EPAPER_RST = -1)

/// SX1262 LoRa radio
const PROBE_LORA_CS: u32 = 3;
const PROBE_LORA_BUSY: u32 = 6; // BUSY pin, active HIGH when radio is busy
const PROBE_LORA_EN: u32 = 46; // power enable for LoRa

/// SD card
const PROBE_SD_CS: u32 = 48;

/// GPS (MIA-M10Q) — UART_NUM_2, TX=43, RX=44
const PROBE_GPS_UART: i32 = 2; // UART_NUM_2
const PROBE_GPS_TX: i32 = 43;
const PROBE_GPS_RX: i32 = 44;
const PROBE_GPS_EN: u32 = 39; // power enable for GPS

/// Modem (A7682E) — not present in T-Deck Pro board header; no UART defined.
/// Modem detection is omitted as there is no canonical UART assignment.

/// Human-readable names for well-known I2C addresses.
///
/// Used by the web UI to show `"Found: TCA8418 Keyboard (I2C 0x34)"` before
/// the catalog has even been fetched.
const KNOWN_I2C_DEVICES: &[(u8, &str)] = &[
    (0x15, "CST816S Touch"),
    (0x1A, "CST328 Touch"),
    (0x28, "BHI260AP IMU"),
    (0x34, "TCA8418 Keyboard"),
    (0x23, "LTR-553 Light Sensor"),
    (0x18, "LIS3DH Accelerometer"),
    (0x51, "PCF8563 RTC"),
    (0x55, "PCF8563 RTC"), // alternate address (T-Deck Pro)
    (0x6A, "QMI8658C Accel/Gyro"),
    (0x6B, "QMI8658C Accel/Gyro"),
    (0x76, "BMP280 Pressure"),
    (0x3C, "SSD1306 OLED"),
];

/// Human-readable names for SPI devices, keyed by CS GPIO pin number.
const KNOWN_SPI_DEVICES: &[(u8, &str)] = &[
    (34, "GDEQ031T10 E-Paper"), // CS = GPIO 34
    (3, "SX1262 LoRa Radio"),   // CS = GPIO 3
    (48, "SD Card"),            // CS = GPIO 48
];

/// Human-readable names for UART devices, keyed by UART port number.
const KNOWN_UART_DEVICES: &[(u8, &str)] = &[
    (2, "MIA-M10Q GPS"), // UART_NUM_2
];

/// Return the display name for a known I2C address, or None.
pub fn i2c_device_name(addr: u8) -> Option<&'static str> {
    KNOWN_I2C_DEVICES
        .iter()
        .find(|(a, _)| *a == addr)
        .map(|(_, n)| *n)
}

/// Return the display name for a known SPI device by CS pin, or None.
pub fn spi_device_name(cs_pin: u8) -> Option<&'static str> {
    KNOWN_SPI_DEVICES
        .iter()
        .find(|(p, _)| *p == cs_pin)
        .map(|(_, n)| *n)
}

/// Return the display name for a known UART device by port number, or None.
pub fn uart_device_name(port: u8) -> Option<&'static str> {
    KNOWN_UART_DEVICES
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, n)| *n)
}

/// Return the display name for any detected component regardless of bus type.
pub fn component_device_name(c: &DetectedComponent) -> &'static str {
    match c.bus.as_str() {
        "i2c" => i2c_device_name(c.address as u8).unwrap_or("Unknown Device"),
        "spi" => spi_device_name(c.address as u8).unwrap_or("Unknown SPI Device"),
        "uart" => uart_device_name(c.address as u8).unwrap_or("Unknown UART Device"),
        _ => "Unknown Device",
    }
}

/// Minimal recovery no longer probes generic board pins.
///
/// Board catalogs/configs are authoritative; probing remains intentionally
/// disabled here so recovery never drives board-specific display, radio, power,
/// or input pins before the user has selected the board profile.
pub fn scan_hardware() -> Vec<DetectedComponent> {
    let chip = detect_chip();
    info!("Chip: {} ({})", chip.to_uppercase(), chip_arch_family(chip));
    info!("Hardware probing disabled; selected board config drives install matching");
    Vec::new()
}

// ---------------------------------------------------------------------------
// I2C scanner
// ---------------------------------------------------------------------------

/// Scan I2C bus 0 (SDA=13, SCL=14) for all responding devices.
///
/// Probes addresses 0x08–0x77 (standard 7-bit range, reserved addresses
/// 0x00–0x07 and 0x78–0x7F excluded).
fn scan_i2c(found: &mut Vec<DetectedComponent>) {
    unsafe {
        #[repr(C)]
        struct I2cMasterBusConfig {
            i2c_port: i32,
            sda_io_num: i32,
            scl_io_num: i32,
            clk_source: u32,
            glitch_ignore_cnt: u8,
            intr_priority: i32,
            trans_queue_depth: usize,
            flags: u32,
        }

        let cfg = I2cMasterBusConfig {
            i2c_port: 0,               // I2C_NUM_0
            sda_io_num: PROBE_I2C_SDA, // GPIO 13
            scl_io_num: PROBE_I2C_SCL, // GPIO 14
            clk_source: 11,            // SOC_MOD_CLK_XTAL = I2C_CLK_SRC_DEFAULT on ESP32-S3
            glitch_ignore_cnt: 7,
            intr_priority: 0,
            trans_queue_depth: 0,
            flags: 0x01, // enable_internal_pullup
        };

        let mut bus_handle: *mut core::ffi::c_void = core::ptr::null_mut();
        let ret = i2c_new_master_bus(
            &cfg as *const _ as *const core::ffi::c_void,
            &mut bus_handle,
        );
        if ret != 0 || bus_handle.is_null() {
            info!("I2C bus init failed (ret={}), skipping I2C scan", ret);
            return;
        }

        info!("Scanning I2C bus (0x08..0x77)...");

        for addr in 0x08u8..=0x77u8 {
            if i2c_probe(bus_handle, addr) {
                let name = i2c_device_name(addr).unwrap_or("unknown");
                info!("  Found I2C 0x{:02X} ({})", addr, name);
                found.push(DetectedComponent {
                    bus: "i2c".to_string(),
                    address: addr as u16,
                    chip_id: None,
                });
            }
        }

        i2c_del_master_bus(bus_handle);
        info!("I2C scan complete");
    }
}

// ---------------------------------------------------------------------------
// SPI scanner — GPIO-level probing (non-destructive, no bus writes)
// ---------------------------------------------------------------------------

/// Probe SPI-connected devices using GPIO-level detection on their CS and
/// BUSY/status pins.  No SPI transactions are issued; probing is non-destructive.
///
/// Devices probed:
///   - GDEQ031T10 e-paper: CS=GPIO 34, BUSY=GPIO 37 (active LOW when busy).
///     Enable LoRa power rail (GPIO 46) is not needed for e-paper; BUSY is
///     driven by the panel itself.  We configure BUSY as input with pull-up and
///     read the level.  A valid panel will drive BUSY HIGH when idle.
///   - SX1262 LoRa: CS=GPIO 3, BUSY=GPIO 6 (active HIGH when busy).
///     Power enable via GPIO 46.  Configure BUSY as input with pull-down and
///     read the level.  A powered radio will drive BUSY LOW when idle.
///   - SD card: CS=GPIO 48.  Configure CS as output, briefly assert LOW, then
///     release.  If an SD card is present the SPI MISO line would respond, but
///     since we don't run the full SPI bus we instead check if the CS GPIO
///     itself can be driven without short-circuit (always succeeds; this is a
///     best-effort heuristic — the SD card is often detected reliably via the
///     filesystem mount in main.rs instead).
fn scan_spi(found: &mut Vec<DetectedComponent>) {
    info!("Probing SPI devices (GPIO-level)...");

    // --- E-paper (GDEQ031T10): CS=34, BUSY=37 ---
    // BUSY is driven by the panel: HIGH = idle, LOW = busy/initialising.
    // Configure as input with pull-up and read.  A floating pin would also
    // read HIGH, so we accept HIGH as "panel present" given the CS line is
    // also wired (CS=34).  This is a best-effort probe; false positives are
    // possible on unpopulated boards but acceptable for recovery scanning.
    let epaper_detected = unsafe {
        gpio_set_direction(PROBE_EPAPER_BUSY, 1); // GPIO_MODE_INPUT = 1
        gpio_set_pull_mode(PROBE_EPAPER_BUSY, 1); // GPIO_PULLUP_ONLY = 1
        vTaskDelay(1); // let pull-up settle
                       // Configure CS as output — if it configures cleanly the GPIO is wired
        gpio_set_direction(PROBE_EPAPER_CS, 2); // GPIO_MODE_OUTPUT = 2
        gpio_set_level(PROBE_EPAPER_CS, 1); // deassert (CS active LOW)
        let busy_level = gpio_get_level(PROBE_EPAPER_BUSY);
        // BUSY HIGH = panel idle (expected after power-on).
        // Restore BUSY to input floating to avoid interfering with the kernel driver.
        gpio_set_direction(PROBE_EPAPER_BUSY, 1);
        busy_level == 1
    };
    if epaper_detected {
        let name = spi_device_name(PROBE_EPAPER_CS as u8).unwrap_or("SPI Device");
        info!("  Found SPI CS={} ({})", PROBE_EPAPER_CS, name);
        found.push(DetectedComponent {
            bus: "spi".to_string(),
            address: PROBE_EPAPER_CS as u16,
            chip_id: None,
        });
    }

    // --- SX1262 LoRa: CS=3, BUSY=6, power enable=46 ---
    // Enable LoRa power rail, then read BUSY.
    // SX1262 drives BUSY LOW when idle (ready), HIGH when executing a command.
    // After power-on the radio briefly asserts BUSY HIGH then releases it.
    let lora_detected = unsafe {
        gpio_set_direction(PROBE_LORA_EN, 2); // GPIO_MODE_OUTPUT = 2
        gpio_set_level(PROBE_LORA_EN, 1); // enable LoRa power
        vTaskDelay(5); // allow radio to power up (~5 ms)
        gpio_set_direction(PROBE_LORA_BUSY, 1); // GPIO_MODE_INPUT = 1
        gpio_set_pull_mode(PROBE_LORA_BUSY, 2); // GPIO_PULLDOWN_ONLY = 2
        vTaskDelay(1);
        let busy_level = gpio_get_level(PROBE_LORA_BUSY);
        // BUSY LOW = radio idle = radio is present and powered.
        // Do not power down the LoRa rail here — the kernel may need it.
        busy_level == 0
    };
    if lora_detected {
        let name = spi_device_name(PROBE_LORA_CS as u8).unwrap_or("SPI Device");
        info!("  Found SPI CS={} ({})", PROBE_LORA_CS, name);
        found.push(DetectedComponent {
            bus: "spi".to_string(),
            address: PROBE_LORA_CS as u16,
            chip_id: None,
        });
    }

    // --- SD card: CS=48 ---
    // Probe by briefly driving CS LOW.  If the GPIO configures without error
    // we record the SD card as detected; the firmware mount at boot is the
    // authoritative check.  This is a best-effort / wiring verification probe.
    let sd_detected = unsafe {
        let ret = gpio_set_direction(PROBE_SD_CS, 2); // GPIO_MODE_OUTPUT = 2
        if ret == 0 {
            gpio_set_level(PROBE_SD_CS, 1); // deassert CS (active LOW)
        }
        ret == 0 // GPIO configured → SD card slot is wired on this board
    };
    if sd_detected {
        let name = spi_device_name(PROBE_SD_CS as u8).unwrap_or("SPI Device");
        info!("  Found SPI CS={} ({})", PROBE_SD_CS, name);
        found.push(DetectedComponent {
            bus: "spi".to_string(),
            address: PROBE_SD_CS as u16,
            chip_id: None,
        });
    }

    info!("SPI probe complete");
}

// ---------------------------------------------------------------------------
// UART scanner — listen-only, short timeouts
// ---------------------------------------------------------------------------

/// UART configuration struct matching the ESP-IDF `uart_config_t` layout.
/// Only the fields we need are set; the rest are zeroed (safe defaults).
#[repr(C)]
struct UartConfig {
    baud_rate: i32,
    data_bits: u32, // UART_DATA_8_BITS = 3
    parity: u32,    // UART_PARITY_DISABLE = 0
    stop_bits: u32, // UART_STOP_BITS_1 = 1
    flow_ctrl: u32, // UART_HW_FLOWCTRL_DISABLE = 0
    rx_flow_ctrl_thresh: u8,
    source_clk: u32, // UART_SCLK_DEFAULT = 0
}

/// Probe UART devices by listening briefly for known data patterns.
///
/// Devices probed:
///   - GPS (MIA-M10Q): UART_NUM_2, TX=43, RX=44, 9600 baud.
///     Enable GPS power rail (GPIO 39), install UART driver, listen up to
///     500 ms for NMEA sentences (lines starting with '$').
///
/// Each UART driver is deleted after probing to release the resource.
/// Probing is non-destructive: no commands are sent to the GPS.
fn scan_uart(found: &mut Vec<DetectedComponent>) {
    info!("Probing UART devices...");

    // --- GPS (MIA-M10Q): UART_NUM_2, 9600 baud ---
    let gps_detected = unsafe {
        // Enable GPS power rail
        gpio_set_direction(PROBE_GPS_EN, 2); // GPIO_MODE_OUTPUT = 2
        gpio_set_level(PROBE_GPS_EN, 1); // HIGH = enable
        vTaskDelay(10); // allow GPS module to power up

        let cfg = UartConfig {
            baud_rate: 9600,
            data_bits: 3, // UART_DATA_8_BITS
            parity: 0,    // UART_PARITY_DISABLE
            stop_bits: 1, // UART_STOP_BITS_1
            flow_ctrl: 0, // UART_HW_FLOWCTRL_DISABLE
            rx_flow_ctrl_thresh: 0,
            source_clk: 0, // UART_SCLK_DEFAULT
        };

        let ret_cfg = uart_param_config(
            PROBE_GPS_UART,
            &cfg as *const UartConfig as *const core::ffi::c_void,
        );
        let ret_pin = uart_set_pin(
            PROBE_GPS_UART,
            PROBE_GPS_TX, // TX
            PROBE_GPS_RX, // RX
            -1,           // RTS not used
            -1,           // CTS not used
        );
        // RX buffer 512 bytes; no TX buffer, no queue, no ISR flags
        let ret_drv = uart_driver_install(PROBE_GPS_UART, 512, 0, 0, core::ptr::null_mut(), 0);

        let mut detected = false;
        if ret_cfg == 0 && ret_pin == 0 && ret_drv == 0 {
            // Listen for up to 500 ms (~50 ticks at 100 Hz) for any NMEA data.
            // NMEA sentences start with '$'; 9600 baud = ~960 bytes/sec so
            // in 500 ms we may see ~480 bytes.  Read in small chunks.
            let mut buf = [0u8; 64];
            let deadline_ticks: u32 = 50; // ~500 ms at 100 Hz
            let mut ticks_waited: u32 = 0;

            'outer: while ticks_waited < deadline_ticks {
                // timeout = 2 ticks per read attempt (~20 ms)
                let n = uart_read_bytes(PROBE_GPS_UART, buf.as_mut_ptr(), buf.len() as u32, 2);
                if n > 0 {
                    // Check for '$' (NMEA sentence start) in received bytes
                    for &b in &buf[..n as usize] {
                        if b == b'$' {
                            detected = true;
                            break 'outer;
                        }
                    }
                }
                ticks_waited += 2;
                vTaskDelay(1);
            }
        } else {
            info!(
                "GPS UART init failed (cfg={} pin={} drv={})",
                ret_cfg, ret_pin, ret_drv
            );
        }

        // Always clean up UART driver, even if init partially failed
        uart_driver_delete(PROBE_GPS_UART);
        detected
    };

    if gps_detected {
        let name = uart_device_name(PROBE_GPS_UART as u8).unwrap_or("UART Device");
        info!("  Found UART{} ({})", PROBE_GPS_UART, name);
        found.push(DetectedComponent {
            bus: "uart".to_string(),
            address: PROBE_GPS_UART as u16,
            chip_id: None,
        });
    }

    info!("UART probe complete");
}

/// Legacy board lookup kept for backward compatibility.
///
/// Returns a selected/downloaded board profile when one already exists. Minimal
/// recovery does not infer board identity by probing generic pins.
pub fn autodetect_board() -> Option<String> {
    // First check board.json — if it exists, trust it
    if let Ok(content) = std::fs::read_to_string(BOARD_JSON_PATH) {
        if let Some(id) = json_extract_board_id(&content) {
            info!("Board detected from board.json: {}", id);
            return Some(id);
        }
        if let Some(name) = json_extract_str_nested(&content, "board", "name") {
            let slug = board_name_to_slug(&name);
            info!("Board detected from board.json name '{}': {}", name, slug);
            return Some(slug);
        }
    }

    info!("No board profile found; select one in the recovery web UI");
    None
}

/// Probe a single I2C address — returns true if a device ACKs.
/// Uses ESP-IDF v5.5's `i2c_master_probe()` for reliable detection.
unsafe fn i2c_probe(bus: *mut core::ffi::c_void, addr: u8) -> bool {
    i2c_master_probe(bus, addr as u16, 50) == 0
}

extern "C" {
    // I2C (ESP-IDF v5 master API)
    fn i2c_new_master_bus(
        cfg: *const core::ffi::c_void,
        handle: *mut *mut core::ffi::c_void,
    ) -> i32;
    fn i2c_del_master_bus(handle: *mut core::ffi::c_void) -> i32;
    fn i2c_master_probe(bus: *mut core::ffi::c_void, address: u16, timeout_ms: i32) -> i32;

    // GPIO
    fn gpio_set_direction(pin: u32, mode: u32) -> i32;
    fn gpio_set_level(pin: u32, level: u32) -> i32;
    fn gpio_get_level(pin: u32) -> i32;
    fn gpio_set_pull_mode(pin: u32, mode: u32) -> i32;

    // UART
    fn uart_param_config(port: i32, cfg: *const core::ffi::c_void) -> i32;
    fn uart_set_pin(port: i32, tx: i32, rx: i32, rts: i32, cts: i32) -> i32;
    fn uart_driver_install(
        port: i32,
        rx_buf: i32,
        tx_buf: i32,
        queue_size: i32,
        queue: *mut *mut core::ffi::c_void,
        flags: i32,
    ) -> i32;
    fn uart_driver_delete(port: i32) -> i32;
    fn uart_read_bytes(port: i32, buf: *mut u8, len: u32, timeout_ticks: u32) -> i32;

    // FreeRTOS
    fn vTaskDelay(ticks: u32);

}

// ---------------------------------------------------------------------------
// Board-aware bundle download
// ---------------------------------------------------------------------------

const BOARD_JSON_PATH: &str = "/spiffs/config/board.json";
const FALLBACK_BOARD: &str = "tdeck-pro";
const SD_DRIVERS_DIR: &str = "/sdcard/drivers";
const SD_WM_DIR: &str = "/sdcard/wm";
const SD_BOARDS_DIR: &str = "/sdcard/config/boards";

/// Read the board name from /spiffs/config/board.json, falling back to a
/// hardcoded default when the file is absent or unparseable.
fn read_board_name() -> String {
    if let Ok(content) = std::fs::read_to_string(BOARD_JSON_PATH) {
        // board.json has the shape: { "board": { "name": "LilyGo T-Deck Pro", ... } }
        // The board slug used for matching is a normalised version (lower-case,
        // spaces → hyphens), e.g. "LilyGo T-Deck Pro" → "tdeck-pro".
        // If the JSON contains a top-level "board_id" key we use that directly;
        // otherwise we derive the slug from the "name" field.
        if let Some(id) = json_extract_board_id(&content) {
            return id;
        }
        if let Some(name) = json_extract_str_nested(&content, "board", "name") {
            return board_name_to_slug(&name);
        }
    }
    info!(
        "board.json not found or missing name — using fallback '{}'",
        FALLBACK_BOARD
    );
    FALLBACK_BOARD.to_string()
}

/// Normalise a human-readable board name to a slug.
/// "LilyGo T-Deck Pro" → "tdeck-pro", "LilyGo T-Deck" → "tdeck".
fn board_name_to_slug(name: &str) -> String {
    let lower = name.to_lowercase();
    // Known mappings first — check more specific patterns before generic ones
    if lower.contains("t-deck pro") || lower.contains("tdeck pro") || lower.contains("t-deck-pro") {
        return "tdeck-pro".to_string();
    }
    if lower.contains("t-display-s3")
        || lower.contains("tdisplay-s3")
        || lower.contains("t display s3")
    {
        return "tdisplay-s3".to_string();
    }
    if lower.contains("t3-s3") || lower.contains("t3 s3") {
        return "t3-s3".to_string();
    }
    if lower.contains("t-deck") || lower.contains("tdeck") {
        return "tdeck".to_string();
    }
    // Generic slug: keep alphanumeric and hyphens
    lower
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Extract a direct "board_id" string from JSON.
fn json_extract_board_id(json: &str) -> Option<String> {
    json_extract_string(json, "board_id")
}

/// Extract a string nested one level: `{ "outer": { "key": "value" } }`.
fn json_extract_str_nested(json: &str, outer: &str, key: &str) -> Option<String> {
    // Find the outer object
    let outer_key = format!("\"{}\"", outer);
    let outer_pos = json.find(&outer_key)?;
    let after_outer = &json[outer_pos + outer_key.len()..];
    let brace_start = after_outer.find('{')? + 1;
    let inner_start = &after_outer[brace_start..];
    let brace_end = inner_start.find('}')?;
    let inner = &inner_start[..brace_end];
    json_extract_string(inner, key)
}

/// Extract a top-level JSON string value for `key`.
pub fn json_extract_string(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    let pos = json.find(&search)?;
    let after = &json[pos + search.len()..];
    let colon = after.find(':')? + 1;
    let rest = after[colon..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let inner = &rest[1..];
    let end = inner.find('"')?;
    Some(inner[..end].to_string())
}

/// Check whether a `compatible_boards` JSON array contains `board_name`.
/// Returns true when the array is absent or empty (universal).
fn catalog_entry_board_matches(entry_json: &str, board_name: &str) -> bool {
    // Find compatible_boards array
    let key = "\"compatible_boards\"";
    let pos = match entry_json.find(key) {
        Some(p) => p,
        None => return true, // absent = universal
    };
    let after = &entry_json[pos + key.len()..];
    let bracket = match after.find('[') {
        Some(i) => i,
        None => return true,
    };
    let inner_start = &after[bracket + 1..];
    let bracket_end = match inner_start.find(']') {
        Some(i) => i,
        None => return true,
    };
    let inner = &inner_start[..bracket_end].trim();
    if inner.is_empty() {
        return true; // empty array = universal
    }
    // Walk quoted strings
    let mut rem = *inner;
    loop {
        let q1 = match rem.find('"') {
            Some(i) => i,
            None => break,
        };
        let val = &rem[q1 + 1..];
        let q2 = match val.find('"') {
            Some(i) => i,
            None => break,
        };
        if &val[..q2] == board_name {
            return true;
        }
        rem = &val[q2 + 1..];
    }
    false
}

/// Extract a string value from the `detection` sub-object of a catalog entry.
///
/// Given `{"detection":{"bus":"i2c","address":"0x34"}}` and key `"bus"` → `Some("i2c")`.
fn catalog_extract_detection_str(obj_json: &str, key: &str) -> Option<String> {
    let det_key = "\"detection\"";
    let pos = obj_json.find(det_key)?;
    let after = &obj_json[pos + det_key.len()..];
    let brace = after.find('{')? + 1;
    let inner_start = &after[brace..];
    let brace_end = inner_start.find('}')?;
    let inner = &inner_start[..brace_end];
    json_extract_string(inner, key)
}

/// Extract a numeric value (hex-string `"0x…"` or bare decimal) from the `detection` sub-object.
/// Returns 0 when absent or unparseable.
fn catalog_extract_detection_u16(obj_json: &str, key: &str) -> u16 {
    let det_key = "\"detection\"";
    let pos = match obj_json.find(det_key) {
        Some(p) => p,
        None => return 0,
    };
    let after = &obj_json[pos + det_key.len()..];
    let brace = match after.find('{') {
        Some(i) => i + 1,
        None => return 0,
    };
    let inner_start = &after[brace..];
    let brace_end = match inner_start.find('}') {
        Some(i) => i,
        None => return 0,
    };
    let inner = &inner_start[..brace_end];
    json_hex_or_int_u16(inner, key)
}

/// Extract a u16 from JSON where the value may be `"0x…"` or a bare decimal.
fn json_hex_or_int_u16(json: &str, key: &str) -> u16 {
    let search = format!("\"{}\"", key);
    let pos = match json.find(&search) {
        Some(p) => p,
        None => return 0,
    };
    let after_key = &json[pos + search.len()..];
    let after_colon = match after_key.trim_start().strip_prefix(':') {
        Some(s) => s.trim_start(),
        None => return 0,
    };

    if after_colon.starts_with('"') {
        let inner = &after_colon[1..];
        let end = match inner.find('"') {
            Some(i) => i,
            None => return 0,
        };
        let s = inner[..end].trim();
        if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            u16::from_str_radix(hex, 16).unwrap_or(0)
        } else {
            s.parse::<u16>().unwrap_or(0)
        }
    } else {
        let num_end = after_colon
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_colon.len());
        after_colon[..num_end].parse::<u16>().unwrap_or(0)
    }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Fetch board metadata from a catalog and return web-ready JSON objects.
pub fn catalog_board_options_json(catalog_url: &str, chip: &str) -> anyhow::Result<String> {
    let catalog_json = http_get_string(catalog_url, MAX_CATALOG_SIZE)?;
    let mut boards: Vec<String> = Vec::new();

    for obj in iter_json_objects(&catalog_json) {
        if json_extract_string(obj, "type").as_deref() != Some("board") {
            continue;
        }
        if !catalog_entry_arch_matches(obj, chip) {
            continue;
        }

        let id = json_extract_string(obj, "board_id")
            .or_else(|| json_extract_string(obj, "id"))
            .unwrap_or_default();
        if id.is_empty() {
            continue;
        }

        let label = json_extract_string(obj, "name").unwrap_or_else(|| id.clone());
        let arch = json_extract_string(obj, "arch").unwrap_or_default();
        boards.push(format!(
            r#"{{"id":"{}","label":"{}","arch":"{}"}}"#,
            json_escape(&id),
            json_escape(&label),
            json_escape(&arch)
        ));
    }

    Ok(boards.join(","))
}

/// Return true if `board_id` is present in the catalog board list for this chip.
pub fn catalog_contains_board(catalog_url: &str, chip: &str, board_id: &str) -> bool {
    if board_id.is_empty() {
        return false;
    }

    let catalog_json = match http_get_string(catalog_url, MAX_CATALOG_SIZE) {
        Ok(json) => json,
        Err(_) => return false,
    };

    for obj in iter_json_objects(&catalog_json) {
        if json_extract_string(obj, "type").as_deref() != Some("board") {
            continue;
        }
        if !catalog_entry_arch_matches(obj, chip) {
            continue;
        }
        let id = json_extract_string(obj, "board_id")
            .or_else(|| json_extract_string(obj, "id"))
            .unwrap_or_default();
        if id == board_id {
            return true;
        }
    }

    false
}

/// Iterate top-level `{...}` objects in a JSON array and yield each as a `&str` slice.
fn iter_json_objects(json: &str) -> impl Iterator<Item = &str> {
    JsonObjects { src: json }
}

struct JsonObjects<'a> {
    src: &'a str,
}

impl<'a> Iterator for JsonObjects<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.src.find('{')?;
        self.src = &self.src[start..];
        // Track nesting so we capture the full object
        let mut depth = 0usize;
        let mut in_str = false;
        let mut escape = false;
        let end = self.src.char_indices().find(|(_, c)| {
            if escape {
                escape = false;
                return false;
            }
            match c {
                '\\' if in_str => {
                    escape = true;
                    false
                }
                '"' => {
                    in_str = !in_str;
                    false
                }
                '{' if !in_str => {
                    depth += 1;
                    false
                }
                '}' if !in_str => {
                    depth -= 1;
                    depth == 0
                }
                _ => false,
            }
        });
        let end_idx = end?.0 + 1;
        let obj = &self.src[..end_idx];
        self.src = &self.src[end_idx..];
        Some(obj)
    }
}

fn download_catalog_entry_to_file(
    url: &str,
    expected_sha256: Option<&str>,
    sig_url: Option<&str>,
    require_signature: bool,
    max_size: usize,
    destination: &Path,
) -> anyhow::Result<Option<Vec<u8>>> {
    http_get_to_file(url, max_size, destination)?;
    match expected_sha256.filter(|s| !s.trim().is_empty()) {
        Some(expected) => verify_sha256_file(destination, expected)?,
        None => anyhow::bail!("Catalog entry missing sha256 for {}", url),
    }

    let sig = match sig_url.filter(|s| !s.trim().is_empty()) {
        Some(sig_url) => {
            let sig = http_get_bytes(sig_url, MAX_SIGNATURE_SIZE)?;
            verify_ed25519_file(destination, &sig)?;
            Some(sig)
        }
        None if require_signature => anyhow::bail!("Catalog entry missing sig_url for {}", url),
        None => None,
    };

    Ok(sig)
}

fn verify_sha256_file(path: &Path, expected_sha256: &str) -> anyhow::Result<()> {
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("Invalid catalog sha256 '{}'", expected_sha256);
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = hex_lower(&hasher.finalize());
    if actual != expected {
        anyhow::bail!("SHA-256 mismatch: expected {}, got {}", expected, actual);
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn verify_ed25519_file(path: &Path, signature: &[u8]) -> anyhow::Result<()> {
    let mut file = std::fs::File::open(path)?;
    verify_ed25519_reader(&mut file, signature)
}

fn read_signature_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() != MAX_SIGNATURE_SIZE as u64 {
        anyhow::bail!("Invalid Ed25519 signature size: {} bytes", metadata.len());
    }
    Ok(std::fs::read(path)?)
}

fn verify_ed25519_reader(reader: &mut impl Read, signature: &[u8]) -> anyhow::Result<()> {
    if signature.len() != 64 {
        anyhow::bail!("Invalid Ed25519 signature size: {} bytes", signature.len());
    }

    let verifying_key = VerifyingKey::from_bytes(&RECOVERY_SIGNING_KEY)
        .map_err(|_| anyhow::anyhow!("Invalid recovery signing public key"))?;
    let sig_bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid Ed25519 signature"))?;
    let signature = Signature::from_bytes(&sig_bytes);

    let mut verifier = verifying_key
        .verify_stream(&signature)
        .map_err(|_| anyhow::anyhow!("Invalid Ed25519 signature"))?;
    let mut buffer = [0u8; 4096];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        verifier.update(&buffer[..count]);
    }
    verifier
        .finalize_and_verify()
        .map_err(|_| anyhow::anyhow!("Ed25519 signature verification failed"))
}

/// Download all drivers, WM entries, and the firmware image that are
/// compatible with the current board (read from board.json).
///
/// Convenience wrapper around `recovery_download_board_bundle_for`.
pub fn recovery_download_board_bundle(catalog_url: &str) -> anyhow::Result<u32> {
    let board_name = read_board_name();
    recovery_download_board_bundle_for(catalog_url, &board_name)
}

struct VerifiedManifest {
    manifest: ArtifactManifest,
    canonical: Vec<u8>,
    signature: Vec<u8>,
}

fn fetch_verified_manifests(catalog_json: &str) -> anyhow::Result<Vec<VerifiedManifest>> {
    let mut manifests = Vec::new();
    for obj in iter_json_objects(catalog_json) {
        let Some(legacy_type) = json_extract_string(obj, "type") else {
            continue;
        };
        if !matches!(legacy_type.as_str(), "firmware" | "board" | "driver" | "wm") {
            continue;
        }
        let manifest_url = json_extract_string(obj, "manifest_url").ok_or_else(|| {
            anyhow::anyhow!("Catalog {} entry is missing manifest_url", legacy_type)
        })?;
        let signature_url = json_extract_string(obj, "manifest_sig_url").ok_or_else(|| {
            anyhow::anyhow!("Catalog {} entry is missing manifest_sig_url", legacy_type)
        })?;
        let canonical = http_get_bytes(&manifest_url, MAX_ARTIFACT_MANIFEST_SIZE)?;
        let signature = http_get_bytes(&signature_url, MAX_SIGNATURE_SIZE)?;
        let mut reader = std::io::Cursor::new(&canonical);
        verify_ed25519_reader(&mut reader, &signature)?;
        let text = std::str::from_utf8(&canonical)
            .map_err(|_| anyhow::anyhow!("Signed artifact manifest is not UTF-8"))?;
        let manifest = ArtifactManifest::parse(text).map_err(anyhow::Error::msg)?;

        if manifest.artifact_type.as_str() != legacy_type {
            anyhow::bail!(
                "Catalog type '{}' disagrees with signed type '{}'",
                legacy_type,
                manifest.artifact_type.as_str()
            );
        }
        if let Some(legacy_id) = json_extract_string(obj, "id") {
            if legacy_id != manifest.id {
                anyhow::bail!(
                    "Catalog id '{}' disagrees with signed id '{}'",
                    legacy_id,
                    manifest.id
                );
            }
        }
        manifests.push(VerifiedManifest {
            manifest,
            canonical,
            signature,
        });
    }
    Ok(manifests)
}

fn artifact_size_limit(artifact_type: ArtifactType) -> usize {
    match artifact_type {
        ArtifactType::Firmware => MAX_FIRMWARE_SIZE,
        ArtifactType::Board => MAX_BOARD_PROFILE_SIZE,
        ArtifactType::Driver | ArtifactType::WindowManager => MAX_EXECUTABLE_SIZE,
    }
}

fn download_verified_payload(
    artifact: &ArtifactManifest,
    destination: &Path,
) -> anyhow::Result<()> {
    let limit = artifact_size_limit(artifact.artifact_type);
    if artifact.size > limit {
        anyhow::bail!(
            "Signed {} artifact '{}' exceeds its {} byte limit",
            artifact.artifact_type.as_str(),
            artifact.id,
            limit
        );
    }
    let written = http_get_to_file(&artifact.url, artifact.size, destination)?;
    if written != artifact.size {
        anyhow::bail!(
            "Signed artifact size mismatch: expected {}, received {}",
            artifact.size,
            written
        );
    }
    verify_sha256_file(destination, &artifact.sha256)
}

/// Download all drivers, board config, WM entries, and the firmware image that are
/// compatible with `board_name`.
///
/// Progress is reported via `BUNDLE_PROGRESS` (0-100) so the web UI can poll
/// `/api/bundle/status` while the download runs on the main thread.
///
/// Matching rules (applied per catalog entry):
///   - firmware/driver/wm entries: matched by `compatible_boards`.
///   - board entries: matched by `id`/`board_id`.
///   - all entries: filtered by detected chip `arch` when present.
///
/// All matching artifacts are downloaded and verified before any active file
/// changes. Files are then staged as one journaled generation, firmware is
/// written without changing the boot selector, and boot selection happens last.
///
/// Returns the number of items successfully downloaded.
pub fn recovery_download_board_bundle_for(
    catalog_url: &str,
    board_name: &str,
) -> anyhow::Result<u32> {
    info!("Board: {}", board_name);
    println!("Board: {}", board_name);

    BUNDLE_PROGRESS.store(0, Ordering::Relaxed);

    info!("Fetching catalog: {}", catalog_url);
    let catalog_json = http_get_string(catalog_url, MAX_CATALOG_SIZE)?;

    let chip = detect_chip();
    info!("Verifying signed manifests for chip: {}", chip);
    let verified_manifests = fetch_verified_manifests(&catalog_json)?;
    let mut selected = Vec::new();
    for artifact in verified_manifests {
        if artifact.manifest.arch != chip
            || !artifact
                .manifest
                .compatible_boards
                .iter()
                .any(|candidate| candidate == board_name)
            || (artifact.manifest.artifact_type == ArtifactType::Board
                && artifact.manifest.id != board_name)
        {
            continue;
        }
        artifact
            .manifest
            .validate_for_device(chip, board_name, 1)
            .map_err(anyhow::Error::msg)?;
        selected.push(artifact);
    }

    if selected.is_empty() {
        anyhow::bail!(
            "Catalog has no signed compatible bundle entries for '{}'",
            board_name
        );
    }

    let total_entries = u32::try_from(selected.len())?;
    let firmware_count = selected
        .iter()
        .filter(|artifact| artifact.manifest.artifact_type == ArtifactType::Firmware)
        .count();
    let board_count = selected
        .iter()
        .filter(|artifact| artifact.manifest.artifact_type == ArtifactType::Board)
        .count();
    if firmware_count != 1 || board_count != 1 {
        anyhow::bail!("Signed bundle must contain exactly one firmware and one board profile");
    }

    let mut downloaded = 0u32;
    let download_stage = DownloadStage::new(Path::new(BUNDLE_DOWNLOAD_DIR))?;
    let mut bundle_files = Vec::new();
    let mut firmware_path: Option<std::path::PathBuf> = None;

    let mut destinations = std::collections::BTreeSet::new();
    for artifact in selected {
        let manifest = &artifact.manifest;
        if !destinations.insert(manifest.destination.clone()) {
            anyhow::bail!(
                "Signed bundle contains duplicate destination '{}'",
                manifest.destination
            );
        }
        validate_catalog_id(&manifest.id).map_err(anyhow::Error::msg)?;

        if manifest.artifact_type == ArtifactType::Firmware {
            if firmware_path.is_some() {
                anyhow::bail!("Signed bundle contains multiple firmware images");
            }
            info!("Downloading signed firmware {}", manifest.id);
            println!("  {} [firmware, staged on SD]", manifest.id);
            let staged = download_stage.path(&format!("{}-firmware.bin", downloaded))?;
            download_verified_payload(manifest, &staged)?;
            let staged_manifest =
                download_stage.write(&format!("{}-manifest", downloaded), &artifact.canonical)?;
            let staged_signature = download_stage.write(
                &format!("{}-manifest-signature", downloaded),
                &artifact.signature,
            )?;
            bundle_files.push(BundleFile::from_file(
                "config/firmware.manifest",
                staged_manifest,
            )?);
            bundle_files.push(BundleFile::from_file(
                "config/firmware.manifest.sig",
                staged_signature,
            )?);
            firmware_path = Some(staged);
        } else {
            info!(
                "Downloading signed {} -> {}",
                manifest.id, manifest.destination
            );
            println!("  {} [{}]", manifest.id, manifest.artifact_type.as_str());
            let staged = download_stage.path(&format!("{}-payload", downloaded))?;
            download_verified_payload(manifest, &staged)?;
            let relative = std::path::PathBuf::from(&manifest.destination);
            if manifest.artifact_type == ArtifactType::Board {
                bundle_files.push(BundleFile::from_file("config/board.json", &staged)?);
            }
            bundle_files.push(BundleFile::from_file(&relative, &staged)?);
            let staged_manifest =
                download_stage.write(&format!("{}-manifest", downloaded), &artifact.canonical)?;
            let staged_signature = download_stage.write(
                &format!("{}-manifest-signature", downloaded),
                &artifact.signature,
            )?;
            bundle_files.push(BundleFile::from_file(
                format!("{}.manifest", relative.to_string_lossy()),
                &staged_manifest,
            )?);
            bundle_files.push(BundleFile::from_file(
                format!("{}.manifest.sig", relative.to_string_lossy()),
                &staged_signature,
            )?);
            if manifest.artifact_type == ArtifactType::Board {
                bundle_files.push(BundleFile::from_file(
                    "config/board.json.manifest",
                    staged_manifest,
                )?);
                bundle_files.push(BundleFile::from_file(
                    "config/board.json.manifest.sig",
                    staged_signature,
                )?);
            }
        }

        downloaded += 1;
        info!("Verified signed artifact '{}'", manifest.id);

        // Update progress
        if total_entries > 0 {
            let pct = (downloaded * 80 / total_entries).min(80) as u8;
            BUNDLE_PROGRESS.store(pct, Ordering::Relaxed);
        }
    }

    let firmware_path =
        firmware_path.ok_or_else(|| anyhow::anyhow!("Catalog has no compatible firmware image"))?;
    let sdcard_root = Path::new("/sdcard");
    bundle_transaction::install(
        sdcard_root,
        &bundle_files,
        || write_ota1_file(&firmware_path).map_err(transaction_io_error),
        || select_ota1_for_boot().map_err(transaction_io_error),
        |boundary| {
            log_transaction_boundary(boundary);
            Ok(())
        },
    )?;

    BUNDLE_PROGRESS.store(100, Ordering::Relaxed);
    info!(
        "Bundle download complete: {} item(s) installed for board '{}'",
        downloaded, board_name
    );
    println!("Bundle complete: {} items installed", downloaded);
    Ok(downloaded)
}

fn sdcard_relative_path(path: &str) -> anyhow::Result<std::path::PathBuf> {
    Path::new(path)
        .strip_prefix("/sdcard")
        .map(|relative| relative.to_path_buf())
        .map_err(|_| anyhow::anyhow!("Bundle destination is outside /sdcard: {}", path))
}

fn transaction_io_error(error: anyhow::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error.to_string())
}

struct DownloadStage {
    root: std::path::PathBuf,
}

impl DownloadStage {
    fn new(root: &Path) -> io::Result<Self> {
        if root.exists() {
            std::fs::remove_dir_all(root)?;
        }
        std::fs::create_dir_all(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    fn write(&self, name: &str, data: &[u8]) -> io::Result<std::path::PathBuf> {
        let path = self.path(name)?;
        let mut file = std::fs::File::create(&path)?;
        file.write_all(data)?;
        file.sync_all()?;
        Ok(path)
    }

    fn path(&self, name: &str) -> io::Result<std::path::PathBuf> {
        if name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name == "."
            || name == ".."
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid download staging name",
            ));
        }
        Ok(self.root.join(name))
    }
}

impl Drop for DownloadStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn log_transaction_boundary(boundary: Boundary) {
    info!("Bundle transaction boundary: {:?}", boundary);
}

/// Resolve a transaction left by a reset, failed boot, or failed cleanup.
/// Only a firmware image that reached ESP-IDF's VALID state may commit the
/// matching filesystem generation; every other state restores the old files.
pub fn recover_interrupted_bundle() -> anyhow::Result<()> {
    let root = Path::new("/sdcard");
    let ota_selected = ota1_is_boot_partition();
    let ota_valid = matches!(check_ota1(), Ota1State::Valid);
    match bundle_transaction::recovery_action(root, ota_selected, ota_valid)? {
        RecoveryAction::None => {}
        RecoveryAction::Rollback => {
            warn!("Rolling back interrupted bundle transaction");
            bundle_transaction::rollback(root)?;
        }
        RecoveryAction::Finalize => {
            info!("Finalizing confirmed bundle transaction");
            bundle_transaction::finalize(root)?;
        }
    }
    if Path::new(BUNDLE_DOWNLOAD_DIR).exists() {
        std::fs::remove_dir_all(BUNDLE_DOWNLOAD_DIR)?;
    }
    Ok(())
}

fn ota1_is_boot_partition() -> bool {
    unsafe {
        let ota1 = esp_ota_get_next_update_partition(std::ptr::null());
        let selected = esp_ota_get_boot_partition();
        !ota1.is_null() && selected == ota1
    }
}

/// Build a dry-run JSON plan for the bundle entries recovery would download.
///
/// This uses the same catalog, chip, and board matching rules as the installer,
/// but does not write to flash or SD card.
pub fn recovery_bundle_plan_json(catalog_url: &str, board_name: &str) -> anyhow::Result<String> {
    let catalog_json = http_get_string(catalog_url, MAX_CATALOG_SIZE)?;
    let chip = detect_chip();
    let mut entries: Vec<String> = Vec::new();

    for artifact in fetch_verified_manifests(&catalog_json)? {
        let manifest = artifact.manifest;
        if manifest.arch != chip
            || !manifest
                .compatible_boards
                .iter()
                .any(|candidate| candidate == board_name)
            || (manifest.artifact_type == ArtifactType::Board && manifest.id != board_name)
        {
            continue;
        }
        manifest
            .validate_for_device(chip, board_name, 1)
            .map_err(anyhow::Error::msg)?;
        entries.push(format!(
            r#"{{"id":"{}","type":"{}","version":"{}","destination":"{}"}}"#,
            manifest.id,
            manifest.artifact_type.as_str(),
            manifest.version,
            manifest.destination,
        ));
    }

    Ok(format!(
        r#"{{"ok":true,"board":"{}","chip":"{}","catalog_source":"{}","count":{},"entries":[{}]}}"#,
        board_name,
        chip,
        catalog_url,
        entries.len(),
        entries.join(",")
    ))
}

fn find_catalog_entry_by_type<'a>(json: &'a str, entry_type: &str) -> Option<&'a str> {
    iter_json_objects(json)
        .find(|obj| json_extract_string(obj, "type").as_deref() == Some(entry_type))
}

/// HTTP GET returning a bounded string.
fn http_get_string(url: &str, max_size: usize) -> anyhow::Result<String> {
    let bytes = http_get_bytes(url, max_size)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn http_get_bytes(url: &str, max_size: usize) -> anyhow::Result<Vec<u8>> {
    with_http_response(url, max_size, |mut response, declared| {
        let mut body = Vec::with_capacity(declared.unwrap_or(0).min(max_size));
        let mut limit = bounded_io::BodyLimit::new(declared, max_size)?;
        let mut buffer = [0u8; 4096];
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|error| anyhow::anyhow!("HTTP read failed: {}", error))?;
            if count == 0 {
                break;
            }
            limit.observe(count)?;
            body.extend_from_slice(&buffer[..count]);
        }
        limit.finish()?;
        Ok(body)
    })
}

fn http_get_to_file(url: &str, max_size: usize, destination: &Path) -> anyhow::Result<usize> {
    with_http_response(url, max_size, |mut response, declared| {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(destination)?;
        let mut limit = bounded_io::BodyLimit::new(declared, max_size)?;
        let mut buffer = [0u8; 4096];
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|error| anyhow::anyhow!("HTTP read failed: {}", error))?;
            if count == 0 {
                break;
            }
            limit.observe(count)?;
            file.write_all(&buffer[..count])?;
        }
        let written = limit.finish()?;
        file.sync_all()?;
        Ok(written)
    })
}

fn with_http_response<T>(
    url: &str,
    max_size: usize,
    consume: impl FnOnce(
        embedded_svc::http::client::Response<&mut EspHttpConnection>,
        Option<usize>,
    ) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    use embedded_svc::http::client::Client;

    let connection = EspHttpConnection::new(&HttpConfig {
        timeout: Some(std::time::Duration::from_secs(30)),
        ..Default::default()
    })?;

    let mut client = Client::wrap(connection);
    let response = client.get(url)?.submit()?;
    let status = response.status();

    if status != 200 {
        anyhow::bail!("HTTP {} for {}", status, url);
    }

    let declared = bounded_io::validate_framing(
        response.header("Content-Length"),
        response.header("Transfer-Encoding"),
        max_size,
    )?;
    consume(response, declared)
}
