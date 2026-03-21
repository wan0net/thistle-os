// SPDX-License-Identifier: BSD-3-Clause
// Recovery Web UI — captive portal served from the ESP32's WiFi AP
//
// User connects phone/laptop to "ThistleOS-Recovery" WiFi → opens browser →
// gets this web UI at 192.168.4.1 for WiFi config and firmware download.

use esp_idf_svc::http::server::EspHttpServer;
use log::*;

const RECOVERY_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ThistleOS Recovery</title>
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { font-family: system-ui, -apple-system, sans-serif; background: #111110; color: #ededed; padding: 20px; max-width: 480px; margin: 0 auto; }
  h1 { color: #2563eb; margin-bottom: 8px; font-size: 24px; }
  h2 { color: #a09f9b; font-size: 14px; margin-bottom: 20px; }
  .card { background: #1c1c1b; border: 1px solid #282826; border-radius: 8px; padding: 16px; margin-bottom: 12px; }
  .card h3 { color: #ededed; margin-bottom: 8px; font-size: 16px; }
  label { display: block; color: #a09f9b; font-size: 13px; margin-bottom: 4px; }
  input { width: 100%; padding: 10px; background: #282826; border: 1px solid #343432; border-radius: 6px; color: #ededed; font-size: 14px; margin-bottom: 8px; }
  input:focus { border-color: #2563eb; outline: none; }
  button { width: 100%; padding: 12px; background: #2563eb; color: white; border: none; border-radius: 6px; font-size: 14px; cursor: pointer; margin-top: 4px; }
  button:active { background: #1d4ed8; }
  .status { padding: 10px; background: #1a2e1a; border: 1px solid #166534; border-radius: 6px; color: #22c55e; font-size: 13px; margin-top: 12px; display: none; }
  .status.error { background: #2c1515; border-color: #991b1b; color: #ef4444; }
  .info { color: #a09f9b; font-size: 12px; margin-top: 8px; }
  .progress { width: 100%; height: 8px; background: #282826; border-radius: 4px; overflow: hidden; margin-top: 8px; display: none; }
  .progress-bar { height: 100%; background: #2563eb; width: 0%; transition: width 0.3s; }
</style>
</head>
<body>
<h1>ThistleOS Recovery</h1>
<h2>v0.1.0 — Firmware Recovery Mode</h2>

<div class="card">
  <h3>1. Connect to WiFi</h3>
  <form id="wifi-form" onsubmit="connectWifi(event)">
    <label>Network Name (SSID)</label>
    <input type="text" id="ssid" placeholder="Your WiFi network" required>
    <label>Password</label>
    <input type="password" id="password" placeholder="WiFi password">
    <button type="submit">Connect</button>
  </form>
  <div id="wifi-status" class="status"></div>
</div>

<div class="card">
  <h3>2. Download Firmware</h3>
  <p class="info">Downloads the latest ThistleOS from the app store and installs it.</p>
  <button onclick="downloadFirmware()">Download & Install ThistleOS</button>
  <div class="progress" id="dl-progress">
    <div class="progress-bar" id="dl-bar"></div>
  </div>
  <div id="dl-status" class="status"></div>
</div>

<div class="card">
  <h3>System</h3>
  <button onclick="location.href='/api/status'">View Status</button>
  <button onclick="reboot()" style="background:#dc2626;margin-top:8px;">Reboot Device</button>
</div>

<script>
async function connectWifi(e) {
  e.preventDefault();
  const ssid = document.getElementById('ssid').value;
  const pass = document.getElementById('password').value;
  const st = document.getElementById('wifi-status');
  st.style.display = 'block';
  st.className = 'status';
  st.textContent = 'Connecting...';
  try {
    const r = await fetch('/api/wifi/connect', {
      method: 'POST',
      headers: {'Content-Type':'application/json'},
      body: JSON.stringify({ssid, password: pass})
    });
    const d = await r.json();
    if (d.ok) {
      st.textContent = 'Connected! IP: ' + (d.ip || 'assigned');
    } else {
      st.className = 'status error';
      st.textContent = 'Failed: ' + (d.error || 'unknown');
    }
  } catch(e) {
    st.className = 'status error';
    st.textContent = 'Error: ' + e.message;
  }
}

async function downloadFirmware() {
  const st = document.getElementById('dl-status');
  const prog = document.getElementById('dl-progress');
  const bar = document.getElementById('dl-bar');
  st.style.display = 'block';
  prog.style.display = 'block';
  st.className = 'status';
  st.textContent = 'Downloading firmware...';
  bar.style.width = '10%';
  try {
    const r = await fetch('/api/ota/download', {method:'POST'});
    bar.style.width = '90%';
    const d = await r.json();
    if (d.ok) {
      bar.style.width = '100%';
      st.textContent = 'Firmware installed! Rebooting in 3 seconds...';
      setTimeout(() => { st.textContent = 'Rebooting...'; fetch('/api/reboot',{method:'POST'}); }, 3000);
    } else {
      st.className = 'status error';
      st.textContent = 'Failed: ' + (d.error || 'unknown');
    }
  } catch(e) {
    st.className = 'status error';
    st.textContent = 'Error: ' + e.message;
  }
}

function reboot() {
  if (confirm('Reboot the device?')) {
    fetch('/api/reboot', {method:'POST'});
  }
}
</script>
</body>
</html>"#;

pub fn register_handlers(server: &mut EspHttpServer) -> anyhow::Result<()> {
    // Serve the main recovery page
    server.fn_handler("/", esp_idf_svc::http::Method::Get, |req| -> anyhow::Result<()> {
        let mut resp = req.into_response(200, None, &[])?;
        resp.write(RECOVERY_HTML.as_bytes())?;
        Ok(())
    })?;

    // Captive portal redirect — iOS/Android check these
    for path in &["/generate_204", "/hotspot-detect.html", "/connecttest.txt"] {
        server.fn_handler(path, esp_idf_svc::http::Method::Get, |req| -> anyhow::Result<()> {
            let mut resp = req.into_response(302, None, &[("Location", "/")])?;
            resp.write(b"Redirecting to recovery portal...")?;
            Ok(())
        })?;
    }

    // API: status
    server.fn_handler("/api/status", esp_idf_svc::http::Method::Get, |req| -> anyhow::Result<()> {
        let status = format!(
            r#"{{"version":"{}","mode":"recovery","ota1":"{:?}","sd_firmware":{}}}"#,
            super::VERSION,
            crate::recovery_ota::check_ota1(),
            crate::recovery_ota::check_sd_firmware(),
        );
        let mut resp = req.into_response(200, None, &[])?;
        resp.write(status.as_bytes())?;
        Ok(())
    })?;

    // API: reboot
    server.fn_handler("/api/reboot", esp_idf_svc::http::Method::Post, |req| -> anyhow::Result<()> {
        let mut resp = req.into_response(200, None, &[])?;
        resp.write(b"{\"ok\":true}")?;
        info!("Reboot requested via web UI");
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(1));
            unsafe { esp_idf_sys::esp_restart() };
        });
        Ok(())
    })?;

    // TODO: /api/wifi/connect — POST {ssid, password}
    // TODO: /api/ota/download — POST, triggers download_and_flash

    info!("Web UI handlers registered (captive portal active)");
    Ok(())
}
