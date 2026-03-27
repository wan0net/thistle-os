// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — syscall_table module
//
// Port of components/kernel/src/syscall_table.c
// Provides a static table of (name, function-pointer) pairs that maps
// kernel/HAL symbol names to their addresses for ELF dynamic linking.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0x000;

// ---------------------------------------------------------------------------
// C functions exported in the syscall table
//
// Each extern "C" block declares one group of related symbols.
// On simulator builds the ESP-IDF-specific ones are excluded.
// ---------------------------------------------------------------------------

extern "C" {
    // Kernel subsystems (Rust)
    fn kernel_uptime_ms() -> u32;

    // IPC (Rust)
    fn ipc_send(msg: *const c_void) -> i32;
    fn ipc_recv(msg: *mut c_void, timeout_ms: u32) -> i32;

    // Event bus (Rust)
    fn event_subscribe(event_type: u32, handler: *const c_void, user_data: *mut c_void) -> i32;
    fn event_publish(event: *const c_void) -> i32;

    // HAL registry
    fn hal_get_registry() -> *const c_void;
    fn hal_display_register(drv: *const c_void, cfg: *const c_void) -> i32;
    fn hal_input_register(drv: *const c_void, cfg: *const c_void) -> i32;
    fn hal_radio_register(drv: *const c_void, cfg: *const c_void) -> i32;
    fn hal_gps_register(drv: *const c_void, cfg: *const c_void) -> i32;
    fn hal_audio_register(drv: *const c_void, cfg: *const c_void) -> i32;
    fn hal_power_register(drv: *const c_void, cfg: *const c_void) -> i32;
    fn hal_imu_register(drv: *const c_void, cfg: *const c_void) -> i32;
    fn hal_storage_register(drv: *const c_void, cfg: *const c_void) -> i32;
    fn hal_set_board_name(name: *const c_char);

    // HAL bus
    fn hal_bus_register_spi(bus: *const c_void) -> i32;
    fn hal_bus_register_i2c(bus: *const c_void) -> i32;
    fn hal_bus_get_spi(id: u32) -> *const c_void;
    fn hal_bus_get_i2c(id: u32) -> *const c_void;

    // Driver loader config (Rust)
    fn driver_loader_get_config() -> *const c_char;

    // Logging
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

// FreeRTOS — stubs provided in simulator. Not available on aarch64-apple-darwin test builds.
#[cfg(not(test))]
extern "C" {
    fn vTaskDelay(ticks: u32);
    fn xTaskCreatePinnedToCore(
        task_fn: *const c_void,
        name: *const c_char,
        stack: u32,
        param: *mut c_void,
        prio: u32,
        handle: *mut *mut c_void,
        core: i32,
    ) -> i32;
    fn vTaskDelete(task: *mut c_void);
    fn xQueueGenericCreate(length: u32, item_size: u32, queue_type: u8) -> *mut c_void;
    fn xQueueGenericSend(
        queue: *mut c_void,
        item: *const c_void,
        ticks_to_wait: u32,
        copy_pos: i32,
    ) -> i32;
    fn xQueueReceive(queue: *mut c_void, buf: *mut c_void, ticks_to_wait: u32) -> i32;
}

// ---------------------------------------------------------------------------
// ESP-IDF platform-specific symbols (hardware only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn esp_timer_create(args: *const c_void, out: *mut *mut c_void) -> i32;
    fn esp_timer_start_periodic(timer: *mut c_void, period_us: i64) -> i32;
    fn esp_timer_start_once(timer: *mut c_void, timeout_us: i64) -> i32;
    fn esp_timer_stop(timer: *mut c_void) -> i32;
    fn esp_timer_delete(timer: *mut c_void) -> i32;

    fn gpio_config(config: *const c_void) -> i32;
    fn gpio_set_level(pin: u32, level: u32) -> i32;
    fn gpio_get_level(pin: u32) -> i32;
    fn gpio_set_direction(pin: u32, mode: u32) -> i32;
    fn gpio_set_pull_mode(pin: u32, mode: u32) -> i32;
    fn gpio_isr_handler_add(pin: u32, handler: *const c_void, arg: *mut c_void) -> i32;
    fn gpio_isr_handler_remove(pin: u32) -> i32;
    fn gpio_install_isr_service(flags: i32) -> i32;

    fn spi_bus_initialize(host: u32, cfg: *const c_void, dma_chan: i32) -> i32;
    fn spi_bus_add_device(host: u32, cfg: *const c_void, handle: *mut *mut c_void) -> i32;
    fn spi_bus_remove_device(handle: *mut c_void) -> i32;
    fn spi_device_polling_transmit(handle: *mut c_void, trans: *mut c_void) -> i32;
    fn spi_device_transmit(handle: *mut c_void, trans: *mut c_void) -> i32;

    fn i2c_new_master_bus(cfg: *const c_void, handle: *mut *mut c_void) -> i32;
    fn i2c_master_bus_add_device(bus: *mut c_void, cfg: *const c_void, handle: *mut *mut c_void) -> i32;
    fn i2c_master_bus_rm_device(handle: *mut c_void) -> i32;
    fn i2c_master_transmit(handle: *mut c_void, data: *const u8, len: usize, timeout_ms: i32) -> i32;
    fn i2c_master_receive(handle: *mut c_void, buf: *mut u8, len: usize, timeout_ms: i32) -> i32;
    fn i2c_master_transmit_receive(
        handle: *mut c_void,
        write_data: *const u8,
        write_size: usize,
        read_data: *mut u8,
        read_size: usize,
        timeout_ms: i32,
    ) -> i32;

    fn uart_driver_install(port: u32, rx_buf: i32, tx_buf: i32, queue_size: i32, queue: *mut *mut c_void, flags: i32) -> i32;
    fn uart_param_config(port: u32, cfg: *const c_void) -> i32;
    fn uart_set_pin(port: u32, tx: i32, rx: i32, rts: i32, cts: i32) -> i32;
    fn uart_read_bytes(port: u32, buf: *mut u8, len: u32, timeout: u32) -> i32;
    fn uart_write_bytes(port: u32, buf: *const u8, len: usize) -> i32;
}

// ---------------------------------------------------------------------------
// Syscall entry type
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct SyscallEntry {
    pub name: *const c_char,
    pub func_ptr: *const c_void,
}

// SAFETY: All pointers are to static data and long-lived C function pointers.
unsafe impl Send for SyscallEntry {}
unsafe impl Sync for SyscallEntry {}

// ---------------------------------------------------------------------------
// Syscall implementations (thin wrappers calling kernel subsystems)
// ---------------------------------------------------------------------------

// thistle_log calls esp_log_write (not available on host test builds).
// thistle_millis calls kernel_uptime_ms which calls esp_timer_get_time (also not on host).
// Both are excluded from test builds; the test-mode SYSCALL_TABLE omits these entries.
#[cfg(not(test))]
unsafe extern "C" fn thistle_log(tag: *const c_char, msg: *const c_char) {
    let t = if tag.is_null() { b"app\0".as_ptr() } else { tag as *const u8 };
    let m = if msg.is_null() { b"\0".as_ptr() } else { msg as *const u8 };
    esp_log_write(3 /* INFO */, t, b"%s\0".as_ptr(), m);
}

#[cfg(not(test))]
unsafe extern "C" fn thistle_millis() -> u32 {
    kernel_uptime_ms()
}

unsafe extern "C" fn thistle_delay(ms: u32) {
    #[cfg(target_os = "espidf")]
    {
        // pdMS_TO_TICKS: on 100 Hz tick rate, 1 tick = 10 ms
        vTaskDelay((ms + 9) / 10);
    }
    #[cfg(not(target_os = "espidf"))]
    {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

unsafe extern "C" fn thistle_malloc(size: usize) -> *mut c_void {
    malloc(size)
}

unsafe extern "C" fn thistle_free(ptr: *mut c_void) {
    free(ptr);
}

unsafe extern "C" fn thistle_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    realloc(ptr, size)
}

unsafe extern "C" fn thistle_msg_send(msg: *const c_void) -> i32 {
    ipc_send(msg)
}

unsafe extern "C" fn thistle_msg_recv(msg: *mut c_void, timeout_ms: u32) -> i32 {
    ipc_recv(msg, timeout_ms)
}

unsafe extern "C" fn thistle_event_subscribe(
    event_type: u32,
    handler: *const c_void,
    user_data: *mut c_void,
) -> i32 {
    event_subscribe(event_type, handler, user_data)
}

unsafe extern "C" fn thistle_event_publish(event: *const c_void) -> i32 {
    event_publish(event)
}

// Crypto (Rust, in crypto.rs) — pure Rust with optional hardware dispatch
extern "C" {
    fn thistle_crypto_sha256(data: *const u8, len: usize, hash_out: *mut u8) -> i32;
    fn thistle_crypto_hmac_sha256(key: *const u8, key_len: usize, data: *const u8, data_len: usize, mac_out: *mut u8) -> i32;
    fn thistle_crypto_hmac_verify(key: *const u8, key_len: usize, data: *const u8, data_len: usize, expected_mac: *const u8) -> i32;
    fn thistle_crypto_aes256_cbc_encrypt(key: *const u8, iv: *const u8, plaintext: *const u8, len: usize, ciphertext_out: *mut u8) -> i32;
    fn thistle_crypto_aes256_cbc_decrypt(key: *const u8, iv: *const u8, ciphertext: *const u8, len: usize, plaintext_out: *mut u8) -> i32;
    fn thistle_crypto_pbkdf2_sha256(password: *const c_char, salt: *const u8, salt_len: usize, iterations: u32, key_out: *mut u8, key_len: usize) -> i32;
    fn thistle_crypto_random(buf: *mut u8, len: usize) -> i32;
    fn thistle_crypto_aes128_ecb_encrypt(key: *const u8, plaintext: *const u8, len: usize, ciphertext_out: *mut u8) -> i32;
    fn thistle_crypto_aes128_ecb_decrypt(key: *const u8, ciphertext: *const u8, len: usize, plaintext_out: *mut u8) -> i32;
    fn thistle_crypto_ed25519_keygen(private_key_out: *mut u8, public_key_out: *mut u8) -> i32;
    fn thistle_crypto_ed25519_sign(private_key: *const u8, message: *const u8, msg_len: usize, signature_out: *mut u8) -> i32;
    fn thistle_crypto_ed25519_verify(public_key: *const u8, message: *const u8, msg_len: usize, signature: *const u8) -> i32;
    fn thistle_crypto_ed25519_derive_public(private_key: *const u8, public_key_out: *mut u8) -> i32;
}

// Mesh service (Rust, in mesh_manager.rs) — wrappers around rs_mesh_* functions
extern "C" {
    fn thistle_mesh_init(name: *const c_char, node_type: u8) -> i32;
    fn thistle_mesh_deinit() -> i32;
    fn thistle_mesh_loop() -> i32;
    fn thistle_mesh_send(dest_key: *const u8, text: *const c_char) -> i32;
    fn thistle_mesh_send_advert() -> i32;
    fn thistle_mesh_send_advert_pos(lat: f64, lon: f64) -> i32;
    fn thistle_mesh_get_contact_count() -> i32;
    fn thistle_mesh_get_contact(index: i32, out: *mut c_void) -> i32;
    fn thistle_mesh_find_contact(pub_key: *const u8) -> i32;
    fn thistle_mesh_get_inbox_count() -> i32;
    fn thistle_mesh_get_inbox_message(index: i32, out: *mut c_void) -> i32;
    fn thistle_mesh_clear_inbox() -> i32;
    fn thistle_mesh_get_self_key(out: *mut u8) -> i32;
    fn thistle_mesh_get_self_name() -> *const c_char;
    fn thistle_mesh_get_stats(out: *mut c_void) -> i32;
}

// libc memory allocation — available in both espidf and simulator
extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
}

// libc file I/O — available in both espidf and simulator
extern "C" {
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fread(buf: *mut c_void, size: usize, count: usize, stream: *mut c_void) -> usize;
    fn fwrite(buf: *const c_void, size: usize, count: usize, stream: *mut c_void) -> usize;
    fn fclose(stream: *mut c_void) -> i32;
}

// Display helpers — read from Rust HAL registry
unsafe extern "C" fn thistle_display_get_width() -> u16 {
    let reg = crate::hal_registry::registry();
    if !reg.display.is_null() {
        (*reg.display).width
    } else {
        320 // default
    }
}

unsafe extern "C" fn thistle_display_get_height() -> u16 {
    let reg = crate::hal_registry::registry();
    if !reg.display.is_null() {
        (*reg.display).height
    } else {
        240 // default
    }
}

// ── HAL syscall implementations (pure Rust, using Rust HAL registry) ─

unsafe extern "C" fn thistle_input_register_cb_impl(cb: *const c_void, user_data: *mut c_void) -> i32 {
    let reg = crate::hal_registry::registry();
    for i in 0..(reg.input_count as usize) {
        if !reg.inputs[i].is_null() {
            if let Some(register_cb) = (*reg.inputs[i]).register_callback {
                let typed_cb: crate::hal_registry::HalInputCb = core::mem::transmute(cb);
                register_cb(typed_cb, user_data);
            }
        }
    }
    0
}

unsafe extern "C" fn thistle_radio_send_impl(data: *const u8, len: usize) -> i32 {
    let reg = crate::hal_registry::registry();
    if !reg.radio.is_null() {
        if let Some(send) = (*reg.radio).send {
            return send(data, len);
        }
    }
    -1
}

unsafe extern "C" fn thistle_radio_start_rx_impl(cb: *const c_void, user_data: *mut c_void) -> i32 {
    let reg = crate::hal_registry::registry();
    if !reg.radio.is_null() {
        if let Some(start_receive) = (*reg.radio).start_receive {
            let typed_cb: crate::hal_registry::HalRadioRxCb = core::mem::transmute(cb);
            return start_receive(typed_cb, user_data);
        }
    }
    -1
}

unsafe extern "C" fn thistle_radio_set_freq_impl(freq_hz: u32) -> i32 {
    let reg = crate::hal_registry::registry();
    if !reg.radio.is_null() {
        if let Some(set_frequency) = (*reg.radio).set_frequency {
            return set_frequency(freq_hz);
        }
    }
    -1
}

unsafe extern "C" fn thistle_gps_get_position_impl(pos: *mut c_void) -> i32 {
    let reg = crate::hal_registry::registry();
    if !reg.gps.is_null() {
        if let Some(get_position) = (*reg.gps).get_position {
            return get_position(pos as *mut crate::hal_registry::HalGpsPosition);
        }
    }
    -1
}

unsafe extern "C" fn thistle_gps_enable_impl() -> i32 {
    let reg = crate::hal_registry::registry();
    if !reg.gps.is_null() {
        if let Some(enable) = (*reg.gps).enable {
            return enable();
        }
    }
    -1
}

unsafe extern "C" fn thistle_power_get_battery_mv_impl() -> u16 {
    let reg = crate::hal_registry::registry();
    if !reg.power.is_null() {
        if let Some(get_battery_mv) = (*reg.power).get_battery_mv {
            return get_battery_mv();
        }
    }
    0
}

unsafe extern "C" fn thistle_power_get_battery_pct_impl() -> u8 {
    let reg = crate::hal_registry::registry();
    if !reg.power.is_null() {
        if let Some(get_battery_percent) = (*reg.power).get_battery_percent {
            return get_battery_percent();
        }
    }
    0
}

// ── File I/O syscall wrappers ─────────────────────────────────────────

unsafe extern "C" fn thistle_fs_open_impl(path: *const c_char, mode: *const c_char) -> *mut c_void {
    fopen(path, mode)
}

unsafe extern "C" fn thistle_fs_read_impl(buf: *mut c_void, size: usize, count: usize, stream: *mut c_void) -> i32 {
    fread(buf, size, count, stream) as i32
}

unsafe extern "C" fn thistle_fs_write_impl(buf: *const c_void, size: usize, count: usize, stream: *mut c_void) -> i32 {
    fwrite(buf, size, count, stream) as i32
}

unsafe extern "C" fn thistle_fs_close_impl(stream: *mut c_void) -> i32 {
    fclose(stream)
}

// Widget API (Rust, defined in widget.rs). Not available on aarch64-apple-darwin test builds.
#[cfg(not(test))]
extern "C" {
    fn thistle_ui_get_app_root() -> u32;
    fn thistle_ui_create_container(parent: u32) -> u32;
    fn thistle_ui_create_label(parent: u32, text: *const c_char) -> u32;
    fn thistle_ui_create_button(parent: u32, text: *const c_char) -> u32;
    fn thistle_ui_create_text_input(parent: u32, placeholder: *const c_char) -> u32;
    fn thistle_ui_destroy(widget: u32);
    fn thistle_ui_set_text(widget: u32, text: *const c_char);
    fn thistle_ui_get_text(widget: u32) -> *const c_char;
    fn thistle_ui_set_size(widget: u32, w: i32, h: i32);
    fn thistle_ui_set_pos(widget: u32, x: i32, y: i32);
    fn thistle_ui_set_visible(widget: u32, visible: bool);
    fn thistle_ui_set_bg_color(widget: u32, color: u32);
    fn thistle_ui_set_text_color(widget: u32, color: u32);
    fn thistle_ui_set_font_size(widget: u32, size: i32);
    fn thistle_ui_set_layout(widget: u32, layout: i32);
    fn thistle_ui_set_align(widget: u32, main_a: i32, cross_a: i32);
    fn thistle_ui_set_gap(widget: u32, gap: i32);
    fn thistle_ui_set_flex_grow(widget: u32, grow: i32);
    fn thistle_ui_set_scrollable(widget: u32, scrollable: bool);
    fn thistle_ui_set_padding(widget: u32, t: i32, r: i32, b: i32, l: i32);
    fn thistle_ui_set_border_width(widget: u32, w: i32);
    fn thistle_ui_set_radius(widget: u32, r: i32);
    fn thistle_ui_on_event(widget: u32, event_type: i32, cb: *const c_void, ud: *mut c_void);
    fn thistle_ui_set_password_mode(widget: u32, pw: bool);
    fn thistle_ui_set_one_line(widget: u32, one_line: bool);
    fn thistle_ui_set_placeholder(widget: u32, text: *const c_char);
    fn thistle_ui_theme_primary() -> u32;
    fn thistle_ui_theme_bg() -> u32;
    fn thistle_ui_theme_surface() -> u32;
    fn thistle_ui_theme_text() -> u32;
    fn thistle_ui_theme_text_secondary() -> u32;
}

// ---------------------------------------------------------------------------
// Static syscall table
//
// All entries cast their function pointers to *const c_void.
// The C code receives this as a `const syscall_entry_t *`.
// ---------------------------------------------------------------------------

macro_rules! entry {
    ($name:literal, $fn:expr) => {
        SyscallEntry {
            name: concat!($name, "\0").as_ptr() as *const c_char,
            func_ptr: $fn as *const c_void,
        }
    };
}

// Full syscall table — used in firmware and simulator builds.
// Omitted from test builds because FreeRTOS, widget API, esp_log_write, and
// esp_timer_get_time symbols are not available on aarch64-apple-darwin.
#[cfg(not(test))]
static SYSCALL_TABLE: &[SyscallEntry] = &[
    // System
    entry!("thistle_log",                   thistle_log                         as unsafe extern "C" fn(*const c_char, *const c_char)),
    entry!("thistle_millis",                thistle_millis                      as unsafe extern "C" fn() -> u32),
    entry!("thistle_delay",                 thistle_delay                       as unsafe extern "C" fn(u32)),
    entry!("thistle_malloc",                thistle_malloc                      as unsafe extern "C" fn(usize) -> *mut c_void),
    entry!("thistle_free",                  thistle_free                        as unsafe extern "C" fn(*mut c_void)),
    entry!("thistle_realloc",               thistle_realloc                     as unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void),

    // Display
    entry!("thistle_display_get_width",     thistle_display_get_width           as unsafe extern "C" fn() -> u16),
    entry!("thistle_display_get_height",    thistle_display_get_height          as unsafe extern "C" fn() -> u16),

    // Input
    entry!("thistle_input_register_cb",     thistle_input_register_cb_impl      as unsafe extern "C" fn(*const c_void, *mut c_void) -> i32),

    // Radio
    entry!("thistle_radio_send",            thistle_radio_send_impl             as unsafe extern "C" fn(*const u8, usize) -> i32),
    entry!("thistle_radio_start_rx",        thistle_radio_start_rx_impl         as unsafe extern "C" fn(*const c_void, *mut c_void) -> i32),
    entry!("thistle_radio_set_freq",        thistle_radio_set_freq_impl         as unsafe extern "C" fn(u32) -> i32),

    // GPS
    entry!("thistle_gps_get_position",      thistle_gps_get_position_impl       as unsafe extern "C" fn(*mut c_void) -> i32),
    entry!("thistle_gps_enable",            thistle_gps_enable_impl             as unsafe extern "C" fn() -> i32),

    // Storage
    entry!("thistle_fs_open",               thistle_fs_open_impl                as unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void),
    entry!("thistle_fs_read",               thistle_fs_read_impl                as unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> i32),
    entry!("thistle_fs_write",              thistle_fs_write_impl               as unsafe extern "C" fn(*const c_void, usize, usize, *mut c_void) -> i32),
    entry!("thistle_fs_close",              thistle_fs_close_impl               as unsafe extern "C" fn(*mut c_void) -> i32),

    // IPC
    entry!("thistle_msg_send",              thistle_msg_send                    as unsafe extern "C" fn(*const c_void) -> i32),
    entry!("thistle_msg_recv",              thistle_msg_recv                    as unsafe extern "C" fn(*mut c_void, u32) -> i32),
    entry!("thistle_event_subscribe",       thistle_event_subscribe             as unsafe extern "C" fn(u32, *const c_void, *mut c_void) -> i32),
    entry!("thistle_event_publish",         thistle_event_publish               as unsafe extern "C" fn(*const c_void) -> i32),

    // Power
    entry!("thistle_power_get_battery_mv",  thistle_power_get_battery_mv_impl   as unsafe extern "C" fn() -> u16),
    entry!("thistle_power_get_battery_pct", thistle_power_get_battery_pct_impl  as unsafe extern "C" fn() -> u8),

    // Crypto
    entry!("thistle_crypto_sha256",             thistle_crypto_sha256             as unsafe extern "C" fn(*const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_hmac_sha256",        thistle_crypto_hmac_sha256        as unsafe extern "C" fn(*const u8, usize, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_hmac_verify",        thistle_crypto_hmac_verify        as unsafe extern "C" fn(*const u8, usize, *const u8, usize, *const u8) -> i32),
    entry!("thistle_crypto_aes256_cbc_encrypt", thistle_crypto_aes256_cbc_encrypt as unsafe extern "C" fn(*const u8, *const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_aes256_cbc_decrypt", thistle_crypto_aes256_cbc_decrypt as unsafe extern "C" fn(*const u8, *const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_pbkdf2_sha256",      thistle_crypto_pbkdf2_sha256      as unsafe extern "C" fn(*const c_char, *const u8, usize, u32, *mut u8, usize) -> i32),
    entry!("thistle_crypto_random",             thistle_crypto_random             as unsafe extern "C" fn(*mut u8, usize) -> i32),
    entry!("thistle_crypto_aes128_ecb_encrypt", thistle_crypto_aes128_ecb_encrypt as unsafe extern "C" fn(*const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_aes128_ecb_decrypt", thistle_crypto_aes128_ecb_decrypt as unsafe extern "C" fn(*const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_ed25519_keygen",       thistle_crypto_ed25519_keygen       as unsafe extern "C" fn(*mut u8, *mut u8) -> i32),
    entry!("thistle_crypto_ed25519_sign",         thistle_crypto_ed25519_sign         as unsafe extern "C" fn(*const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_ed25519_verify",       thistle_crypto_ed25519_verify       as unsafe extern "C" fn(*const u8, *const u8, usize, *const u8) -> i32),
    entry!("thistle_crypto_ed25519_derive_public", thistle_crypto_ed25519_derive_public as unsafe extern "C" fn(*const u8, *mut u8) -> i32),

    // Mesh service
    entry!("thistle_mesh_init",               thistle_mesh_init               as unsafe extern "C" fn(*const c_char, u8) -> i32),
    entry!("thistle_mesh_deinit",             thistle_mesh_deinit             as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_loop",               thistle_mesh_loop               as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_send",               thistle_mesh_send               as unsafe extern "C" fn(*const u8, *const c_char) -> i32),
    entry!("thistle_mesh_send_advert",        thistle_mesh_send_advert        as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_send_advert_pos",    thistle_mesh_send_advert_pos    as unsafe extern "C" fn(f64, f64) -> i32),
    entry!("thistle_mesh_get_contact_count",  thistle_mesh_get_contact_count  as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_get_contact",        thistle_mesh_get_contact        as unsafe extern "C" fn(i32, *mut c_void) -> i32),
    entry!("thistle_mesh_find_contact",       thistle_mesh_find_contact       as unsafe extern "C" fn(*const u8) -> i32),
    entry!("thistle_mesh_get_inbox_count",    thistle_mesh_get_inbox_count    as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_get_inbox_message",  thistle_mesh_get_inbox_message  as unsafe extern "C" fn(i32, *mut c_void) -> i32),
    entry!("thistle_mesh_clear_inbox",        thistle_mesh_clear_inbox        as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_get_self_key",       thistle_mesh_get_self_key       as unsafe extern "C" fn(*mut u8) -> i32),
    entry!("thistle_mesh_get_self_name",      thistle_mesh_get_self_name      as unsafe extern "C" fn() -> *const c_char),
    entry!("thistle_mesh_get_stats",          thistle_mesh_get_stats          as unsafe extern "C" fn(*mut c_void) -> i32),

    // HAL registration
    entry!("hal_display_register",          hal_display_register                as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_input_register",            hal_input_register                  as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_radio_register",            hal_radio_register                  as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_gps_register",              hal_gps_register                    as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_audio_register",            hal_audio_register                  as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_power_register",            hal_power_register                  as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_imu_register",              hal_imu_register                    as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_storage_register",          hal_storage_register                as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_set_board_name",            hal_set_board_name                  as unsafe extern "C" fn(*const c_char)),
    entry!("hal_get_registry",              hal_get_registry                    as unsafe extern "C" fn() -> *const c_void),

    // HAL bus
    entry!("hal_bus_register_spi",          hal_bus_register_spi                as unsafe extern "C" fn(*const c_void) -> i32),
    entry!("hal_bus_register_i2c",          hal_bus_register_i2c                as unsafe extern "C" fn(*const c_void) -> i32),
    entry!("hal_bus_get_spi",               hal_bus_get_spi                     as unsafe extern "C" fn(u32) -> *const c_void),
    entry!("hal_bus_get_i2c",               hal_bus_get_i2c                     as unsafe extern "C" fn(u32) -> *const c_void),

    // FreeRTOS
    entry!("vTaskDelay",                    vTaskDelay                          as unsafe extern "C" fn(u32)),
    entry!("xTaskCreatePinnedToCore",       xTaskCreatePinnedToCore             as unsafe extern "C" fn(*const c_void, *const c_char, u32, *mut c_void, u32, *mut *mut c_void, i32) -> i32),
    entry!("vTaskDelete",                   vTaskDelete                         as unsafe extern "C" fn(*mut c_void)),
    entry!("xQueueGenericCreate",           xQueueGenericCreate                 as unsafe extern "C" fn(u32, u32, u8) -> *mut c_void),
    entry!("xQueueGenericSend",             xQueueGenericSend                   as unsafe extern "C" fn(*mut c_void, *const c_void, u32, i32) -> i32),
    entry!("xQueueReceive",                 xQueueReceive                       as unsafe extern "C" fn(*mut c_void, *mut c_void, u32) -> i32),

    // Driver config
    entry!("thistle_driver_get_config",     driver_loader_get_config            as unsafe extern "C" fn() -> *const c_char),

    // Widget API
    entry!("thistle_ui_get_app_root",       thistle_ui_get_app_root             as unsafe extern "C" fn() -> u32),
    entry!("thistle_ui_create_container",   thistle_ui_create_container         as unsafe extern "C" fn(u32) -> u32),
    entry!("thistle_ui_create_label",       thistle_ui_create_label             as unsafe extern "C" fn(u32, *const c_char) -> u32),
    entry!("thistle_ui_create_button",      thistle_ui_create_button            as unsafe extern "C" fn(u32, *const c_char) -> u32),
    entry!("thistle_ui_create_text_input",  thistle_ui_create_text_input        as unsafe extern "C" fn(u32, *const c_char) -> u32),
    entry!("thistle_ui_destroy",            thistle_ui_destroy                  as unsafe extern "C" fn(u32)),
    entry!("thistle_ui_set_text",           thistle_ui_set_text                 as unsafe extern "C" fn(u32, *const c_char)),
    entry!("thistle_ui_get_text",           thistle_ui_get_text                 as unsafe extern "C" fn(u32) -> *const c_char),
    entry!("thistle_ui_set_size",           thistle_ui_set_size                 as unsafe extern "C" fn(u32, i32, i32)),
    entry!("thistle_ui_set_pos",            thistle_ui_set_pos                  as unsafe extern "C" fn(u32, i32, i32)),
    entry!("thistle_ui_set_visible",        thistle_ui_set_visible              as unsafe extern "C" fn(u32, bool)),
    entry!("thistle_ui_set_bg_color",       thistle_ui_set_bg_color             as unsafe extern "C" fn(u32, u32)),
    entry!("thistle_ui_set_text_color",     thistle_ui_set_text_color           as unsafe extern "C" fn(u32, u32)),
    entry!("thistle_ui_set_font_size",      thistle_ui_set_font_size            as unsafe extern "C" fn(u32, i32)),
    entry!("thistle_ui_set_layout",         thistle_ui_set_layout               as unsafe extern "C" fn(u32, i32)),
    entry!("thistle_ui_set_align",          thistle_ui_set_align                as unsafe extern "C" fn(u32, i32, i32)),
    entry!("thistle_ui_set_gap",            thistle_ui_set_gap                  as unsafe extern "C" fn(u32, i32)),
    entry!("thistle_ui_set_flex_grow",      thistle_ui_set_flex_grow            as unsafe extern "C" fn(u32, i32)),
    entry!("thistle_ui_set_scrollable",     thistle_ui_set_scrollable           as unsafe extern "C" fn(u32, bool)),
    entry!("thistle_ui_set_padding",        thistle_ui_set_padding              as unsafe extern "C" fn(u32, i32, i32, i32, i32)),
    entry!("thistle_ui_set_border_width",   thistle_ui_set_border_width         as unsafe extern "C" fn(u32, i32)),
    entry!("thistle_ui_set_radius",         thistle_ui_set_radius               as unsafe extern "C" fn(u32, i32)),
    entry!("thistle_ui_on_event",           thistle_ui_on_event                 as unsafe extern "C" fn(u32, i32, *const c_void, *mut c_void)),
    entry!("thistle_ui_set_password_mode",  thistle_ui_set_password_mode        as unsafe extern "C" fn(u32, bool)),
    entry!("thistle_ui_set_one_line",       thistle_ui_set_one_line             as unsafe extern "C" fn(u32, bool)),
    entry!("thistle_ui_set_placeholder",    thistle_ui_set_placeholder          as unsafe extern "C" fn(u32, *const c_char)),
    entry!("thistle_ui_theme_primary",      thistle_ui_theme_primary            as unsafe extern "C" fn() -> u32),
    entry!("thistle_ui_theme_bg",           thistle_ui_theme_bg                 as unsafe extern "C" fn() -> u32),
    entry!("thistle_ui_theme_surface",      thistle_ui_theme_surface            as unsafe extern "C" fn() -> u32),
    entry!("thistle_ui_theme_text",         thistle_ui_theme_text               as unsafe extern "C" fn() -> u32),
    entry!("thistle_ui_theme_text_secondary", thistle_ui_theme_text_secondary   as unsafe extern "C" fn() -> u32),

    // Logging
    entry!("esp_log_write",                 esp_log_write                       as unsafe extern "C" fn(i32, *const u8, *const u8, ...)),
];

// Minimal syscall table for host (aarch64-apple-darwin) test builds.
// Only contains entries whose function bodies are pure Rust or reference
// libc/in-crate symbols resolvable without ESP-IDF or FreeRTOS.
// Excluded: thistle_log (esp_log_write), thistle_millis (esp_timer_get_time),
//           FreeRTOS entries, widget API entries, esp_log_write itself.
#[cfg(test)]
static SYSCALL_TABLE: &[SyscallEntry] = &[
    // System (host-safe only)
    entry!("thistle_delay",                 thistle_delay                       as unsafe extern "C" fn(u32)),
    entry!("thistle_malloc",                thistle_malloc                      as unsafe extern "C" fn(usize) -> *mut c_void),
    entry!("thistle_free",                  thistle_free                        as unsafe extern "C" fn(*mut c_void)),
    entry!("thistle_realloc",               thistle_realloc                     as unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void),

    // Display
    entry!("thistle_display_get_width",     thistle_display_get_width           as unsafe extern "C" fn() -> u16),
    entry!("thistle_display_get_height",    thistle_display_get_height          as unsafe extern "C" fn() -> u16),

    // Input
    entry!("thistle_input_register_cb",     thistle_input_register_cb_impl      as unsafe extern "C" fn(*const c_void, *mut c_void) -> i32),

    // Radio
    entry!("thistle_radio_send",            thistle_radio_send_impl             as unsafe extern "C" fn(*const u8, usize) -> i32),
    entry!("thistle_radio_start_rx",        thistle_radio_start_rx_impl         as unsafe extern "C" fn(*const c_void, *mut c_void) -> i32),
    entry!("thistle_radio_set_freq",        thistle_radio_set_freq_impl         as unsafe extern "C" fn(u32) -> i32),

    // GPS
    entry!("thistle_gps_get_position",      thistle_gps_get_position_impl       as unsafe extern "C" fn(*mut c_void) -> i32),
    entry!("thistle_gps_enable",            thistle_gps_enable_impl             as unsafe extern "C" fn() -> i32),

    // Storage
    entry!("thistle_fs_open",               thistle_fs_open_impl                as unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void),
    entry!("thistle_fs_read",               thistle_fs_read_impl                as unsafe extern "C" fn(*mut c_void, usize, usize, *mut c_void) -> i32),
    entry!("thistle_fs_write",              thistle_fs_write_impl               as unsafe extern "C" fn(*const c_void, usize, usize, *mut c_void) -> i32),
    entry!("thistle_fs_close",              thistle_fs_close_impl               as unsafe extern "C" fn(*mut c_void) -> i32),

    // IPC
    entry!("thistle_msg_send",              thistle_msg_send                    as unsafe extern "C" fn(*const c_void) -> i32),
    entry!("thistle_msg_recv",              thistle_msg_recv                    as unsafe extern "C" fn(*mut c_void, u32) -> i32),
    entry!("thistle_event_subscribe",       thistle_event_subscribe             as unsafe extern "C" fn(u32, *const c_void, *mut c_void) -> i32),
    entry!("thistle_event_publish",         thistle_event_publish               as unsafe extern "C" fn(*const c_void) -> i32),

    // Power
    entry!("thistle_power_get_battery_mv",  thistle_power_get_battery_mv_impl   as unsafe extern "C" fn() -> u16),
    entry!("thistle_power_get_battery_pct", thistle_power_get_battery_pct_impl  as unsafe extern "C" fn() -> u8),

    // Crypto
    entry!("thistle_crypto_sha256",             thistle_crypto_sha256             as unsafe extern "C" fn(*const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_hmac_sha256",        thistle_crypto_hmac_sha256        as unsafe extern "C" fn(*const u8, usize, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_hmac_verify",        thistle_crypto_hmac_verify        as unsafe extern "C" fn(*const u8, usize, *const u8, usize, *const u8) -> i32),
    entry!("thistle_crypto_aes256_cbc_encrypt", thistle_crypto_aes256_cbc_encrypt as unsafe extern "C" fn(*const u8, *const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_aes256_cbc_decrypt", thistle_crypto_aes256_cbc_decrypt as unsafe extern "C" fn(*const u8, *const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_pbkdf2_sha256",      thistle_crypto_pbkdf2_sha256      as unsafe extern "C" fn(*const c_char, *const u8, usize, u32, *mut u8, usize) -> i32),
    entry!("thistle_crypto_random",             thistle_crypto_random             as unsafe extern "C" fn(*mut u8, usize) -> i32),
    entry!("thistle_crypto_aes128_ecb_encrypt", thistle_crypto_aes128_ecb_encrypt as unsafe extern "C" fn(*const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_aes128_ecb_decrypt", thistle_crypto_aes128_ecb_decrypt as unsafe extern "C" fn(*const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_ed25519_keygen",       thistle_crypto_ed25519_keygen       as unsafe extern "C" fn(*mut u8, *mut u8) -> i32),
    entry!("thistle_crypto_ed25519_sign",         thistle_crypto_ed25519_sign         as unsafe extern "C" fn(*const u8, *const u8, usize, *mut u8) -> i32),
    entry!("thistle_crypto_ed25519_verify",       thistle_crypto_ed25519_verify       as unsafe extern "C" fn(*const u8, *const u8, usize, *const u8) -> i32),
    entry!("thistle_crypto_ed25519_derive_public", thistle_crypto_ed25519_derive_public as unsafe extern "C" fn(*const u8, *mut u8) -> i32),

    // Mesh service
    entry!("thistle_mesh_init",               thistle_mesh_init               as unsafe extern "C" fn(*const c_char, u8) -> i32),
    entry!("thistle_mesh_deinit",             thistle_mesh_deinit             as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_loop",               thistle_mesh_loop               as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_send",               thistle_mesh_send               as unsafe extern "C" fn(*const u8, *const c_char) -> i32),
    entry!("thistle_mesh_send_advert",        thistle_mesh_send_advert        as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_send_advert_pos",    thistle_mesh_send_advert_pos    as unsafe extern "C" fn(f64, f64) -> i32),
    entry!("thistle_mesh_get_contact_count",  thistle_mesh_get_contact_count  as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_get_contact",        thistle_mesh_get_contact        as unsafe extern "C" fn(i32, *mut c_void) -> i32),
    entry!("thistle_mesh_find_contact",       thistle_mesh_find_contact       as unsafe extern "C" fn(*const u8) -> i32),
    entry!("thistle_mesh_get_inbox_count",    thistle_mesh_get_inbox_count    as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_get_inbox_message",  thistle_mesh_get_inbox_message  as unsafe extern "C" fn(i32, *mut c_void) -> i32),
    entry!("thistle_mesh_clear_inbox",        thistle_mesh_clear_inbox        as unsafe extern "C" fn() -> i32),
    entry!("thistle_mesh_get_self_key",       thistle_mesh_get_self_key       as unsafe extern "C" fn(*mut u8) -> i32),
    entry!("thistle_mesh_get_self_name",      thistle_mesh_get_self_name      as unsafe extern "C" fn() -> *const c_char),
    entry!("thistle_mesh_get_stats",          thistle_mesh_get_stats          as unsafe extern "C" fn(*mut c_void) -> i32),

    // HAL registration (all Rust FFI exports in hal_registry.rs)
    entry!("hal_display_register",          hal_display_register                as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_input_register",            hal_input_register                  as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_radio_register",            hal_radio_register                  as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_gps_register",              hal_gps_register                    as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_audio_register",            hal_audio_register                  as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_power_register",            hal_power_register                  as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_imu_register",              hal_imu_register                    as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_storage_register",          hal_storage_register                as unsafe extern "C" fn(*const c_void, *const c_void) -> i32),
    entry!("hal_set_board_name",            hal_set_board_name                  as unsafe extern "C" fn(*const c_char)),
    entry!("hal_get_registry",              hal_get_registry                    as unsafe extern "C" fn() -> *const c_void),

    // HAL bus (Rust FFI exports in hal_registry.rs)
    entry!("hal_bus_register_spi",          hal_bus_register_spi                as unsafe extern "C" fn(*const c_void) -> i32),
    entry!("hal_bus_register_i2c",          hal_bus_register_i2c                as unsafe extern "C" fn(*const c_void) -> i32),
    entry!("hal_bus_get_spi",               hal_bus_get_spi                     as unsafe extern "C" fn(u32) -> *const c_void),
    entry!("hal_bus_get_i2c",               hal_bus_get_i2c                     as unsafe extern "C" fn(u32) -> *const c_void),

    // Driver config (Rust FFI export in driver_loader.rs)
    entry!("thistle_driver_get_config",     driver_loader_get_config            as unsafe extern "C" fn() -> *const c_char),
];

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialise the syscall table (logs count, no other work required).
///
/// # Safety
/// May be called from C.
#[no_mangle]
pub extern "C" fn syscall_table_init() -> i32 {
    unsafe {
        esp_log_write(
            3,
            b"syscall\0".as_ptr(),
            b"Syscall table initialized with %d entries\0".as_ptr(),
            SYSCALL_TABLE.len() as i32,
        );
    }
    ESP_OK
}

/// Return a pointer to the first entry in the syscall table.
///
/// # Safety
/// Returns a pointer to static data. Do not free.
#[no_mangle]
pub extern "C" fn syscall_table_get() -> *const SyscallEntry {
    SYSCALL_TABLE.as_ptr()
}

/// Return the number of entries in the syscall table.
#[no_mangle]
pub extern "C" fn syscall_table_count() -> usize {
    SYSCALL_TABLE.len()
}

/// Look up a symbol by name. Returns its address or NULL if not found.
///
/// # Safety
/// `name` must be a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn syscall_resolve(name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }

    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    for entry in SYSCALL_TABLE {
        let entry_name = match CStr::from_ptr(entry.name).to_str() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if entry_name == name_str {
            return entry.func_ptr as *mut c_void;
        }
    }

    #[cfg(not(test))]
    esp_log_write(
        2, // WARN
        b"syscall\0".as_ptr(),
        b"syscall_resolve: unknown symbol '%s'\0".as_ptr(),
        name,
    );
    std::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Tests
//
// Only pure-Rust functions are tested here: syscall_table_count(),
// syscall_table_get(), and the success path of syscall_resolve() (which
// returns before calling esp_log_write).
//
// syscall_table_init() and the failure path of syscall_resolve() both call
// esp_log_write which is not available on the host target, so they are skipped.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    // -----------------------------------------------------------------------
    // test_table_count_nonzero
    // Mirrors test_syscall_table.c: count must be > 0 after the table is built.
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_count_nonzero() {
        let count = syscall_table_count();
        assert!(count > 0, "syscall table must contain at least one entry");
    }

    // -----------------------------------------------------------------------
    // test_table_get_returns_non_null
    // Mirrors test_syscall_table.c: syscall_table_get() must not be NULL.
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_get_returns_non_null() {
        let ptr = syscall_table_get();
        assert!(!ptr.is_null(), "syscall_table_get() must return a non-null pointer");
    }

    // -----------------------------------------------------------------------
    // test_resolve_thistle_delay
    // Mirrors test_syscall_table.c: resolve a known symbol must return non-NULL.
    // Uses "thistle_delay" (host-safe wrapper using std::thread::sleep).
    // Note: "thistle_log" and "thistle_millis" are excluded from the test-mode
    // table because their bodies call esp_log_write / esp_timer_get_time which
    // are not available on aarch64-apple-darwin.
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_thistle_delay() {
        let name = b"thistle_delay\0";
        let ptr = unsafe { syscall_resolve(name.as_ptr() as *const c_char) };
        assert!(
            !ptr.is_null(),
            "syscall_resolve(\"thistle_delay\") must return a non-null address"
        );
    }

    // -----------------------------------------------------------------------
    // test_resolve_thistle_malloc
    // Mirrors test_syscall_table.c: resolve("thistle_malloc") must return non-NULL.
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_thistle_malloc() {
        let name = b"thistle_malloc\0";
        let ptr = unsafe { syscall_resolve(name.as_ptr() as *const c_char) };
        assert!(
            !ptr.is_null(),
            "syscall_resolve(\"thistle_malloc\") must return a non-null address"
        );
    }

    // -----------------------------------------------------------------------
    // test_resolve_thistle_crypto_sha256
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_thistle_crypto_sha256() {
        let name = b"thistle_crypto_sha256\0";
        let ptr = unsafe { syscall_resolve(name.as_ptr() as *const c_char) };
        assert!(
            !ptr.is_null(),
            "syscall_resolve(\"thistle_crypto_sha256\") must return a non-null address"
        );
    }

    #[test]
    fn test_resolve_thistle_crypto_random() {
        let name = b"thistle_crypto_random\0";
        let ptr = unsafe { syscall_resolve(name.as_ptr() as *const c_char) };
        assert!(
            !ptr.is_null(),
            "syscall_resolve(\"thistle_crypto_random\") must return a non-null address"
        );
    }

    // -----------------------------------------------------------------------
    // test_resolve_null_returns_null
    // syscall_resolve(NULL) must return NULL without crashing.
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_null_returns_null() {
        let ptr = unsafe { syscall_resolve(std::ptr::null()) };
        assert!(ptr.is_null(), "syscall_resolve(NULL) must return NULL");
    }

    // -----------------------------------------------------------------------
    // test_table_entries_have_non_null_names_and_funcs
    // All entries in the static table must have valid name and func_ptr.
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_entries_have_non_null_names_and_funcs() {
        let count = syscall_table_count();
        let base = syscall_table_get();

        for i in 0..count {
            let entry = unsafe { &*base.add(i) };
            assert!(
                !entry.name.is_null(),
                "entry[{}].name must not be null", i
            );
            assert!(
                !entry.func_ptr.is_null(),
                "entry[{}].func_ptr must not be null (name={})",
                i,
                unsafe { CStr::from_ptr(entry.name).to_str().unwrap_or("?") }
            );
        }
    }
}
