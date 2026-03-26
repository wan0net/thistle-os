# Ralph Loop Log -- Persona-Driven Feature Development

## Iteration 1

### Unimplemented Features Reviewed

| Category | Gap | Severity |
|----------|-----|----------|
| Messenger | SMS transport stub | High |
| Messenger | BLE relay transport stub | High |
| Messenger | Internet transport stub | Medium |
| Driver | BHI260AP IMU stub | Medium |
| Driver | LTR-553 Rust mirror stub | Low |
| Kernel | Runtime WM loading | Low |
| Kernel | Theme JSON loading | Low |
| App | MeshCore integration stub | Medium |

### Persona Reviews

**Cairn (Ewan -- SAR volunteer):**
> "LoRa works, which is my primary need. But I'd really benefit from:
> 1. **GPS waypoint sharing over LoRa** -- I can see my position in Navigator but can't send a waypoint as a structured message that another device renders on a map.
> 2. **Emergency beacon mode** -- a one-button SOS that broadcasts my GPS coords on repeat over LoRa with a distinctive alert tone on receiving devices.
> 3. **Offline maps** -- even low-res tile cache on SD card would be massive for navigation."

**Thorn (Amara -- journalist):**
> "Encrypted messaging is promised but I need to verify:
> 1. **SMS transport for Messenger** -- when I have cell coverage, I need to reach normal phones. The modem driver has SMS now but it's not wired into the Messenger app.
> 2. **Message burn timer** -- auto-delete messages after a configurable time. Essential for source protection.
> 3. **Panic wipe** -- a key combo that securely erases the vault and conversation history."

**Fern (Yuki -- hardware maker):**
> "The driver SDK and .drv.elf system works but:
> 1. **IMU driver (BHI260AP)** -- I need this working so my motion-sensing products have a reference implementation.
> 2. **Driver hot-reload** -- ability to reload a .drv.elf without full reboot during development.
> 3. **Sensor data IPC** -- a standard IPC message format for sensor readings so any app can subscribe to any sensor."

**Ember (Rafael -- field researcher):**
> "For my fieldwork I need:
> 1. **Data logger app** -- timestamped sensor readings (GPS, IMU, light, temperature) saved to CSV on SD card. I shouldn't need to write an app for basic data collection.
> 2. **LoRa message queuing** -- when a relay node comes in range, queued messages should auto-send. Currently if nobody's listening, the message is lost.
> 3. **Battery percentage display** -- I need to know how much charge I have left at a glance from the status bar."

**Spark (Dani -- pentester):**
> "For security work:
> 1. **BLE scanner app** -- enumerate nearby BLE devices, show services/characteristics, signal strength. Basic recon tool.
> 2. **WiFi Scanner improvements** -- channel hopping, deauth detection, probe request logging.
> 3. **USB serial passthrough** -- use the device as a USB-to-serial adapter when connected to a laptop."

### Prioritised Build List (Iteration 1)

Combining persona needs with implementation feasibility:

| Priority | Feature | Requested By | Effort |
|----------|---------|-------------|--------|
| 1 | SMS transport in Messenger | Thorn | Small -- modem driver ready |
| 2 | Battery % in status bar | Ember | Small -- power driver exists |
| 3 | GPS waypoint sharing via LoRa | Cairn | Medium |
| 4 | Emergency beacon mode | Cairn | Medium |
| 5 | Data logger app | Ember | Medium |
| 6 | BLE scanner app | Spark | Medium |
| 7 | Message burn timer | Thorn | Small |

### Build Results (Iteration 1)

| # | Feature | Status | Notes |
|---|---------|--------|-------|
| 1 | SMS transport in Messenger | DONE | Wired `drv_a7682e_send_sms/read_sms` into messenger_transport.c |
| 2 | BLE relay transport | DONE | Added `MSG:<sender>\n<text>` framing protocol over NUS |
| 3 | Battery % in status bar | ALREADY EXISTS | `statusbar_update_tick()` reads power HAL every 30s |

### Open Questions (Iteration 1)
- BLE companion app protocol: currently using simple `MSG:` framing. A real companion app would need a spec document.
- SMS URC handler: `drv_a7682e_register_sms_cb()` stores the callback but URC dispatch from esp_modem is not wired yet (noted as TODO in modem driver). SMS receive will work once esp_modem exposes URC registration.
- Internet transport: deferred. Requires a relay server design (WebSocket endpoint, auth, message routing). Out of scope for device-only development.

---

## Iteration 2

### Persona Re-Review (post Iteration 1)

**Cairn (Ewan):**
> "SMS and BLE transports are great for the others but I still can't share my GPS position. This is my #1 remaining need. Also -- when a team member sends an SOS, every device in range should alert immediately."

**Thorn (Amara):**
> "SMS transport works now -- excellent. Next I want the **message burn timer** so conversations auto-delete. And a **panic wipe** hotkey."

**Fern (Yuki):**
> "The BHI260AP IMU driver is still a stub. I need it for my products. Also a **sensor data IPC format** so apps can subscribe to sensor events."

**Ember (Rafael):**
> "Battery is already visible, great. I really need the **data logger app** for my field surveys. Timestamp, GPS coords, sensor readings, all to CSV."

**Spark (Dani):**
> "The **BLE scanner app** is still missing. This is basic recon I need for every engagement."

### Prioritised Build List (Iteration 2)

| Priority | Feature | Requested By | Effort | Notes |
|----------|---------|-------------|--------|-------|
| 1 | GPS waypoint sharing via LoRa | Cairn | Medium | New message type in messenger |
| 2 | Emergency SOS beacon | Cairn | Medium | Repeating GPS broadcast + alert |
| 3 | Message burn timer | Thorn | Small | Auto-delete after N minutes |
| 4 | Data logger app | Ember | Medium | New app, CSV to SD |
| 5 | BLE scanner app | Spark | Medium | New app, NimBLE scan |

### Building (Iteration 2)

Building items 1-3 (GPS sharing, SOS beacon, burn timer).

---

## Iteration 6

### Feature Selected

**Contact Manager** (Backlog Priority 11)
- **Personas:** Cairn (team roster), Thorn (source contacts), Ember (collaborators)
- **Description:** Address book for messenger. Name, callsign, device ID, public key. Import/export vCard. Integrates with messenger and SOS beacon.

### Build Results (Iteration 6)

| # | Feature | Status | Notes |
|---|---------|--------|-------|
| 1 | Contact Manager kernel module | DONE | `contact_manager.rs` — 65 tests, all 653 suite tests pass |

**What was built:**
- Pure Rust kernel module at `components/kernel_rs/src/contact_manager.rs` (~1050 lines)
- Data model: Contact with name, callsign, device_id (LoRa), phone (SMS), ble_addr, Ed25519 public key, notes, emergency flag, timestamps
- `CContactInfo` repr(C) FFI struct for C interop with fixed-size byte arrays
- 16 FFI exports (`rs_contact_*`): init, add, remove, update, get, count, get_at, find_by_device_id, find_by_phone, search, get_emergency, set_pubkey, save, export_vcard, import_vcard
- JSON persistence to `/sdcard/data/contacts.json` (manual serialization, no serde)
- Minimal base64 encode/decode for public key serialization
- vCard 3.0 export/import with FN, NICKNAME, TEL, NOTE, KEY fields
- 65 unit tests covering all operations, edge cases, JSON round-trip, vCard round-trip

### Persona Review (Iteration 6)

**Cairn (Ewan):**
> "This is exactly what I needed. I can store my whole team roster with callsigns and LoRa device IDs. The `find_by_device_id` function means the messenger can now show 'CAIRN-1' instead of 'Node-A63B'. Emergency contacts flag is perfect for SOS beacon integration."

**Thorn (Amara):**
> "Public key storage per contact is critical for my work. I can verify who I'm communicating with. The vCard import/export means I can prepare contacts on my laptop and transfer via SD card. Notes field lets me add context about sources without it being structured data."

**Ember (Rafael):**
> "Having a shared contact list across messenger and SOS is good. When my field assistant triggers an SOS, I want to know who it is immediately from their device ID rather than a hex number."

**Fern (Yuki):**
> "Clean module design. The FFI surface is well-defined — I could build a contacts management app as a `.app.elf` using just these syscalls."

**Spark (Dani):**
> "BLE address storage is useful for my scanning work. I can correlate discovered devices with known contacts."

### Open Questions (Iteration 6)

- **Q1: Messenger UI integration.** The kernel module provides `rs_contact_find_by_device_id()` and `rs_contact_find_by_phone()` for sender resolution, but the messenger UI (`messenger_ui.c`) needs to be updated to call these. Currently it displays raw sender strings like "Node-XXXX". This is a UI-level change, not a kernel change.
- **Q2: Contact picker widget.** Messenger's "new message" flow should offer a contact picker. This requires a new UI component — probably a scrollable list with search. Should this be a shared UI widget in thistle-tk, or messenger-specific?
- **Q3: SOS beacon wiring.** The `rs_contact_get_emergency()` FFI is ready, but the SOS beacon module (on `feat/sos-beacon` branch) needs to call it during broadcast to include the sender's identity and notify emergency contacts. These are on separate branches — needs coordination at merge time.
- **Q4: Contact sync.** No mechanism to sync contacts between devices. For Cairn's SAR team, everyone needs the same roster. Could use LoRa broadcast of vCards, but that's bandwidth-heavy. Deferred.
- **Q5: Event bus integration.** Should we add a `ContactsChanged` event type to the event bus so messenger/SOS/other apps auto-refresh when contacts are modified? Currently the event bus has 17 types with no spare slots — would need to increase `EVENT_MAX`.

---

## Iteration 7

### Review Phase

Persona re-review post iteration 6. All 5 personas reviewed project state.

**Cairn:** GPS track ✓, SOS ✓, Contacts ✓. Still wants MeshCore (P9) and LoRa store-and-forward.
**Thorn:** Secure wipe ✓, Contacts ✓. Wants message burn timer and E2E encryption.
**Fern:** Data logger ✓. Wants driver hot-reload (P8) and sensor IPC format.
**Ember:** GPS ✓, Data logger ✓, SOS ✓, Contacts ✓. Wants LoRa queue and internet transport.
**Spark:** No features delivered yet. BLE Scanner (P4) is #1 priority.

New backlog items added: Message burn timer (P12), LoRa store-and-forward (P13).

**Selected:** Priority 4 — BLE Scanner App. Highest impact for most underserved persona (Spark has zero features delivered).

### Build Results (Iteration 7)

| # | Feature | Status | Notes |
|---|---------|--------|-------|
| 1 | BLE Scanner kernel module | DONE | `ble_scanner.rs` — 45 tests, 633/633 suite pass |

**What was built:**
- Pure Rust kernel module at `components/kernel_rs/src/ble_scanner.rs` (~1010 lines)
- BLE device discovery: passive/active scan modes via NimBLE `ble_gap_disc()`
- Advertising data TLV parser: device names, 16-bit and 128-bit UUIDs, manufacturer data (company ID + payload), flags
- Storage for up to 64 discovered devices with auto-update on repeated discovery
- RSSI filtering (minimum signal strength threshold)
- Name prefix filtering
- Sort by RSSI (strongest first)
- Find by MAC address or name substring
- Scan statistics: device count, total advertisements, signal strength range
- 13 FFI exports for syscall table integration
- 45 unit tests across 9 categories

### Persona Review (Iteration 7)

**Spark (Dani):**
> "Finally! This is exactly what I need. I can scan for BLE devices, see their services, manufacturer data, and signal strength. The RSSI filtering is great for proximity work — set it to -60 dBm and I only see nearby devices. The company ID extraction lets me quickly identify device manufacturers. Now I need the UI app to display all this."

**Thorn (Amara):**
> "Good for counter-surveillance. I can check what BLE devices are near me and whether any are tracking beacons. The name and manufacturer data parsing helps identify suspicious devices."

**Cairn (Ewan):**
> "Not my primary need, but useful for debugging BLE relay connections with my team."

**Fern (Yuki):**
> "The advertising data parser is well-structured. I can use this to verify my custom BLE peripherals are advertising correctly."

### Open Questions (Iteration 7)

- **Q1: BLE scanner + BLE manager coexistence.** Scanning and NUS advertising should coexist on NimBLE, but scanning while connected may conflict. Need real-hardware testing.
- **Q2: Scanner UI app needed.** The kernel module is the engine; a UI app with scrollable device list, RSSI bars, and detail view is still needed.
- **Q3: Scan callback threading.** NimBLE callback runs on host task; FFI queries run on app task. No deadlock in practice, but worth documenting.

---

## Iteration 8

### Review Phase

Remaining PENDING: P2 (power driver, mostly done), P3 (internet transport), P8 (driver hot-reload), P9 (MeshCore), P12 (burn timer), P13 (LoRa queue).

**Selected:** P12 — Message Burn Timer. High impact for Thorn (journalist source protection). Small effort, no dependencies.

### Build Results (Iteration 8)

| # | Feature | Status | Notes |
|---|---------|--------|-------|
| 1 | Message Burn Timer kernel module | DONE | `burn_timer.rs` — 47 tests, 635/635 suite pass |

**What was built:**
- Pure Rust kernel module at `components/kernel_rs/src/burn_timer.rs` (~660 lines)
- Per-message burn timers: set duration, tick monotonic clock, drain expired entries
- Per-conversation burn policies: enable auto-burn with default duration for all new messages
- Expired queue: tick() detects expired timers, get_expired() drains them for messenger to act on
- Countdown remaining: query ms left on any timer
- Circular buffer awareness: setting timer on reused slot auto-cancels old timer
- 12 FFI exports, 47 unit tests

### Persona Review (Iteration 8)

**Thorn (Amara):**
> "This is exactly what I've been asking for. Per-conversation policies mean I can set all SMS conversations to auto-burn after 10 minutes. The expired queue model means the messenger wipes the data — I know the message text is gone, not just hidden. Combined with secure wipe, I now have good data hygiene tools."

**Cairn (Ewan):**
> "Useful for sensitive casualty information. I'll set a 24-hour burn on SAR conversations after an operation ends."

### Open Questions (Iteration 8)
- **Q1: Messenger UI integration.** The messenger needs to call tick() periodically and wipe expired messages.
- **Q2: Duration setting UI.** How does the user set burn duration? Per-conversation settings menu needed.
- **Q3: Clock source.** Module uses caller-provided monotonic time. ESP-IDF: esp_timer_get_time(). Simulator: SDL_GetTicks().

---

## Iteration 9

### Review Phase

Remaining PENDING: P2 (power driver), P3 (internet transport), P8 (driver hot-reload), P9 (MeshCore), P13 (LoRa queue).

**Selected:** P13 — LoRa Store-and-Forward Message Queue. Critical for off-grid reliability (Cairn's SAR team, Ember's field stations).

### Build Results (Iteration 9)

| # | Feature | Status | Notes |
|---|---------|--------|-------|
| 1 | Message Queue kernel module | DONE | `msg_queue.rs` — 53 tests, 641/641 suite pass |

**What was built:**
- Pure Rust kernel module at `components/kernel_rs/src/msg_queue.rs` (~900 lines)
- Store-and-forward queue for up to 64 messages with exponential backoff retry
- Priority ordering: Urgent > High > Normal for send scheduling
- Configurable TTL (default 1hr) and max retries (default 10) per message
- Exponential backoff: 5s base, 2x multiplier, 5min max cap
- JSON persistence to SD card with base64-encoded payloads
- tick()/get_ready()/mark_sent()/mark_failed() lifecycle for transport integration
- 15 FFI exports, 53 unit tests

### Persona Review (Iteration 9)

**Cairn (Ewan):**
> "This changes everything. When I send a position update and the relay is behind a ridge, the message queues and auto-retries. When my team member crests the ridge and comes in range, the queued messages go out automatically. With Urgent priority for SOS messages, they'll always be first in line."

**Ember (Rafael):**
> "My field assistants send observation counts to me daily. If I'm out on the river when they send, the messages queue at their station and auto-deliver when they come in range of the relay node. The SD card persistence means even a power cycle doesn't lose queued data."

### Open Questions (Iteration 9)
- **Q1: Transport integration.** The C messenger transport needs to call enqueue/tick/get_ready FFI. C-side change needed.
- **Q2: LoRa ACK protocol.** No way to know if broadcast was received. Lightweight ACK (hash-based) could be added later.
- **Q3: Queue status UI.** Status bar should show queued message count. Notification manager could help.

---

## Iteration 10

### Review Phase

Remaining PENDING: P2 (power driver, mostly done), P3 (internet transport, needs server), P8 (driver hot-reload), P9 (MeshCore, needs research).

Persona gap analysis found critical missing feature: **E2E message encryption**. Thorn has contacts with public keys, burn timers, and secure wipe — but messages themselves travel in plaintext. This undermines the entire security story.

New backlog item P14 added. Selected for build.

### Build Results (Iteration 10)

| # | Feature | Status | Notes |
|---|---------|--------|-------|
| 1 | E2E Message Encryption | DONE | `msg_crypto.rs` — 48 tests, 636/636 suite pass |

**What was built:**
- Pure Rust kernel module at `components/kernel_rs/src/msg_crypto.rs` (~730 lines)
- AES-256-CTR encryption + HMAC-SHA256 authentication (encrypt-then-MAC)
- PBKDF2-SHA256 key derivation (10000 iterations) at channel establishment
- Per-message key derivation via HMAC(master_key, nonce || purpose) — fast, unique per message
- Wire format: [version:1 | nonce:16 | ciphertext:N | hmac:32] — 49 bytes overhead
- Up to 32 encrypted channels, keyed by contact_id
- Constant-time HMAC comparison, key zeroization on destroy
- 12 FFI exports, 48 unit tests

### Persona Review (Iteration 10)

**Thorn (Amara):**
> "This completes my security stack. Secure wipe destroys data at rest. Burn timer destroys messages after time. Now encryption protects data in transit. I share a passphrase with my editor when we meet in person, establish the channel, and from then on our LoRa messages are encrypted. Even if someone captures the radio traffic, they can't read it without the passphrase."

**Cairn (Ewan):**
> "For sensitive casualty information, I can set up encrypted channels with the team leader and the medical coordinator. Standard position updates stay unencrypted for the wider team, but medical details are encrypted."

### Open Questions (Iteration 10)
- **Q1: Key exchange UX.** QR code or NFC for passphrase exchange would help.
- **Q2: Messenger integration.** Detect encrypted messages by version byte, resolve contact, decrypt. C-side change.
- **Q3: PBKDF2 performance.** May be slow on ESP32. Channel establishment should show progress.
- **Q4: Forward secrecy.** Current design has per-message nonces. True forward secrecy needs ratcheting protocol (deferred).

---

## Iteration 11

### Review Phase

Remaining PENDING: P2 (power driver), P3 (internet transport), P8 (driver hot-reload), P9 (MeshCore).

Fern (Yuki) has had only 1 feature delivered across all iterations. Driver hot-reload (P8) is her primary ask. Selected.

### Build Results (Iteration 11)

| # | Feature | Status | Notes |
|---|---------|--------|-------|
| 1 | Driver Hot-Reload kernel module | DONE | `driver_reload.rs` — 54 tests, 642/642 suite pass |

**What was built:**
- Pure Rust kernel module at `components/kernel_rs/src/driver_reload.rs` (~850 lines)
- Driver lifecycle state machine: Empty → Loaded → Running → Stopped → (unload) → Empty
- Auto-stop on reload from Running state
- Error recovery: reload from Error state attempts recovery
- 10 HAL type categories tracked
- Reload by driver ID or by file path
- Version tracking and load count per driver
- Platform abstraction for ESP-IDF vs test builds
- 16 FFI exports, 54 unit tests

### Persona Review (Iteration 11)

**Fern (Yuki):**
> "Finally! I can iterate on my soil sensor driver without rebooting. Write Rust, compile to .drv.elf, copy to SD, call reload — the new version is running. The state machine catches my mistakes (can't unload while running, must stop first). Load count tells me which version I'm on."

### Open Questions (Iteration 11)
- **Q1: HAL deregister.** HAL registry needs null-out functions to prevent dangling vtable pointers after unload.
- **Q2: File watching.** Auto-detect modified .drv.elf files for seamless development.
- **Q3: Display/input reload safety.** Coordinate with display server to avoid visual glitches during driver reload.
