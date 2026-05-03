// SPDX-License-Identifier: BSD-3-Clause
// Recovery Web UI — captive portal served from the ESP32's WiFi AP
//
// User connects phone/laptop to "ThistleOS-Recovery" WiFi → opens browser →
// gets this web UI at 192.168.4.1 for WiFi config, board selection, and
// full bundle download.
//
// Architecture: web handlers are stateless; they read/write a shared
// `RecoveryState` protected by a Mutex.  The main loop polls that state
// and performs all operations that require owning the WiFi driver.

use esp_idf_svc::http::server::EspHttpServer;
use log::*;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

pub struct RecoveryState {
    /// Set by /api/wifi/connect; cleared by main after processing.
    pub wifi_request: Option<(String, String)>,
    /// Updated by main after WiFi connection attempt.
    pub wifi_connected: bool,
    pub wifi_ip: String,
    /// Set by /api/board/select (still used for firmware/WM selection).
    pub board_name: Option<String>,
    /// Hardware components detected by the last scan_hardware() call.
    /// Each entry is (bus, address, display_name).
    pub detected_components: Vec<(String, u16, String)>,
    /// Set by /api/bundle/download; cleared by main after processing.
    pub bundle_request: bool,
    /// "idle" | "downloading" | "complete" | "error: <msg>"
    pub bundle_status: String,
    /// 0-100
    pub bundle_progress: u8,
    pub catalog_url: String,
    /// Chip slug detected at boot, e.g. "esp32s3", "esp32c3".
    pub chip: String,
}

impl RecoveryState {
    pub const fn new() -> Self {
        RecoveryState {
            wifi_request: None,
            wifi_connected: false,
            wifi_ip: String::new(),
            board_name: None,
            detected_components: Vec::new(),
            bundle_request: false,
            bundle_status: String::new(),
            bundle_progress: 0,
            catalog_url: String::new(),
            chip: String::new(),
        }
    }
}

pub static STATE: Mutex<RecoveryState> = Mutex::new(RecoveryState::new());

// ---------------------------------------------------------------------------
// Known boards
// ---------------------------------------------------------------------------

/// (board_id, label, arch)
///
/// `arch` matches the slug returned by `detect_chip()`.  An empty string means
/// the board entry is shown regardless of the detected chip (reserved for
/// future universal boards or boards with ambiguous chip identification).
const KNOWN_BOARDS: &[(&str, &str, &str)] = &[
    ("tdeck-pro",    "LilyGo T-Deck Pro (E-Paper, Keyboard, LoRa, GPS)", "esp32s3"),
    ("tdeck-plus",   "LilyGo T-Deck Plus (LCD, Keyboard, LoRa, GPS, Power Mgmt)", "esp32s3"),
    ("tdeck",        "LilyGo T-Deck (LCD, Keyboard, LoRa, GPS)",         "esp32s3"),
    ("tdisplay-s3",  "LilyGo T-Display-S3 (LCD, Touch)",                 "esp32s3"),
    ("t3-s3",        "LilyGo T3-S3 (OLED, LoRa)",                        "esp32s3"),
    ("heltec-v3",    "Heltec WiFi LoRa 32 V3 (OLED, LoRa)",              "esp32s3"),
    ("cardputer",    "M5Stack Cardputer (LCD, Keyboard)",                 "esp32s3"),
    ("rak3312",      "RAK WisBlock RAK3312",                              "esp32s3"),
    ("twatch-ultra", "LilyGo T-Watch Ultra (AMOLED, Touch)",              "esp32s3"),
    ("waveshare-esp32-s3-touch-amoled-2.06", "Waveshare ESP32-S3 Touch AMOLED 2.06", "esp32s3"),
    ("cyd-2432s022", "CYD ESP32-2432S022 (I80 LCD, Touch)",               "esp32"),
    ("cyd-2432s028", "CYD ESP32-2432S028 (LCD, Touch)",                   "esp32"),
    ("c3-mini",      "ESP32-C3 SuperMini (OLED)",                         "esp32c3"),
];

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

const RECOVERY_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>ThistleOS Recovery</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: system-ui, -apple-system, sans-serif; background: #111110; color: #ededed; padding: 16px; max-width: 480px; margin: 0 auto; }
h1 { color: #2563eb; margin-bottom: 6px; font-size: 22px; }
h2 { color: #6b6966; font-size: 13px; margin-bottom: 18px; }
.card { background: #1c1c1b; border: 1px solid #282826; border-radius: 10px; padding: 16px; margin-bottom: 12px; }
.card h3 { color: #ededed; margin-bottom: 12px; font-size: 15px; display: flex; align-items: center; gap: 8px; }
.step-num { display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; background: #2563eb; border-radius: 50%; font-size: 12px; font-weight: 700; flex-shrink: 0; }
.step-num.done { background: #16a34a; }
.step-num.locked { background: #3a3a38; }
label { display: block; color: #a09f9b; font-size: 12px; margin-bottom: 4px; }
input[type=text], input[type=password] { width: 100%; padding: 10px; background: #282826; border: 1px solid #343432; border-radius: 6px; color: #ededed; font-size: 14px; margin-bottom: 10px; }
input:focus { border-color: #2563eb; outline: none; }
.btn { display: block; width: 100%; padding: 11px; background: #2563eb; color: #fff; border: none; border-radius: 6px; font-size: 14px; cursor: pointer; margin-top: 4px; text-align: center; }
.btn:active { background: #1d4ed8; }
.btn.danger { background: #dc2626; }
.btn.danger:active { background: #b91c1c; }
.btn:disabled { background: #3a3a38; color: #6b6966; cursor: not-allowed; }
.status-box { padding: 10px 12px; background: #162316; border: 1px solid #166534; border-radius: 6px; color: #4ade80; font-size: 13px; margin-top: 12px; display: none; }
.status-box.error { background: #2c1515; border-color: #991b1b; color: #f87171; }
.status-box.info  { background: #1a1f2e; border-color: #1e3a5f; color: #93c5fd; }
.info-text { color: #6b6966; font-size: 12px; margin-top: 8px; line-height: 1.5; }
.progress-wrap { width: 100%; height: 8px; background: #282826; border-radius: 4px; overflow: hidden; margin-top: 10px; }
.progress-bar { height: 100%; background: #2563eb; width: 0%; transition: width 0.4s ease; }
.board-option { display: flex; align-items: flex-start; gap: 10px; padding: 10px; border: 1px solid #343432; border-radius: 6px; margin-bottom: 8px; cursor: pointer; }
.board-option:hover { border-color: #2563eb; background: #1e2535; }
.board-option.selected { border-color: #2563eb; background: #1a2540; }
.board-option input[type=radio] { margin-top: 2px; accent-color: #2563eb; flex-shrink: 0; }
.board-label { font-size: 14px; color: #ededed; font-weight: 500; }
.board-desc  { font-size: 12px; color: #6b6966; margin-top: 2px; }
.bundle-items { margin: 10px 0; }
.bundle-item { display: flex; align-items: center; gap: 8px; font-size: 13px; color: #a09f9b; padding: 4px 0; }
.bundle-item::before { content: ''; display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: #2563eb; flex-shrink: 0; }
.hidden { display: none !important; }
.divider { border: none; border-top: 1px solid #282826; margin: 10px 0; }
</style>
</head>
<body>
<h1>ThistleOS Recovery</h1>
<h2 id="subtitle">v0.1.0 — Recovery Mode</h2>

<!-- Step 1: WiFi -->
<div class="card" id="card-wifi">
  <h3><span class="step-num" id="step1-num">1</span> Connect to WiFi</h3>
  <form id="wifi-form" onsubmit="connectWifi(event)">
    <label>Network Name (SSID)</label>
    <input type="text" id="ssid" placeholder="Your WiFi network" required autocomplete="off" autocorrect="off" autocapitalize="off">
    <label>Password</label>
    <input type="password" id="password" placeholder="WiFi password" autocomplete="current-password">
    <button class="btn" type="submit" id="wifi-btn">Connect</button>
  </form>
  <div id="wifi-status" class="status-box"></div>
</div>

<!-- Step 2: Hardware detection + board selection (hidden until WiFi connected) -->
<div class="card hidden" id="card-board">
  <h3><span class="step-num locked" id="step2-num">2</span> Detect Hardware</h3>
  <div id="scan-status" class="status-box info" style="display:block">Scanning hardware...</div>
  <div id="components-list" class="bundle-items hidden"></div>
  <hr class="divider" id="board-divider" style="display:none">
  <p class="info-text" id="board-select-label" style="display:none">Select board for firmware &amp; window manager:</p>
  <div id="board-list" style="margin-top:8px"></div>
  <button class="btn" id="board-refresh-btn" onclick="refreshBoards()" style="margin-top:8px">Download Board List</button>
  <button class="btn" id="board-btn" onclick="selectBoard()" disabled style="margin-top:8px">Confirm Selection</button>
  <div id="board-status" class="status-box"></div>
</div>

<!-- Step 3: Install (hidden until board selected) -->
<div class="card hidden" id="card-install">
  <h3><span class="step-num locked" id="step3-num">3</span> Install ThistleOS</h3>
  <p class="info-text">The following will be downloaded and installed for your board:</p>
  <div class="bundle-items" id="bundle-items">
    <div class="bundle-item">Kernel firmware (ota_1)</div>
    <div class="bundle-item">Board profile (.json)</div>
    <div class="bundle-item">Hardware drivers (.drv.elf)</div>
    <div class="bundle-item">Window manager (.wm.elf)</div>
    <div class="bundle-item">Signatures for verification</div>
  </div>
  <button class="btn" id="install-btn" onclick="startInstall()">Download &amp; Install</button>
  <div class="progress-wrap hidden" id="install-progress">
    <div class="progress-bar" id="install-bar"></div>
  </div>
  <div id="install-status" class="status-box"></div>
</div>

<!-- System card -->
<div class="card">
  <h3>System</h3>
  <button class="btn" onclick="viewStatus()">View Status JSON</button>
  <hr class="divider">
  <button class="btn danger" onclick="confirmReboot()">Reboot Device</button>
</div>

<script>
var wifiConnected = false;
var boardSelected = null;
var pollTimer = null;
var bundlePollTimer = null;

// ---------------------------------------------------------------------------
// Initialise board list and detected components from /api/boards
// ---------------------------------------------------------------------------
function initBoards() {
  fetch('/api/boards')
    .then(function(r) { return r.json(); })
    .then(function(d) {
      var scanStatus   = document.getElementById('scan-status');
      var compList     = document.getElementById('components-list');
      var boardDivider = document.getElementById('board-divider');
      var boardLabel   = document.getElementById('board-select-label');
      var boardList    = document.getElementById('board-list');

      // Show detected hardware components
      var comps = d.components || [];
      if (comps.length > 0) {
        while (compList.firstChild) { compList.removeChild(compList.firstChild); }
        comps.forEach(function(c) {
          var item = document.createElement('div');
          item.className = 'bundle-item';
          item.textContent = c.name + ' (' + c.bus.toUpperCase() + ' 0x' + c.address.toString(16).toUpperCase().padStart(2,'0') + ')';
          compList.appendChild(item);
        });
        scanStatus.textContent = 'Found ' + comps.length + ' hardware component(s)';
        scanStatus.className = 'status-box';
        compList.classList.remove('hidden');
      } else {
        scanStatus.textContent = 'No I2C components detected — select board manually';
        scanStatus.className = 'status-box info';
      }

      // Show board picker (secondary — for firmware/WM selection)
      boardDivider.style.display = '';
      boardLabel.style.display   = '';
      while (boardList.firstChild) { boardList.removeChild(boardList.firstChild); }

      // If board was auto-detected, pre-select it
      if (d.detected) {
        boardSelected = d.detected;
      }

      d.boards.forEach(function(b) {
        var div = document.createElement('div');
        div.className = 'board-option';
        div.dataset.id = b.id;
        div.addEventListener('click', function() { pickBoard(b.id); });

        var radio = document.createElement('input');
        radio.type = 'radio';
        radio.name = 'board';
        radio.value = b.id;
        if (d.detected && b.id === d.detected) {
          radio.checked = true;
          div.className = 'board-option selected';
        }

        var textWrap = document.createElement('div');
        var lbl = document.createElement('div');
        lbl.className = 'board-label';
        lbl.textContent = b.label;
        var desc = document.createElement('div');
        desc.className = 'board-desc';
        desc.textContent = b.id;
        textWrap.appendChild(lbl);
        textWrap.appendChild(desc);
        div.appendChild(radio);
        div.appendChild(textWrap);
        boardList.appendChild(div);
      });

      // Enable confirm button if board is pre-selected
      if (d.detected) {
        document.getElementById('board-btn').disabled = false;
        fetch('/api/board/select', {
          method: 'POST',
          headers: {'Content-Type':'application/json'},
          body: JSON.stringify({board: d.detected})
        });
      }
    })
    .catch(function() {
      var scanStatus = document.getElementById('scan-status');
      scanStatus.textContent = 'Hardware scan unavailable — select board manually';
      scanStatus.className = 'status-box info';
    });

  // Check if already connected (page reload case)
  checkInitialStatus();
}

initBoards();

function refreshBoards() {
  var st = document.getElementById('board-status');
  showStatus(st, 'Downloading board list...', 'info');
  initBoards();
}

function checkInitialStatus() {
  fetch('/api/status')
    .then(function(r) { return r.json(); })
    .then(function(d) {
      if (d.chip) {
        document.getElementById('subtitle').textContent =
          'v0.1.0 — ' + d.chip.toUpperCase() + ' — Recovery Mode';
      }
      if (d.wifi_connected) {
        setWifiDone(d.wifi_ip || '');
      }
      if (d.board_name) {
        boardSelected = d.board_name;
        var opt = document.querySelector('.board-option[data-id="' + d.board_name + '"]');
        if (opt) {
          opt.classList.add('selected');
          var radio = opt.querySelector('input[type=radio]');
          if (radio) { radio.checked = true; }
        }
        setBoardDone(d.board_name);
      }
      if (d.bundle_status === 'downloading') {
        showCard('card-install');
        startBundlePoll();
      } else if (d.bundle_status === 'complete') {
        showCard('card-install');
        setInstallDone(d.bundle_items || 0);
      }
    })
    .catch(function() {});
}

// ---------------------------------------------------------------------------
// Step 1: WiFi
// ---------------------------------------------------------------------------
function connectWifi(e) {
  e.preventDefault();
  var ssid = document.getElementById('ssid').value.trim();
  var pass = document.getElementById('password').value;
  if (!ssid) { return; }

  var st  = document.getElementById('wifi-status');
  var btn = document.getElementById('wifi-btn');
  showStatus(st, 'Connecting...', 'info');
  btn.disabled = true;

  fetch('/api/wifi/connect', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({ssid: ssid, password: pass})
  })
  .then(function(r) { return r.json(); })
  .then(function(d) {
    if (d.ok) {
      showStatus(st, 'Connecting to network...', 'info');
      pollWifiStatus();
    } else {
      showStatus(st, 'Error: ' + (d.error || 'unknown'), 'error');
      btn.disabled = false;
    }
  })
  .catch(function(err) {
    showStatus(st, 'Error: ' + err.message, 'error');
    btn.disabled = false;
  });
}

function pollWifiStatus() {
  if (pollTimer) { clearInterval(pollTimer); }
  var attempts = 0;
  pollTimer = setInterval(function() {
    attempts++;
    fetch('/api/status')
      .then(function(r) { return r.json(); })
      .then(function(d) {
        if (d.wifi_connected) {
          clearInterval(pollTimer);
          setWifiDone(d.wifi_ip || '');
        } else if (attempts >= 30) {
          clearInterval(pollTimer);
          var st = document.getElementById('wifi-status');
          showStatus(st, 'Connection timed out. Check SSID and password.', 'error');
          document.getElementById('wifi-btn').disabled = false;
        }
      })
      .catch(function() {});
  }, 1000);
}

function setWifiDone(ip) {
  wifiConnected = true;
  var st  = document.getElementById('wifi-status');
  var msg = 'Connected' + (ip ? ' — IP: ' + ip : '');
  showStatus(st, msg, '');
  document.getElementById('step1-num').classList.add('done');
  document.getElementById('wifi-btn').disabled = true;
  var fields = document.getElementById('wifi-form').querySelectorAll('input');
  for (var i = 0; i < fields.length; i++) { fields[i].disabled = true; }
  showCard('card-board');
  document.getElementById('step2-num').classList.remove('locked');
  refreshBoards();
}

// ---------------------------------------------------------------------------
// Step 2: Board selection
// ---------------------------------------------------------------------------
function pickBoard(id) {
  boardSelected = id;
  var opts = document.querySelectorAll('.board-option');
  for (var i = 0; i < opts.length; i++) {
    var opt = opts[i];
    var match = opt.dataset.id === id;
    if (match) {
      opt.classList.add('selected');
    } else {
      opt.classList.remove('selected');
    }
    var radio = opt.querySelector('input[type=radio]');
    if (radio) { radio.checked = match; }
  }
  document.getElementById('board-btn').disabled = false;
}

function selectBoard() {
  if (!boardSelected) { return; }
  var st  = document.getElementById('board-status');
  var btn = document.getElementById('board-btn');
  btn.disabled = true;
  showStatus(st, 'Saving...', 'info');

  fetch('/api/board/select', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({board: boardSelected})
  })
  .then(function(r) { return r.json(); })
  .then(function(d) {
    if (d.ok) {
      showStatus(st, 'Board set: ' + boardSelected, '');
      setBoardDone(boardSelected);
    } else {
      showStatus(st, 'Error: ' + (d.error || 'unknown'), 'error');
      btn.disabled = false;
    }
  })
  .catch(function(err) {
    showStatus(st, 'Error: ' + err.message, 'error');
    btn.disabled = false;
  });
}

function setBoardDone(name) {
  document.getElementById('step2-num').classList.add('done');
  document.getElementById('board-btn').disabled = true;
  showCard('card-install');
  document.getElementById('step3-num').classList.remove('locked');
}

// ---------------------------------------------------------------------------
// Step 3: Install
// ---------------------------------------------------------------------------
function startInstall() {
  var st   = document.getElementById('install-status');
  var btn  = document.getElementById('install-btn');
  var prog = document.getElementById('install-progress');
  var bar  = document.getElementById('install-bar');

  btn.disabled = true;
  prog.classList.remove('hidden');
  bar.style.width = '5%';
  showStatus(st, 'Starting download...', 'info');

  fetch('/api/bundle/download', {method: 'POST'})
    .then(function(r) { return r.json(); })
    .then(function(d) {
      if (d.ok) {
        startBundlePoll();
      } else {
        showStatus(st, 'Error: ' + (d.error || 'unknown'), 'error');
        btn.disabled = false;
      }
    })
    .catch(function(err) {
      showStatus(st, 'Error: ' + err.message, 'error');
      btn.disabled = false;
    });
}

function startBundlePoll() {
  if (bundlePollTimer) { clearInterval(bundlePollTimer); }
  var bar  = document.getElementById('install-bar');
  var prog = document.getElementById('install-progress');
  var st   = document.getElementById('install-status');

  prog.classList.remove('hidden');

  bundlePollTimer = setInterval(function() {
    fetch('/api/bundle/status')
      .then(function(r) { return r.json(); })
      .then(function(d) {
        bar.style.width = (d.progress || 0) + '%';
        if (d.status === 'complete') {
          clearInterval(bundlePollTimer);
          setInstallDone(d.items || 0);
        } else if (d.status && d.status.indexOf('error') === 0) {
          clearInterval(bundlePollTimer);
          showStatus(st, d.status, 'error');
          document.getElementById('install-btn').disabled = false;
        } else {
          showStatus(st, 'Downloading... ' + (d.progress || 0) + '%', 'info');
        }
      })
      .catch(function() {});
  }, 1000);
}

function setInstallDone(items) {
  var bar   = document.getElementById('install-bar');
  var st    = document.getElementById('install-status');
  var step3 = document.getElementById('step3-num');
  bar.style.width = '100%';
  step3.classList.add('done');

  var countdown = 5;
  function tick() {
    var suffix = countdown === 1 ? ' second' : ' seconds';
    showStatus(st, 'Installed ' + items + ' item(s). Rebooting in ' + countdown + suffix + '...', '');
    if (countdown <= 0) {
      showStatus(st, 'Rebooting now...', '');
      fetch('/api/reboot', {method: 'POST'}).catch(function() {});
      return;
    }
    countdown--;
    setTimeout(tick, 1000);
  }
  tick();
}

// ---------------------------------------------------------------------------
// System card
// ---------------------------------------------------------------------------
function viewStatus() {
  window.open('/api/status', '_blank');
}

function confirmReboot() {
  if (confirm('Reboot the device now?')) {
    fetch('/api/reboot', {method: 'POST'}).catch(function() {});
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
function showCard(id) {
  document.getElementById(id).classList.remove('hidden');
}

function showStatus(el, msg, type) {
  el.style.display = 'block';
  el.className = 'status-box' + (type ? ' ' + type : '');
  el.textContent = msg;
}
</script>
</body>
</html>"#;

// ---------------------------------------------------------------------------
// Handler registration
// ---------------------------------------------------------------------------

pub fn register_handlers(server: &mut EspHttpServer) -> anyhow::Result<()> {
    // Serve the main recovery page
    server.fn_handler("/", esp_idf_svc::http::Method::Get, |req| -> anyhow::Result<()> {
        let mut resp = req.into_response(200, None, &[("Content-Type", "text/html")])?;
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

    // GET /api/status — overall recovery state
    server.fn_handler("/api/status", esp_idf_svc::http::Method::Get, |req| -> anyhow::Result<()> {
        let (connected, ip, board, bundle_status, bundle_progress, chip) = {
            let st = STATE.lock().unwrap();
            (
                st.wifi_connected,
                st.wifi_ip.clone(),
                st.board_name.clone(),
                st.bundle_status.clone(),
                st.bundle_progress,
                st.chip.clone(),
            )
        };

        let ota1_str   = format!("{:?}", crate::recovery_ota::check_ota1());
        let sd_fw      = crate::recovery_ota::check_sd_firmware();
        let board_json = match &board {
            Some(b) => format!("\"{}\"", b),
            None    => "null".to_string(),
        };
        let status_eff = if bundle_status.is_empty() { "idle" } else { &bundle_status };

        // When complete, report the atomic progress counter as an item count
        let live_progress = if bundle_status == "downloading" || bundle_status == "complete" {
            crate::recovery_ota::BUNDLE_PROGRESS.load(std::sync::atomic::Ordering::Relaxed)
        } else {
            bundle_progress
        };

        let bundle_items = if bundle_status == "complete" { live_progress as u32 } else { 0 };

        let chip_str = if chip.is_empty() {
            crate::recovery_ota::detect_chip().to_string()
        } else {
            chip
        };

        let json = format!(
            concat!(
                r#"{{"version":"{}","mode":"recovery","chip":"{}","wifi_connected":{},"wifi_ip":"{}","#,
                r#""board_name":{},"bundle_status":"{}","bundle_progress":{},"bundle_items":{},"#,
                r#""ota1":"{}","sd_firmware":{}}}"#,
            ),
            super::VERSION,
            chip_str,
            connected,
            ip,
            board_json,
            status_eff,
            live_progress,
            bundle_items,
            ota1_str,
            sd_fw,
        );

        let mut resp = req.into_response(200, None, &[("Content-Type", "application/json")])?;
        resp.write(json.as_bytes())?;
        Ok(())
    })?;

    // GET /api/boards — list of known boards filtered by chip + detected hardware components
    server.fn_handler("/api/boards", esp_idf_svc::http::Method::Get, |req| -> anyhow::Result<()> {
        let (detected_board, components, chip, wifi_connected, catalog_url) = {
            let st = STATE.lock().unwrap();
            (
                st.board_name.clone(),
                st.detected_components.clone(),
                st.chip.clone(),
                st.wifi_connected,
                st.catalog_url.clone(),
            )
        };

        // Fall back to runtime detection if chip wasn't stored at boot yet.
        let chip_slug = if chip.is_empty() {
            crate::recovery_ota::detect_chip().to_string()
        } else {
            chip
        };

        let mut fallback_board_parts: Vec<String> = Vec::new();
        for (id, label, arch) in KNOWN_BOARDS {
            if arch.is_empty() || *arch == chip_slug.as_str() {
                fallback_board_parts.push(format!(r#"{{"id":"{}","label":"{}","arch":"{}"}}"#, id, label, arch));
            }
        }
        let fallback_boards = fallback_board_parts.join(",");

        // Once STA WiFi is connected, prefer the catalog board list. Recovery
        // keeps the built-in list as an offline fallback.
        let (boards_json, source) = if wifi_connected && !catalog_url.is_empty() {
            match crate::recovery_ota::catalog_board_options_json(&catalog_url, &chip_slug) {
                Ok(downloaded) if !downloaded.is_empty() => (downloaded, "catalog"),
                _ => (fallback_boards, "builtin"),
            }
        } else {
            (fallback_boards, "builtin")
        };

        let detected_json = match &detected_board {
            Some(d) => format!(r#""{}""#, d),
            None => "null".to_string(),
        };

        // Detected hardware components for driver auto-matching
        let mut comp_parts: Vec<String> = Vec::new();
        for (bus, addr, name) in &components {
            comp_parts.push(format!(
                r#"{{"bus":"{}","address":{},"name":"{}"}}"#,
                bus, addr, name
            ));
        }

        let json = format!(
            r#"{{"boards":[{}],"source":"{}","detected":{},"components":[{}]}}"#,
            boards_json,
            source,
            detected_json,
            comp_parts.join(","),
        );

        let mut resp = req.into_response(200, None, &[("Content-Type", "application/json")])?;
        resp.write(json.as_bytes())?;
        Ok(())
    })?;

    // POST /api/wifi/connect — store credentials; main.rs picks them up
    server.fn_handler("/api/wifi/connect", esp_idf_svc::http::Method::Post, |mut req| -> anyhow::Result<()> {
        let mut body = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            match req.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.extend_from_slice(&buf[..n]),
            }
        }

        let body_str = String::from_utf8_lossy(&body);
        let ssid = crate::recovery_ota::json_extract_string(&body_str, "ssid");
        let pass = crate::recovery_ota::json_extract_string(&body_str, "password");

        match (ssid, pass) {
            (Some(s), Some(p)) if !s.is_empty() => {
                {
                    let mut st = STATE.lock().unwrap();
                    st.wifi_request = Some((s, p));
                    st.wifi_connected = false;
                    st.wifi_ip.clear();
                }
                let mut resp = req.into_response(200, None, &[("Content-Type", "application/json")])?;
                resp.write(b"{\"ok\":true}")?;
            }
            _ => {
                let mut resp = req.into_response(400, None, &[("Content-Type", "application/json")])?;
                resp.write(b"{\"ok\":false,\"error\":\"missing ssid or password\"}")?;
            }
        }
        Ok(())
    })?;

    // POST /api/board/select — set the board name
    server.fn_handler("/api/board/select", esp_idf_svc::http::Method::Post, |mut req| -> anyhow::Result<()> {
        let mut body = Vec::new();
        let mut buf = [0u8; 128];
        loop {
            match req.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.extend_from_slice(&buf[..n]),
            }
        }

        let body_str = String::from_utf8_lossy(&body);
        let board = crate::recovery_ota::json_extract_string(&body_str, "board");

        let (chip, wifi_connected, catalog_url) = {
            let st = STATE.lock().unwrap();
            (st.chip.clone(), st.wifi_connected, st.catalog_url.clone())
        };
        let chip_slug = if chip.is_empty() {
            crate::recovery_ota::detect_chip().to_string()
        } else {
            chip
        };

        match board {
            Some(b) if board_is_selectable(&b, &chip_slug, wifi_connected, &catalog_url) => {
                {
                    let mut st = STATE.lock().unwrap();
                    st.board_name = Some(b);
                }
                let mut resp = req.into_response(200, None, &[("Content-Type", "application/json")])?;
                resp.write(b"{\"ok\":true}")?;
            }
            Some(_) => {
                let mut resp = req.into_response(400, None, &[("Content-Type", "application/json")])?;
                resp.write(b"{\"ok\":false,\"error\":\"unknown board\"}")?;
            }
            None => {
                let mut resp = req.into_response(400, None, &[("Content-Type", "application/json")])?;
                resp.write(b"{\"ok\":false,\"error\":\"missing board field\"}")?;
            }
        }
        Ok(())
    })?;

    // POST /api/bundle/download — signal main.rs to start the bundle download
    server.fn_handler("/api/bundle/download", esp_idf_svc::http::Method::Post, |req| -> anyhow::Result<()> {
        let (has_board, already_downloading) = {
            let st = STATE.lock().unwrap();
            (st.board_name.is_some(), st.bundle_status == "downloading")
        };

        if !has_board {
            let mut resp = req.into_response(400, None, &[("Content-Type", "application/json")])?;
            resp.write(b"{\"ok\":false,\"error\":\"no board selected\"}")?;
            return Ok(());
        }
        if already_downloading {
            let mut resp = req.into_response(409, None, &[("Content-Type", "application/json")])?;
            resp.write(b"{\"ok\":false,\"error\":\"download already in progress\"}")?;
            return Ok(());
        }

        {
            let mut st = STATE.lock().unwrap();
            st.bundle_request = true;
            st.bundle_status = "downloading".to_string();
            st.bundle_progress = 0;
        }
        crate::recovery_ota::BUNDLE_PROGRESS.store(0, std::sync::atomic::Ordering::Relaxed);

        let mut resp = req.into_response(200, None, &[("Content-Type", "application/json")])?;
        resp.write(b"{\"ok\":true,\"status\":\"downloading\"}")?;
        Ok(())
    })?;

    // GET /api/bundle/status — poll download progress
    server.fn_handler("/api/bundle/status", esp_idf_svc::http::Method::Get, |req| -> anyhow::Result<()> {
        let (status, _stored_progress) = {
            let st = STATE.lock().unwrap();
            (st.bundle_status.clone(), st.bundle_progress)
        };

        let status_str = if status.is_empty() { "idle".to_string() } else { status };

        // Always read fresh progress from the atomic (updated by download loop)
        let live = crate::recovery_ota::BUNDLE_PROGRESS.load(std::sync::atomic::Ordering::Relaxed);

        let items_field = if status_str == "complete" {
            format!(",\"items\":{}", live)
        } else {
            String::new()
        };

        let json = format!(
            r#"{{"status":"{}","progress":{}{}}}"#,
            status_str, live, items_field
        );

        let mut resp = req.into_response(200, None, &[("Content-Type", "application/json")])?;
        resp.write(json.as_bytes())?;
        Ok(())
    })?;

    // POST /api/reboot
    server.fn_handler("/api/reboot", esp_idf_svc::http::Method::Post, |req| -> anyhow::Result<()> {
        let mut resp = req.into_response(200, None, &[("Content-Type", "application/json")])?;
        resp.write(b"{\"ok\":true}")?;
        info!("Reboot requested via web UI");
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(1));
            unsafe { esp_idf_sys::esp_restart() };
        });
        Ok(())
    })?;

    info!("Web UI handlers registered (captive portal active)");
    Ok(())
}

fn board_is_selectable(board_id: &str, chip: &str, wifi_connected: bool, catalog_url: &str) -> bool {
    if KNOWN_BOARDS
        .iter()
        .any(|(id, _, arch)| *id == board_id && (arch.is_empty() || *arch == chip))
    {
        return true;
    }

    wifi_connected
        && !catalog_url.is_empty()
        && crate::recovery_ota::catalog_contains_board(catalog_url, chip, board_id)
}
