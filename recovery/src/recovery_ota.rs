// SPDX-License-Identifier: BSD-3-Clause
// Recovery OTA — check/flash firmware from SD card or HTTP

use esp_idf_svc::http::client::{Configuration as HttpConfig, EspHttpConnection};
use esp_idf_sys::*;
use log::*;

const SD_FIRMWARE_PATH: &str = "/sdcard/update/thistle_os.bin";
const MAX_FIRMWARE_SIZE: usize = 4 * 1024 * 1024; // 4MB

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
            x if x == esp_ota_img_states_t_ESP_OTA_IMG_PENDING_VERIFY => {
                Ota1State::PendingVerify
            }
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

    let data = std::fs::read(SD_FIRMWARE_PATH)?;
    if data.is_empty() || data.len() > MAX_FIRMWARE_SIZE {
        anyhow::bail!("Invalid firmware size: {} bytes", data.len());
    }

    flash_to_ota1(&data)?;
    Ok(())
}

/// Download firmware from app store catalog and flash to ota_1
pub fn download_and_flash(catalog_url: &str) -> anyhow::Result<()> {
    info!("Fetching catalog: {}", catalog_url);

    // Fetch catalog JSON
    let catalog_json = http_get_string(catalog_url)?;

    // Find the firmware entry (type = "firmware")
    let fw_url = extract_firmware_url(&catalog_json)
        .ok_or_else(|| anyhow::anyhow!("No firmware entry in catalog"))?;

    info!("Downloading firmware: {}", fw_url);
    println!("Downloading: {}", fw_url);

    // Download firmware binary
    let firmware_data = http_get_bytes(&fw_url)?;
    info!("Downloaded {} bytes", firmware_data.len());
    println!("Downloaded {} bytes. Flashing...", firmware_data.len());

    // Flash to ota_1
    flash_to_ota1(&firmware_data)?;

    info!("Firmware flashed successfully");
    Ok(())
}

/// Write firmware data to the ota_1 partition
fn flash_to_ota1(data: &[u8]) -> anyhow::Result<()> {
    unsafe {
        let part = esp_ota_get_next_update_partition(std::ptr::null());
        if part.is_null() {
            anyhow::bail!("No OTA update partition");
        }

        let mut handle: esp_ota_handle_t = 0;
        let ret = esp_ota_begin(part, data.len(), &mut handle);
        if ret != ESP_OK as i32 {
            anyhow::bail!("esp_ota_begin failed: {}", ret);
        }

        // Write in 4KB chunks
        let chunk_size = 4096;
        let total = data.len();
        let mut written = 0;
        while written < total {
            let end = std::cmp::min(written + chunk_size, total);
            let ret = esp_ota_write(handle, data[written..end].as_ptr() as *const _, end - written);
            if ret != ESP_OK as i32 {
                esp_ota_abort(handle);
                anyhow::bail!("esp_ota_write failed at offset {}: {}", written, ret);
            }
            written = end;

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

        let ret = esp_ota_set_boot_partition(part);
        if ret != ESP_OK as i32 {
            anyhow::bail!("esp_ota_set_boot_partition failed: {}", ret);
        }

        info!("OTA flash complete, boot partition set");
        Ok(())
    }
}

/// Simple JSON extraction — find "url" value for the first "firmware" type entry
fn extract_firmware_url(json: &str) -> Option<String> {
    // Find "type":"firmware" then find the "url" in the same object
    let fw_pos = json.find("\"firmware\"")?;
    let obj_start = json[..fw_pos].rfind('{')?;
    let obj_end = json[fw_pos..].find('}').map(|p| fw_pos + p)?;
    let obj = &json[obj_start..=obj_end];

    // Extract "url" value
    let url_key = obj.find("\"url\"")?;
    let colon = obj[url_key..].find(':')?;
    let quote_start = obj[url_key + colon..].find('"').map(|p| url_key + colon + p + 1)?;
    let quote_end = obj[quote_start..].find('"').map(|p| quote_start + p)?;

    Some(obj[quote_start..quote_end].to_string())
}

/// HTTP GET returning a string
fn http_get_string(url: &str) -> anyhow::Result<String> {
    let bytes = http_get_bytes(url)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// HTTP GET returning bytes
fn http_get_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
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

    let mut body = Vec::new();
    let mut reader = response;
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&buf[..n]),
            Err(e) => anyhow::bail!("Read error: {}", e),
        }
    }

    Ok(body)
}
