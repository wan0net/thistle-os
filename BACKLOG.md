# ThistleOS Backlog

## Iteration 1 — 2026-03-26

### Priority 1: GPS Track Logger & GPX Export Module
**Status:** DONE (unmerged, branch: feat/gps-track-gpx)
**Personas:** Cairn (SAR coordinate sharing), Ember (field transect logging)
**Description:** Add a `gps_track` kernel module that records GPS positions over time and exports them as GPX XML. Enables track recording, waypoint management, and GPX file generation for SD card export. Pure Rust, no hardware dependency beyond existing GPS HAL.

### Priority 2: Complete Power Driver (TP4065B) — Real ADC Battery Readings
**Status:** PENDING (local changes in progress on main)
**Personas:** ALL (battery monitoring is universal)
**Description:** Replace stub/TODO ADC readings in drv_power_tp4065b.rs with real ESP-IDF ADC calls. Provide accurate battery percentage, voltage, and charging status.

### Priority 3: Messenger Internet Transport (WiFi/WebSocket)
**Status:** PENDING (local changes in progress on main)
**Personas:** Cairn, Thorn, Ember
**Description:** Implement the WiFi/WebSocket transport path in messenger_transport.c. Currently stubbed with TODO, returns ESP_ERR_NOT_SUPPORTED.

### Priority 4: BLE Scanner App
**Status:** DONE (unmerged, branch: feat/ble-scanner)
**Personas:** Spark (primary), Thorn (security awareness)
**Description:** New app that discovers and displays nearby BLE devices, their services, RSSI, and manufacturer data. Builds on the pure Rust BLE manager.

### Priority 5: SOS Beacon Mode
**Status:** DONE (unmerged, branch: feat/sos-beacon)
**Personas:** Cairn (emergency), Ember (safety)
**Description:** Emergency mode that broadcasts GPS position over LoRa at regular intervals with SOS flag. Minimal UI, maximum battery conservation.

### Priority 6: Device Wipe / Panic Button
**Status:** DONE (unmerged, branch: feat/secure-wipe)
**Personas:** Thorn (journalist safety)
**Description:** Secure wipe of SD card contents and SPIFFS user data on long-press of a configurable key combination.

### Priority 7: Structured Data Logger
**Status:** DONE (unmerged, branch: feat/data-logger)
**Personas:** Ember (field research), Fern (sensor monitoring)
**Description:** Generic CSV/JSON data logging module that apps can use to record timestamped sensor readings to SD card.

### Priority 8: Driver Hot-Reload
**Status:** DONE (unmerged, branch: feat/driver-hot-reload)
**Personas:** Fern (development workflow)
**Description:** Ability to unload and reload .drv.elf files without rebooting. Requires driver lifecycle management in driver_manager.

## Iteration 4 — 2026-03-26 (new items from persona review)

### Priority 9: MeshCore Integration
**Status:** DONE (on main — mesh_manager, tk_meshchat, real Ed25519/X25519 crypto, HAL radio integration)
**Personas:** Cairn (multi-hop SAR), Ember (field station mesh)
**Description:** Integrate MeshCore mesh protocol for LoRa networking. Custom ThistleOS UI on top of MeshCore protocol. Do NOT reinvent mesh routing — use existing MeshCore.

### Priority 10: Notification Manager
**Status:** DONE (unmerged, branch: feat/notification-manager)
**Personas:** ALL
**Description:** System-wide notification queue. Apps post notifications, WM displays them. Priority levels, expiry, dismissal. Cross-app coordination.

### Priority 11: Contact Manager
**Status:** DONE (unmerged, branch: feat/contact-manager)
**Personas:** Cairn (team roster), Thorn (source contacts), Ember (collaborators)
**Description:** Address book for messenger. Name, callsign, device ID, public key. Import/export vCard. Integrates with messenger and SOS beacon.

## Iteration 7 — 2026-03-27 (new items from persona review)

### Priority 12: Message Burn Timer
**Status:** DONE (unmerged, branch: feat/burn-timer)
**Personas:** Thorn (source protection), Cairn (sensitive casualty info)
**Description:** Auto-delete messages after a configurable time interval. Per-conversation setting. Timer starts at message receive/send time. Countdown visible in UI. Integrates with messenger.

### Priority 14: End-to-End Message Encryption
**Status:** DONE (unmerged, branch: feat/msg-crypto)
**Personas:** Thorn (source protection — critical), Cairn (sensitive casualty info)
**Description:** Encrypt/decrypt messages using per-contact shared secrets. PBKDF2 key derivation from passphrase, AES-256-CTR encryption, HMAC-SHA256 authentication. Integrates with contact manager (stores per-contact key material) and messenger transport.

### Priority 15: Waypoint Manager & Navigation
**Status:** PENDING
**Personas:** Cairn (grid ref sharing, SAR convergence), Ember (boat transect routes, field station locations)
**Description:** Manage named GPS waypoints with categories. Calculate distance and bearing between current position and any waypoint (Haversine formula). Import/export waypoints. Persistent storage on SD card. Enables the "Navigator" use case described by both Cairn and Ember.

### Priority 16: File Manager
**Status:** PENDING
**Personas:** Ember (SD card data management), ALL (universal utility)
**Description:** Browse SD card contents, view file sizes/dates, copy/delete files, create directories. Kernel module for file operations with FFI for UI apps.

### Priority 17: Serial Terminal (GhostTerm)
**Status:** PENDING
**Personas:** Spark (network device console), Fern (hardware debugging)
**Description:** UART serial terminal for connecting to external devices via GPIO pins. Configurable baud rate, data bits, parity, stop bits. Buffer incoming data, send typed commands.

### Priority 13: LoRa Store-and-Forward Message Queue
**Status:** DONE (unmerged, branch: feat/lora-msg-queue)
**Personas:** Cairn (multi-hop SAR), Ember (field station mesh)
**Description:** Queue outbound LoRa messages when no recipients in range. Auto-send when relay node comes in range. Persistent queue on SD card survives reboots. Retry with exponential backoff.

## Post-1.0 — 2026-03-28

### Priority 18: Voice Calls via A7682E Modem
**Status:** PENDING
**Personas:** Cairn (SAR coordination), Thorn (secure comms)
**Description:** Add voice call support using A7682E AT commands (ATD, ATA, ATH, +RING URC). Requires I2S audio bridge between modem PCM output and PCM5102A DAC, plus microphone input from T-Deck Pro keyboard PCB. Needs call UI: dialer, incoming call screen, in-call controls with mute/speaker/hangup.

### Priority 19: Mesh-to-Internet Gateway Mode
**Status:** PENDING
**Personas:** Cairn (SAR base camp), Ember (field station), ALL
**Description:** A ThistleOS node with internet (WiFi or LTE) acts as a transparent mesh-to-internet gateway. Field devices on LoRa mesh send messages that hop to the gateway, which proxies them out via SMS, HTTP webhook, or any configured endpoint. Replies are relayed back into the mesh. Enables the "base station" pattern: one powered node at home/camp gives the entire mesh internet access. Implementation: MeshCore `onMessageRecv` checks if message is marked for internet relay → forwards via modem SMS or esp_http_client POST → relays response back as mesh message. Configuration via system.json (relay phone number, webhook URL, auth token). MeshCore already supports MQTT — the gateway should use MQTT as the primary internet transport (publish mesh messages to `thistle/mesh/{node_id}/msg`, subscribe to `thistle/mesh/{node_id}/inbox` for replies). Works with self-hosted Mosquitto or cloud brokers. ESP-IDF provides `esp_mqtt_client` natively over WiFi or PPP/LTE.

### Priority 20: Mesh Base Station (Dedicated Gateway Hardware)
**Status:** PENDING
**Personas:** Cairn (SAR coordination), Ember (field station mesh)
**Description:** Headless base station mode for cheap hardware (Heltec V3, RAK, C3-Mini). No display needed — boots into MeshCore gateway with WiFi/LTE backhaul. Web UI for configuration (relay targets, mesh settings, status monitoring). Could run on a Raspberry Pi with LoRa hat as an alternative. Auto-starts mesh + PPP on boot, persists message queue across power cycles.

### Priority 21: Messenger Internet Transport (LTE/WiFi)
**Status:** PENDING
**Personas:** Cairn, Thorn, Ember
**Description:** Wire messenger Internet transport to use PPP (4G) or WiFi for message delivery. The modem driver's PPP stack is already implemented — once connected, esp_http_client routes over LTE transparently. Needs: backend API endpoint definition (REST or WebSocket), message relay server, auth token storage in NVS. Could use a simple self-hosted relay or a public service.
