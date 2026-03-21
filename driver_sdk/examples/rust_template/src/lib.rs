// Example ThistleOS driver (Rust)
// This is a template for writing hardware drivers in Rust as standalone .drv.elf files.
//
// The driver is compiled as a cdylib and loaded by the kernel's ELF loader.
// All kernel functions are resolved from the syscall table at load time.

#![no_std]

use core::ffi::{c_char, c_int, c_void};

// ── Kernel functions (resolved from syscall table) ──────────────────

extern "C" {
    fn thistle_log(tag: *const c_char, fmt: *const c_char, ...);
    fn thistle_millis() -> u32;
    fn thistle_delay(ms: u32);
    fn thistle_malloc(size: u32) -> *mut c_void;
    fn thistle_free(ptr: *mut c_void);

    // Bus handles
    fn hal_bus_get_spi(index: c_int) -> *mut c_void;
    fn hal_bus_get_i2c(index: c_int) -> *mut c_void;

    // HAL registration
    fn hal_input_register(driver: *const c_void, config: *const c_void) -> c_int;
    fn hal_display_register(driver: *const c_void, config: *const c_void) -> c_int;
    fn hal_radio_register(driver: *const c_void, config: *const c_void) -> c_int;

    // I2C
    fn i2c_master_bus_add_device(
        bus: *mut c_void,
        config: *const c_void,
        handle: *mut *mut c_void,
    ) -> c_int;
    fn i2c_master_transmit(
        dev: *mut c_void,
        data: *const u8,
        len: u32,
        timeout: c_int,
    ) -> c_int;
    fn i2c_master_transmit_receive(
        dev: *mut c_void,
        tx: *const u8,
        tx_len: u32,
        rx: *mut u8,
        rx_len: u32,
        timeout: c_int,
    ) -> c_int;
}

// ── Driver entry point ──────────────────────────────────────────────

/// Entry point called by the kernel after loading the .drv.elf.
/// `config_json` contains driver-specific config from board.json.
#[no_mangle]
pub unsafe extern "C" fn driver_init(config_json: *const c_char) -> c_int {
    thistle_log(
        b"example_drv\0".as_ptr() as *const c_char,
        b"Rust driver initializing\0".as_ptr() as *const c_char,
    );

    // Parse config_json for your pin assignments, I2C addresses, etc.
    // let config = if !config_json.is_null() {
    //     CStr::from_ptr(config_json).to_str().unwrap_or("{}")
    // } else {
    //     "{}"
    // };

    // Get bus handles from the kernel
    // let i2c = hal_bus_get_i2c(0);

    // Register your HAL vtable
    // hal_input_register(&MY_DRIVER as *const _ as *const c_void, core::ptr::null());

    thistle_log(
        b"example_drv\0".as_ptr() as *const c_char,
        b"Rust driver ready\0".as_ptr() as *const c_char,
    );

    0 // success
}

// Required for no_std
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
