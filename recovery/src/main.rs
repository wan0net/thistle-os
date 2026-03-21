// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Recovery OS — Rust implementation
//
// Minimal firmware for ota_0 that:
// 1. Checks if ota_1 has valid firmware → boots it
// 2. Checks SD card for firmware update → flashes to ota_1
// 3. Starts WiFi AP + captive portal web UI → user configures WiFi
// 4. Downloads firmware from app store → flashes to ota_1
// 5. Falls back to UART console for manual recovery

use esp_idf_hal::prelude::*;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration,
    EspWifi,
};
use esp_idf_sys as _;

use log::*;
use std::io::{BufRead, Write};
use std::net::Ipv4Addr;

mod recovery_ota;
mod recovery_web;

const VERSION: &str = "0.1.0";
const AP_SSID: &str = "ThistleOS-Recovery";
const CATALOG_URL: &str = "https://wan0net.github.io/thistle-apps/catalog.json";

fn main() -> anyhow::Result<()> {
    // Initialize ESP-IDF
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("========================================");
    info!("  ThistleOS Recovery v{}", VERSION);
    info!("========================================");

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

    // Step 3: Start WiFi AP + captive portal
    info!("Starting WiFi Access Point: {}", AP_SSID);
    println!("\nStarting WiFi hotspot: {}", AP_SSID);
    println!("Connect your phone/laptop and open http://192.168.4.1");

    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(Peripherals::take()?.modem, sysloop.clone(), Some(nvs.clone()))?,
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

    // Step 5: UART console loop
    println!("\n========================================");
    println!("  ThistleOS Recovery — Interactive Mode");
    println!("========================================");
    println!("Options:");
    println!("  scan              — Scan WiFi networks");
    println!("  connect SSID,PASS — Connect to WiFi");
    println!("  download          — Download firmware from app store");
    println!("  reboot            — Restart device");
    println!("  status            — Show current state");
    println!("  help              — Show this message");
    println!("");
    println!("Or use the web UI at http://192.168.4.1 from any device");
    println!("connected to the '{}' WiFi network.", AP_SSID);
    println!("");

    let stdin = std::io::stdin();
    loop {
        print!("recovery> ");
        std::io::stdout().flush().ok();

        let mut line = String::new();
        // Non-blocking: check UART every 100ms, also let web server handle requests
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => {
                FreeRtos::delay_ms(100);
                continue;
            }
            Ok(_) => {}
        }

        let cmd = line.trim();
        if cmd.is_empty() {
            continue;
        }

        match cmd {
            "help" => {
                println!("Commands: scan, connect, download, reboot, status, help");
            }
            "scan" => {
                println!("Scanning...");
                wifi.scan().ok();
                if let Ok(results) = wifi.scan() {
                    for (i, ap) in results.iter().enumerate().take(15) {
                        let lock = if ap.auth_method == AuthMethod::None {
                            "[open]"
                        } else {
                            "[secured]"
                        };
                        println!(
                            "  {:2}. {:<32} {:4} dBm  ch{:2}  {}",
                            i + 1,
                            ap.ssid,
                            ap.signal_strength,
                            ap.channel,
                            lock
                        );
                    }
                    println!("Found {} networks", results.len());
                }
            }
            "reboot" => {
                println!("Rebooting...");
                FreeRtos::delay_ms(500);
                unsafe { esp_idf_sys::esp_restart() };
            }
            "status" => {
                println!("Recovery v{}", VERSION);
                println!("WiFi AP: {} (192.168.4.1)", AP_SSID);
                let connected = wifi.is_connected().unwrap_or(false);
                println!("WiFi STA: {}", if connected { "connected" } else { "disconnected" });
                println!("ota_1: {:?}", recovery_ota::check_ota1());
                println!("SD card firmware: {}", recovery_ota::check_sd_firmware());
            }
            "download" => {
                let connected = wifi.is_connected().unwrap_or(false);
                if !connected {
                    println!("Not connected to WiFi. Use 'connect SSID,PASS' first.");
                } else {
                    println!("Downloading firmware from {}...", CATALOG_URL);
                    match recovery_ota::download_and_flash(CATALOG_URL) {
                        Ok(()) => {
                            println!("Firmware installed! Rebooting...");
                            FreeRtos::delay_ms(1000);
                            unsafe { esp_idf_sys::esp_restart() };
                        }
                        Err(e) => println!("Download failed: {}", e),
                    }
                }
            }
            _ if cmd.starts_with("connect ") => {
                let args = &cmd[8..];
                if let Some(comma) = args.find(',') {
                    let ssid = &args[..comma];
                    let pass = &args[comma + 1..];
                    println!("Connecting to '{}'...", ssid);

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
                    ))
                    .ok();

                    match wifi.connect() {
                        Ok(()) => {
                            wifi.wait_netif_up().ok();
                            println!("Connected!");
                            if let Ok(info) = wifi.wifi().sta_netif().get_ip_info() {
                                println!("IP: {}", info.ip);
                            }
                        }
                        Err(e) => println!("Connection failed: {:?}", e),
                    }
                } else {
                    println!("Usage: connect SSID,PASSWORD");
                }
            }
            _ => {
                println!("Unknown command: '{}'. Type 'help' for options.", cmd);
            }
        }
    }
}
