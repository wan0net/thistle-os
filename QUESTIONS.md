# ThistleOS — Open Questions

## Iteration 1 — 2026-03-26

(No open questions yet — first iteration.)

### Q1: CI only triggers on main/PR-to-main
GitHub Actions workflows (`build.yml`, `tests.yml`) only run on `push: main` and `pull_request: main`. Feature branches don't trigger CI unless a PR is opened. The loop constraint says "never merge to main" — should we open draft PRs to trigger CI, or add branch triggers to the workflows?
**Workaround used:** Ran full cargo test locally (632/632 pass including 44 new). Feature branch is push-verified but not CI-verified.

## Iteration 6 — 2026-03-26 (Contact Manager)

### Q2: Messenger UI contact resolution
The contact manager provides `rs_contact_find_by_device_id()` and `rs_contact_find_by_phone()` for resolving sender strings to contact names. The messenger UI (`messenger_ui.c`) currently displays raw strings like "Node-XXXX". Wiring the lookup into the UI is a C-side change in the messenger app — when should this happen?

### Q3: Contact picker widget for messenger
When composing a new message, the user should be able to pick a recipient from contacts. Should this be a shared thistle-tk widget or messenger-specific?

### Q4: SOS beacon ↔ contact manager coordination
`rs_contact_get_emergency()` is ready but the SOS beacon module is on a separate branch (`feat/sos-beacon`). Integration needs to happen after both merge to main.

### Q5: Contact sync across devices
Cairn's SAR team needs the same contact roster on all devices. No sync mechanism exists. vCard broadcast over LoRa is bandwidth-heavy. Could use a lightweight roster-hash-then-sync protocol, or just SD card sneakernet.

### Q6: Event bus capacity for ContactsChanged
The event bus currently has 17 types (EVENT_MAX). Adding a `ContactsChanged` type would let messenger/SOS auto-refresh. But EVENT_MAX needs to increase, which changes the C header too.

## Iteration 7 — 2026-03-27 (BLE Scanner)

### Q7: BLE scanner coexistence with BLE manager
The `ble_manager` runs NimBLE for NUS (Nordic UART Service) advertising. The `ble_scanner` uses `ble_gap_disc()` for discovery. On NimBLE, scanning and advertising can coexist but scanning and connected state may conflict. Need to test: can scanning run while NUS is connected? Should the scanner check BLE manager state before starting?

### Q8: BLE scanner UI app
The kernel module provides the scanning engine and FFI. A UI app (`ble_scanner_app`) is still needed to display results — a scrollable list with device name, MAC, RSSI bars, service count, and tap-to-detail. This would be a C app using LVGL or a Rust app using thistle-tk.

### Q9: BLE scanner scan callback threading
The NimBLE scan callback (`ble_gap_disc` event handler) runs on the NimBLE host task thread. The callback locks the global `SCANNER` mutex. If any FFI query function is called from the same thread, it would deadlock. In practice this won't happen because FFI queries come from the app task, not the NimBLE host task. But worth documenting as a constraint.

## Iteration 8 — 2026-03-27 (Burn Timer)

### Q10: Messenger integration for burn timer
The burn timer kernel module provides the timing engine. The messenger UI needs changes to: (a) call `rs_burn_timer_tick(now_ms)` periodically, (b) call `rs_burn_timer_get_expired()` and wipe the corresponding message slots, (c) show countdown overlay on burn-timed messages. This is a C-side change in `messenger_ui.c`.

### Q11: Burn timer UI for setting duration
Users need a way to set burn duration per conversation. Could be a settings menu within the messenger, or a per-conversation option accessed via long-press. UX design needed.

### Q12: Clock source for burn timer
The module uses monotonic time provided by the caller. On ESP-IDF, `esp_timer_get_time()` provides microsecond monotonic time. The messenger should convert to milliseconds and pass to `rs_burn_timer_tick()`. On simulator, SDL_GetTicks() could be used.

## Iteration 9 — 2026-03-27 (LoRa Message Queue)

### Q13: Transport layer integration for message queue
The msg_queue module provides the queue engine, but the messenger transport layer (`messenger_transport.c`) needs to: (a) call `rs_msg_queue_enqueue()` when a send fails or when explicitly queueing, (b) call `rs_msg_queue_tick()` periodically, (c) call `rs_msg_queue_get_ready()` and attempt resend. This is a C-side change.

### Q14: LoRa ACK mechanism
LoRa is broadcast with no ACK. The msg_queue currently treats `mark_sent()` as "attempt made" — it can't know if anyone received it. For reliable delivery, a lightweight ACK protocol would be needed (recipient sends a short ACK packet with message hash). This is a protocol-level change that could be added later.

### Q15: Queue UI indicators
Users should see queued message count and retry status. Status bar badge showing "3 queued" would help. The notification manager (feat/notification-manager) could display queue status.

## Iteration 10 — 2026-03-27 (Message Encryption)

### Q16: Key exchange UX
Currently, both contacts must know the passphrase (shared out-of-band, e.g., in person). A QR code exchange or NFC tap could simplify this. The contact manager could store the channel passphrase hash for UI display ("encrypted channel active with CAIRN-1").

### Q17: Messenger integration for encryption
The messenger transport needs to: (a) check `rs_msg_crypto_is_active(contact_id)` before sending, (b) call `rs_msg_crypto_encrypt()` to wrap plaintext, (c) on receive, detect encrypted messages (version byte 0x01), resolve contact, call `rs_msg_crypto_decrypt()`. This requires contact manager integration (find contact by device_id → get contact_id → check crypto channel).

### Q18: PBKDF2 performance on ESP32
10000 PBKDF2 iterations takes ~5s on host. On ESP32-S3 at 240MHz it may take longer. Channel establishment should be async or show a progress indicator. Per-message encryption is fast (HMAC + AES-CTR).

### Q19: Forward secrecy
The current design derives per-message keys from nonces, providing some forward secrecy (compromised master key + old nonce = old message keys, but attacker needs the ciphertext too). True forward secrecy would require a ratcheting protocol (like Signal's Double Ratchet). Deferred — current design is sufficient for the threat model.

## Iteration 11 — 2026-03-27 (Driver Hot-Reload)

### Q20: HAL registry deregister functions
The driver_reload module manages lifecycle state but the HAL registry still has no deregister functions. When a driver is "unloaded" via driver_reload, the HAL registry still holds the old vtable pointer. On real hardware, the ESP-IDF `esp_elf_deinit()` frees the ELF memory, making the old vtable pointer dangling. Deregister functions need to be added to `hal_registry.rs` to null out pointers before unload.

### Q21: Driver reload file watching
For a Fern-like development workflow, the system should watch the SD card for modified .drv.elf files and auto-trigger reload. This could use a polling approach (check file timestamps periodically) or an inotify-like mechanism.

### Q22: Reload safety for display/input drivers
Reloading the display or input driver while the user is interacting would cause a visible glitch or input loss. The reload system should coordinate with the display server to show a "reloading driver..." screen and buffer input events during the reload window.

## Crypto / Syscall Review — 2026-03-28

### Q23: AES-128-ECB has no hardware acceleration path — **RESOLVED**
**Resolution:** HAL vtable extended with `aes128_ecb_encrypt`/`aes128_ecb_decrypt` fields. The `drv_crypto_mbedtls` driver implements them via ESP-IDF mbedtls hardware AES.

### Q24: PBKDF2 bypasses hardware SHA entirely
The Rust `pbkdf2` crate calls its own internal SHA-256, bypassing the HAL crypto dispatch. On ESP32-S3, hardware SHA is 3-5x faster. Options: (a) rewrite PBKDF2 to use `thistle_crypto_hmac_sha256` per iteration, (b) accept software-only PBKDF2 since it's infrequent.
**Note:** Deferred to post-1.0 — acceptable for infrequent key derivation.

### Q25: thistle_fs_open ABI mismatch — **RESOLVED**
**Resolution:** SDK header now uses `const char *mode` (fopen-style), matching the implementation.

### Q26: thistle_log is not variadic — **RESOLVED**
**Resolution:** SDK header now declares `thistle_log(const char *tag, const char *msg)` without variadic arguments, matching the Rust implementation.
