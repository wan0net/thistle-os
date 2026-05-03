// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Recovery OS — Rust implementation
//
// Minimal firmware for ota_0 that:
// 1. Checks if ota_1 has valid firmware → boots it
// 2. Checks SD card for firmware update → flashes to ota_1
// 3. Starts WiFi AP + captive portal web UI → user configures WiFi / selects board
// 4. Downloads full bundle from app store (firmware + drivers + WM) → flashes / installs
// 5. Falls back to UART console for manual recovery

#![allow(dead_code)]

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration,
    EspWifi,
};
use esp_idf_svc::sys as esp_idf_sys;

use log::*;
use std::io::{BufRead, Write};

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

    // Detect chip early so it's available for catalog filtering and the web UI.
    let chip = recovery_ota::detect_chip();
    info!("Chip: {} ({})", chip.to_uppercase(), recovery_ota::chip_arch_family(chip));
    println!("Chip: {} ({})", chip.to_uppercase(), recovery_ota::chip_arch_family(chip));
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

    // Step 2.5: Scan hardware components and auto-detect board
    info!("Scanning hardware components...");
    println!("Scanning hardware...");
    let hw_components = recovery_ota::scan_hardware();
    {
        let mut st = recovery_web::STATE.lock().unwrap();
        // Populate detected_components for the web UI /api/boards response
        for c in &hw_components {
            let name = recovery_ota::component_device_name(c).to_string();
            st.detected_components.push((c.bus.clone(), c.address, name));
        }
    }
    // Derive board slug from the detected component set (for firmware/WM selection)
    let has_tca8418 = hw_components.iter().any(|c| c.bus == "i2c" && c.address == 0x34);
    let has_bhi260  = hw_components.iter().any(|c| c.bus == "i2c" && c.address == 0x28);
    let board_slug: Option<&str> = if has_tca8418 && has_bhi260 {
        Some("tdeck-pro")
    } else if has_tca8418 {
        Some("tdeck")
    } else {
        None
    };
    if let Some(board) = board_slug {
        println!("Detected board: {}", board);
        if let Ok(mut st) = recovery_web::STATE.lock() {
            st.board_name = Some(board.to_string());
        }
    } else {
        println!("Board not detected — select in web UI");
    }

    // Step 3: Start WiFi AP + captive portal
    info!("Starting WiFi Access Point: {}", AP_SSID);
    println!("\nStarting WiFi hotspot: {}", AP_SSID);
    println!("Connect your phone/laptop and open http://192.168.4.1");

    // Seed the catalog URL in shared state so handlers can reference it
    {
        let mut st = recovery_web::STATE.lock().unwrap();
        st.catalog_url = CATALOG_URL.to_string();
    }

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

    // Step 5: UART console loop (also polls web UI state)
    println!("\n========================================");
    println!("  ThistleOS Recovery — Interactive Mode");
    println!("========================================");
    println!("Options:");
    println!("  scan                — Scan WiFi networks");
    println!("  connect SSID,PASS   — Connect to WiFi");
    println!("  download            — Download full bundle for selected/detected board");
    println!("  download firmware   — Download firmware only (legacy)");
    println!("  board               — Show detected/selected board");
    println!("  board <id>          — Select board manually (e.g. 'board tdeck-pro')");
    println!("  reboot              — Restart device");
    println!("  status              — Show current state");
    println!("  help                — Show this message");
    println!("");
    println!("Or use the web UI at http://192.168.4.1 from any device");
    println!("connected to the '{}' WiFi network.", AP_SSID);
    println!("");

    let stdin = std::io::stdin();
    loop {
        // ----------------------------------------------------------------
        // Poll web UI shared state — handle requests queued by HTTP handlers
        // ----------------------------------------------------------------
        poll_web_state(&mut wifi);

        // ----------------------------------------------------------------
        // UART command line
        // ----------------------------------------------------------------
        print!("recovery> ");
        std::io::stdout().flush().ok();

        let mut line = String::new();
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
                println!("Commands: scan, connect, download, download firmware, board, reboot, status, help");
            }
            "scan" => {
                println!("Scanning...");
                wifi.scan().ok();
                if let Ok(results) = wifi.scan() {
                    for (i, ap) in results.iter().enumerate().take(15) {
                        let lock = if ap.auth_method == Some(AuthMethod::None) {
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
                let chip = recovery_ota::detect_chip();
                println!("Chip: {} ({})", chip.to_uppercase(), recovery_ota::chip_arch_family(chip));
                println!("WiFi AP: {} (192.168.4.1)", AP_SSID);
                let connected = wifi.is_connected().unwrap_or(false);
                println!("WiFi STA: {}", if connected { "connected" } else { "disconnected" });
                println!("ota_1: {:?}", recovery_ota::check_ota1());
                println!("SD card firmware: {}", recovery_ota::check_sd_firmware());
                let (board, bundle_status, components) = {
                    let st = recovery_web::STATE.lock().unwrap();
                    (st.board_name.clone(), st.bundle_status.clone(), st.detected_components.clone())
                };
                println!("Board: {}", board.as_deref().unwrap_or("(not selected)"));
                println!("Bundle status: {}", if bundle_status.is_empty() { "idle" } else { &bundle_status });
                if components.is_empty() {
                    println!("Hardware: no devices detected");
                } else {
                    println!("Hardware ({} device(s) detected):", components.len());
                    for (bus, addr, name) in &components {
                        match bus.as_str() {
                            "i2c"  => println!("  I2C  0x{:02X}        {}", addr, name),
                            "spi"  => println!("  SPI  CS=GPIO{}      {}", addr, name),
                            "uart" => println!("  UART port={}        {}", addr, name),
                            _      => println!("  {}   addr={}         {}", bus.to_uppercase(), addr, name),
                        }
                    }
                }
            }
            "board" => {
                // Show current board selection
                let board = {
                    let st = recovery_web::STATE.lock().unwrap();
                    st.board_name.clone()
                };
                match board {
                    Some(b) => println!("Selected board: {}", b),
                    None    => println!("No board selected. Use 'board <id>' to select."),
                }
                println!("Known boards (esp32s3): tdeck-pro, tdeck-plus, tdeck, tdisplay-s3, t3-s3, heltec-v3, cardputer, rak3312, twatch-ultra, waveshare-esp32-s3-touch-amoled-2.06");
                println!("Known boards (esp32):   cyd-2432s022, cyd-2432s028");
                println!("Known boards (esp32c3): c3-mini");
            }
            "download" => {
                let connected = wifi.is_connected().unwrap_or(false);
                if !connected {
                    println!("Not connected to WiFi. Use 'connect SSID,PASS' first.");
                } else {
                    let board = {
                        let st = recovery_web::STATE.lock().unwrap();
                        st.board_name.clone()
                    };
                    let board_name = board.as_deref().unwrap_or("tdeck-pro");
                    println!("Downloading full bundle for board '{}' from {}...", board_name, CATALOG_URL);
                    match recovery_ota::recovery_download_board_bundle_for(CATALOG_URL, board_name) {
                        Ok(count) => {
                            println!("Bundle installed ({} items). Rebooting...", count);
                            FreeRtos::delay_ms(1000);
                            unsafe { esp_idf_sys::esp_restart() };
                        }
                        Err(e) => println!("Bundle download failed: {}", e),
                    }
                }
            }
            "download firmware" => {
                let connected = wifi.is_connected().unwrap_or(false);
                if !connected {
                    println!("Not connected to WiFi. Use 'connect SSID,PASS' first.");
                } else {
                    println!("Downloading firmware (only) from {}...", CATALOG_URL);
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
            _ if cmd.starts_with("board ") => {
                let board_id = cmd[6..].trim();
                let known = [
                    "tdeck-pro",
                    "tdeck-plus",
                    "tdeck",
                    "tdisplay-s3",
                    "t3-s3",
                    "heltec-v3",
                    "cardputer",
                    "rak3312",
                    "twatch-ultra",
                    "waveshare-esp32-s3-touch-amoled-2.06",
                    "cyd-2432s022",
                    "cyd-2432s028",
                    "c3-mini",
                ];
                if known.contains(&board_id) {
                    {
                        let mut st = recovery_web::STATE.lock().unwrap();
                        st.board_name = Some(board_id.to_string());
                    }
                    println!("Board set to '{}'", board_id);
                } else {
                    println!("Unknown board '{}'. Known boards: {}", board_id, known.join(", "));
                }
            }
            _ if cmd.starts_with("connect ") => {
                let args = &cmd[8..];
                if let Some(comma) = args.find(',') {
                    let ssid = &args[..comma];
                    let pass = &args[comma + 1..];
                    println!("Connecting to '{}'...", ssid);

                    match do_wifi_connect(&mut wifi, ssid, pass) {
                        Ok(ip) => {
                            println!("Connected! IP: {}", ip);
                            // Mirror into shared state so web UI shows correct status
                            let mut st = recovery_web::STATE.lock().unwrap();
                            st.wifi_connected = true;
                            st.wifi_ip = ip;
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
        let url   = if catalog_url.is_empty() { CATALOG_URL } else { &catalog_url };
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
