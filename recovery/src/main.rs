// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Recovery OS — Rust implementation
//
// Minimal firmware for ota_0 that:
// 1. Checks if ota_1 has valid firmware → boots it
// 2. Checks SD card for firmware update → flashes to ota_1
// 3. Starts WiFi AP + captive portal web UI → user configures WiFi / selects board
// 4. Downloads and verifies the selected board bundle → flashes / installs
// 5. Reboots into ThistleOS

#![allow(dead_code)]

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys as esp_idf_sys;
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi,
};

use log::*;

mod recovery_ota;
mod recovery_web;

const VERSION: &str = "0.1.0";
const AP_SSID: &str = "ThistleOS-Recovery";
const BUNDLE_CATALOG_URL: &str = "https://wan0net.github.io/thistle-apps/catalog.json";
const BOARD_CATALOG_URL: &str = "https://wan0net.github.io/thistle-os/catalog.json";

fn main() -> anyhow::Result<()> {
    // Initialize ESP-IDF
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("========================================");
    info!("  ThistleOS Recovery v{}", VERSION);
    info!("========================================");

    // Detect chip early so it's available for catalog filtering and the web UI.
    let chip = recovery_ota::detect_chip();
    info!(
        "Chip: {} ({})",
        chip.to_uppercase(),
        recovery_ota::chip_arch_family(chip)
    );
    println!(
        "Chip: {} ({})",
        chip.to_uppercase(),
        recovery_ota::chip_arch_family(chip)
    );
    {
        let mut st = recovery_web::STATE.lock().unwrap();
        st.chip = chip.to_string();
    }

    // Step 1: Check if ota_1 has valid firmware
    info!("Checking ota_1 partition...");
    match recovery_ota::check_ota1() {
        recovery_ota::Ota1State::Valid => {
            info!("ota_1 has valid firmware — booting main OS");
            println!("Booting ThistleOS...");
            recovery_ota::boot_ota1()?;
            // unreachable — device reboots
        }
        recovery_ota::Ota1State::PendingVerify => {
            info!("ota_1 pending verification — booting to verify");
            recovery_ota::boot_ota1()?;
        }
        recovery_ota::Ota1State::Invalid => {
            info!("ota_1 is invalid or empty");
            println!("Main OS not found — entering Recovery mode");
        }
        recovery_ota::Ota1State::NotFound => {
            error!("No ota_1 partition found!");
            println!("ERROR: Partition table missing ota_1");
        }
    }

    // Step 2: Check SD card for firmware
    info!("Checking SD card for firmware...");
    println!("Checking SD card...");
    if recovery_ota::check_sd_firmware() {
        println!("Firmware found on SD card! Installing...");
        match recovery_ota::apply_sd_firmware() {
            Ok(()) => {
                println!("Installed! Rebooting...");
                FreeRtos::delay_ms(1000);
                unsafe { esp_idf_sys::esp_restart() };
            }
            Err(e) => {
                println!("SD install failed: {}", e);
            }
        }
    } else {
        println!("No firmware on SD card");
    }

    // Board/component knowledge is catalog-led. Recovery avoids generic probing
    // so it does not drive board-specific pins before the user selects hardware.
    println!("Board selection is catalog-driven — select your board in the web UI");

    // Step 3: Start WiFi AP + captive portal
    info!("Starting WiFi Access Point: {}", AP_SSID);
    println!("\nStarting WiFi hotspot: {}", AP_SSID);
    println!("Connect your phone/laptop and open http://192.168.4.1");

    // Seed catalog URLs in shared state so handlers can reference them.
    {
        let mut st = recovery_web::STATE.lock().unwrap();
        st.catalog_url = BUNDLE_CATALOG_URL.to_string();
        st.board_catalog_url = BOARD_CATALOG_URL.to_string();
    }

    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(
            Peripherals::take()?.modem,
            sysloop.clone(),
            Some(nvs.clone()),
        )?,
        sysloop.clone(),
    )?;

    // Configure as AP (hotspot) + STA (client) simultaneously
    wifi.set_configuration(&Configuration::Mixed(
        ClientConfiguration::default(),
        AccessPointConfiguration {
            ssid: AP_SSID.try_into().unwrap(),
            auth_method: AuthMethod::None,
            max_connections: 4,
            ..Default::default()
        },
    ))?;

    wifi.start()?;
    info!("WiFi AP started: {} (192.168.4.1)", AP_SSID);

    // Step 4: Start HTTP server for captive portal
    let mut server = EspHttpServer::new(&HttpConfig::default())?;
    recovery_web::register_handlers(&mut server)?;
    info!("Captive portal running at http://192.168.4.1");

    // Step 5: captive portal control loop
    println!("\n========================================");
    println!("  ThistleOS Recovery — Web Mode");
    println!("========================================");
    println!(
        "Use http://192.168.4.1 from any device connected to '{}'.",
        AP_SSID
    );
    println!("Recovery will keep polling web requests until install/reboot.");

    loop {
        poll_web_state(&mut wifi);
        FreeRtos::delay_ms(100);
    }
}

// ---------------------------------------------------------------------------
// Poll shared web UI state and act on any pending requests
// ---------------------------------------------------------------------------

fn poll_web_state(wifi: &mut BlockingWifi<EspWifi>) {
    // Check for pending WiFi connect request from web UI
    let wifi_req = {
        let mut st = recovery_web::STATE.lock().unwrap();
        st.wifi_request.take()
    };

    if let Some((ssid, pass)) = wifi_req {
        info!("Web UI requested WiFi connect to '{}'", ssid);
        match do_wifi_connect(wifi, &ssid, &pass) {
            Ok(ip) => {
                info!("Web UI WiFi connect succeeded: {}", ip);
                let mut st = recovery_web::STATE.lock().unwrap();
                st.wifi_connected = true;
                st.wifi_ip = ip;
            }
            Err(e) => {
                error!("Web UI WiFi connect failed: {:?}", e);
                let mut st = recovery_web::STATE.lock().unwrap();
                st.wifi_connected = false;
                st.wifi_ip.clear();
            }
        }
    }

    // Check for pending bundle download request from web UI
    let (bundle_req, board_name, catalog_url) = {
        let mut st = recovery_web::STATE.lock().unwrap();
        let req = st.bundle_request;
        if req {
            st.bundle_request = false;
        }
        (req, st.board_name.clone(), st.catalog_url.clone())
    };

    if bundle_req {
        let board = board_name.as_deref().unwrap_or("tdeck-pro");
        let url = if catalog_url.is_empty() {
            BUNDLE_CATALOG_URL
        } else {
            &catalog_url
        };
        info!("Web UI requested bundle download for board '{}'", board);
        println!("Web UI: downloading bundle for board '{}'...", board);

        match recovery_ota::recovery_download_board_bundle_for(url, board) {
            Ok(count) => {
                info!("Bundle download complete: {} items", count);
                let mut st = recovery_web::STATE.lock().unwrap();
                st.bundle_status = "complete".to_string();
                st.bundle_progress = count.min(255) as u8;
            }
            Err(e) => {
                error!("Bundle download failed: {}", e);
                let mut st = recovery_web::STATE.lock().unwrap();
                st.bundle_status = format!("error: {}", e);
                st.bundle_progress = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// WiFi STA connect helper (used by both UART and web UI paths)
// ---------------------------------------------------------------------------

fn do_wifi_connect(
    wifi: &mut BlockingWifi<EspWifi>,
    ssid: &str,
    pass: &str,
) -> anyhow::Result<String> {
    wifi.set_configuration(&Configuration::Mixed(
        ClientConfiguration {
            ssid: ssid.try_into().unwrap_or_default(),
            password: pass.try_into().unwrap_or_default(),
            ..Default::default()
        },
        AccessPointConfiguration {
            ssid: AP_SSID.try_into().unwrap(),
            auth_method: AuthMethod::None,
            max_connections: 4,
            ..Default::default()
        },
    ))?;

    wifi.connect()?;
    wifi.wait_netif_up()?;

    let ip = wifi
        .wifi()
        .sta_netif()
        .get_ip_info()
        .map(|info| info.ip.to_string())
        .unwrap_or_else(|_| "assigned".to_string());

    Ok(ip)
}
