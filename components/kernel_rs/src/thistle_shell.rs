// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Kernel — thistle_shell module
//
// Command interpreter for Terminal. Provides filesystem, system, and network
// commands via a single FFI entry point. Each command is a function that takes
// parsed args and a print callback.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type PrintFn = extern "C" fn(*const c_char, *mut c_void);

struct ShellCmd {
    name: &'static str,
    help: &'static str,
    func: fn(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32,
}

// ---------------------------------------------------------------------------
// SD card root — platform-dependent
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
const DEFAULT_ROOT: &str = "/sdcard";
#[cfg(not(target_os = "espidf"))]
const DEFAULT_ROOT: &str = "/tmp/thistle_sdcard";

// ---------------------------------------------------------------------------
// Current working directory
// ---------------------------------------------------------------------------

static CWD: Mutex<[u8; 256]> = Mutex::new([0u8; 256]);

fn get_cwd() -> String {
    if let Ok(cwd) = CWD.lock() {
        let len = cwd.iter().position(|&b| b == 0).unwrap_or(0);
        if len == 0 {
            return DEFAULT_ROOT.to_string();
        }
        String::from_utf8_lossy(&cwd[..len]).to_string()
    } else {
        DEFAULT_ROOT.to_string()
    }
}

fn set_cwd(path: &str) {
    if let Ok(mut cwd) = CWD.lock() {
        let bytes = path.as_bytes();
        let len = bytes.len().min(cwd.len() - 1);
        cwd[..len].copy_from_slice(&bytes[..len]);
        cwd[len] = 0;
        // Zero the rest
        for b in &mut cwd[len + 1..] {
            *b = 0;
        }
    }
}

fn resolve_path(arg: &str) -> String {
    if arg.starts_with('/') {
        arg.to_string()
    } else {
        let cwd = get_cwd();
        if cwd.ends_with('/') {
            format!("{}{}", cwd, arg)
        } else {
            format!("{}/{}", cwd, arg)
        }
    }
}

// ---------------------------------------------------------------------------
// Print helpers
// ---------------------------------------------------------------------------

fn shell_print(cb: PrintFn, ctx: *mut c_void, msg: &str) {
    let mut buf = [0u8; 512];
    let len = msg.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&msg.as_bytes()[..len]);
    buf[len] = 0;
    cb(buf.as_ptr() as *const c_char, ctx);
}

fn shell_printf(cb: PrintFn, ctx: *mut c_void, args: std::fmt::Arguments) {
    use std::io::Write;
    let mut buf = [0u8; 512];
    let pos = {
        let mut cursor = std::io::Cursor::new(&mut buf[..]);
        let _ = write!(cursor, "{}", args);
        cursor.position() as usize
    };
    let idx = pos.min(buf.len() - 1);
    buf[idx] = 0;
    cb(buf.as_ptr() as *const c_char, ctx);
}

macro_rules! sprint {
    ($cb:expr, $ctx:expr, $($arg:tt)*) => {
        shell_printf($cb, $ctx, format_args!($($arg)*))
    };
}

// ---------------------------------------------------------------------------
// Extern C FFI for system/HAL calls
// ---------------------------------------------------------------------------

extern "C" {
    fn esp_get_free_heap_size() -> u32;
    fn heap_caps_get_free_size(caps: u32) -> usize;
    fn kernel_uptime_ms() -> u32;
    fn esp_restart();
    fn app_manager_get_count() -> i32;
    fn app_manager_launch(id: *const c_char) -> i32;
    fn wifi_manager_get_state() -> i32;
    fn wifi_manager_get_ip() -> *const c_char;
    fn wifi_manager_get_rssi() -> i32;
    fn wifi_manager_scan_start() -> i32;
    fn wifi_manager_scan_get_count() -> i32;
    fn ble_manager_get_state() -> i32;
    fn ble_manager_get_peer_name() -> *const c_char;
    fn hal_storage_get_total_bytes() -> u64;
    fn hal_storage_get_free_bytes() -> u64;
    fn driver_loader_get_count() -> i32;
    fn hal_get_registry() -> *const crate::hal_registry::HalRegistry;
}

// ---------------------------------------------------------------------------
// FFI implementations for functions declared in the extern block above.
// These provide real implementations that the firmware linker needs.
// ---------------------------------------------------------------------------

/// Read total bytes from the first mounted HAL storage driver.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn hal_storage_get_total_bytes() -> u64 {
    let reg = unsafe { hal_get_registry() };
    if reg.is_null() { return 0; }
    let r = unsafe { &*reg };
    for i in 0..r.storage_count as usize {
        if !r.storage[i].is_null() {
            let drv = unsafe { &*r.storage[i] };
            if let Some(f) = drv.get_total_bytes {
                return unsafe { f() };
            }
        }
    }
    0
}

/// Read free bytes from the first mounted HAL storage driver.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn hal_storage_get_free_bytes() -> u64 {
    let reg = unsafe { hal_get_registry() };
    if reg.is_null() { return 0; }
    let r = unsafe { &*reg };
    for i in 0..r.storage_count as usize {
        if !r.storage[i].is_null() {
            let drv = unsafe { &*r.storage[i] };
            if let Some(f) = drv.get_free_bytes {
                return unsafe { f() };
            }
        }
    }
    0
}

/// Stub for wifi scan start — placeholder until async scan is implemented.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn wifi_manager_scan_start() -> i32 { -1 }

/// Stub for wifi scan count — placeholder until async scan is implemented.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn wifi_manager_scan_get_count() -> i32 { 0 }

// Test stubs — provide linkable C symbols for functions not already defined
// in the Rust crate. Functions like wifi_manager_get_state, app_manager_launch,
// kernel_uptime_ms, etc. are already implemented in their respective modules.
#[cfg(test)]
mod test_stubs {
    use std::os::raw::c_char;

    #[no_mangle] pub extern "C" fn esp_get_free_heap_size() -> u32 { 65536 }
    #[no_mangle] pub extern "C" fn heap_caps_get_free_size(_caps: u32) -> usize { 0 }
    #[no_mangle] pub extern "C" fn esp_restart() {}
    /* app_manager_get_count is now in app_manager.rs — no test stub needed */
    #[no_mangle] pub extern "C" fn wifi_manager_scan_start() -> i32 { 0 }
    #[no_mangle] pub extern "C" fn wifi_manager_scan_get_count() -> i32 { 0 }
    #[no_mangle] pub extern "C" fn hal_storage_get_total_bytes() -> u64 { 10 * 1024 * 1024 }
    #[no_mangle] pub extern "C" fn hal_storage_get_free_bytes() -> u64 { 5 * 1024 * 1024 }
    /* driver_loader_get_count is in driver_loader.rs — no test stub needed */
    // Transitive dependency: kernel_uptime_ms (kernel_boot.rs) calls esp_timer_get_time
    #[no_mangle] pub extern "C" fn esp_timer_get_time() -> i64 { 0 }
}

// MALLOC_CAP_SPIRAM
const MALLOC_CAP_SPIRAM: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// Filesystem commands
// ---------------------------------------------------------------------------

fn cmd_ls(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    let path = if args.len() > 1 {
        resolve_path(args[1])
    } else {
        get_cwd()
    };
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                let (size, is_dir) = match entry.metadata() {
                    Ok(m) => (m.len(), m.is_dir()),
                    Err(_) => (0, false),
                };
                if is_dir {
                    sprint!(print, ctx, "  {}/", name_str);
                } else {
                    sprint!(print, ctx, "  {:>8} {}", size, name_str);
                }
            }
            0
        }
        Err(e) => {
            sprint!(print, ctx, "ls: {}: {}", path, e);
            1
        }
    }
}

fn cmd_cd(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 2 {
        set_cwd(DEFAULT_ROOT);
        return 0;
    }
    let path = resolve_path(args[1]);
    match std::fs::metadata(&path) {
        Ok(m) if m.is_dir() => {
            set_cwd(&path);
            0
        }
        Ok(_) => {
            sprint!(print, ctx, "cd: {}: Not a directory", path);
            1
        }
        Err(e) => {
            sprint!(print, ctx, "cd: {}: {}", path, e);
            1
        }
    }
}

fn cmd_pwd(_args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    shell_print(print, ctx, &get_cwd());
    0
}

fn cmd_cat(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 2 {
        shell_print(print, ctx, "Usage: cat <file>");
        return 1;
    }
    let path = resolve_path(args[1]);
    match std::fs::read(&path) {
        Ok(data) => {
            // Limit to 4KB to avoid flooding
            let limit = data.len().min(4096);
            let text = String::from_utf8_lossy(&data[..limit]);
            for line in text.lines() {
                shell_print(print, ctx, line);
            }
            if data.len() > 4096 {
                sprint!(print, ctx, "... truncated ({} bytes total)", data.len());
            }
            0
        }
        Err(e) => {
            sprint!(print, ctx, "cat: {}: {}", path, e);
            1
        }
    }
}

fn cmd_mkdir(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 2 {
        shell_print(print, ctx, "Usage: mkdir <path>");
        return 1;
    }
    let path = resolve_path(args[1]);
    match std::fs::create_dir(&path) {
        Ok(()) => 0,
        Err(e) => {
            sprint!(print, ctx, "mkdir: {}: {}", path, e);
            1
        }
    }
}

fn cmd_rm(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 2 {
        shell_print(print, ctx, "Usage: rm <file>");
        return 1;
    }
    let path = resolve_path(args[1]);
    match std::fs::remove_file(&path) {
        Ok(()) => 0,
        Err(e) => {
            sprint!(print, ctx, "rm: {}: {}", path, e);
            1
        }
    }
}

fn cmd_rmdir(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 2 {
        shell_print(print, ctx, "Usage: rmdir <path>");
        return 1;
    }
    let path = resolve_path(args[1]);
    match std::fs::remove_dir(&path) {
        Ok(()) => 0,
        Err(e) => {
            sprint!(print, ctx, "rmdir: {}: {}", path, e);
            1
        }
    }
}

fn cmd_cp(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 3 {
        shell_print(print, ctx, "Usage: cp <src> <dst>");
        return 1;
    }
    let src = resolve_path(args[1]);
    let dst = resolve_path(args[2]);
    match std::fs::copy(&src, &dst) {
        Ok(_) => 0,
        Err(e) => {
            sprint!(print, ctx, "cp: {}", e);
            1
        }
    }
}

fn cmd_mv(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 3 {
        shell_print(print, ctx, "Usage: mv <src> <dst>");
        return 1;
    }
    let src = resolve_path(args[1]);
    let dst = resolve_path(args[2]);
    match std::fs::rename(&src, &dst) {
        Ok(()) => 0,
        Err(e) => {
            sprint!(print, ctx, "mv: {}", e);
            1
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn cmd_df(_args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    let total = unsafe { hal_storage_get_total_bytes() };
    let free = unsafe { hal_storage_get_free_bytes() };
    let used = total.saturating_sub(free);
    sprint!(print, ctx, "Total: {}", format_size(total));
    sprint!(print, ctx, "Used:  {}", format_size(used));
    sprint!(print, ctx, "Free:  {}", format_size(free));
    0
}

fn cmd_hexdump(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 2 {
        shell_print(print, ctx, "Usage: hexdump <file> [limit]");
        return 1;
    }
    let path = resolve_path(args[1]);
    let limit: usize = if args.len() > 2 {
        args[2].parse().unwrap_or(256)
    } else {
        256
    };
    match std::fs::read(&path) {
        Ok(data) => {
            let len = data.len().min(limit);
            let mut offset = 0usize;
            while offset < len {
                let end = (offset + 16).min(len);
                let chunk = &data[offset..end];

                // Hex portion
                let mut hex = String::with_capacity(48);
                for (i, b) in chunk.iter().enumerate() {
                    if i > 0 {
                        hex.push(' ');
                    }
                    hex.push_str(&format!("{:02x}", b));
                }

                // ASCII portion
                let ascii: String = chunk
                    .iter()
                    .map(|&b| if (0x20..=0x7e).contains(&b) { b as char } else { '.' })
                    .collect();

                sprint!(print, ctx, "{:08x}  {:<48} |{}|", offset, hex, ascii);
                offset += 16;
            }
            if data.len() > limit {
                sprint!(
                    print,
                    ctx,
                    "... showing {} of {} bytes",
                    limit,
                    data.len()
                );
            }
            0
        }
        Err(e) => {
            sprint!(print, ctx, "hexdump: {}: {}", path, e);
            1
        }
    }
}

// ---------------------------------------------------------------------------
// System commands
// ---------------------------------------------------------------------------

fn cmd_heap(_args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    let free_heap = unsafe { esp_get_free_heap_size() };
    let free_psram = unsafe { heap_caps_get_free_size(MALLOC_CAP_SPIRAM) };
    sprint!(print, ctx, "Free heap:  {} bytes", free_heap);
    sprint!(print, ctx, "Free PSRAM: {} bytes", free_psram);
    0
}

fn cmd_uptime(_args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    let ms = unsafe { kernel_uptime_ms() };
    let secs = ms / 1000;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    sprint!(print, ctx, "{}h {}m {}s", h, m, s);
    0
}

fn cmd_version(_args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    sprint!(print, ctx, "ThistleOS v{}", crate::version::VERSION_STRING);
    sprint!(print, ctx, "  Kernel:   Rust (thistle-kernel)");
    sprint!(print, ctx, "  Recovery: Rust (thistle-recovery)");

    let reg = unsafe { hal_get_registry() };
    if !reg.is_null() {
        let board = unsafe { (*reg).board_name };
        if !board.is_null() {
            let name = unsafe { std::ffi::CStr::from_ptr(board) }
                .to_str()
                .unwrap_or("unknown");
            sprint!(print, ctx, "  Board:    {}", name);
        }
    }

    let drv_count = unsafe { driver_loader_get_count() };
    sprint!(print, ctx, "  Drivers:  {} loaded", drv_count);

    let app_count = unsafe { app_manager_get_count() };
    sprint!(print, ctx, "  Apps:     {} registered", app_count);
    0
}

fn cmd_reboot(_args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    shell_print(print, ctx, "Rebooting...");
    unsafe {
        esp_restart();
    }
    0
}

fn cmd_apps(_args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    let count = unsafe { app_manager_get_count() };
    sprint!(print, ctx, "{} app(s) registered. Use the launcher to browse.", count);
    0
}

fn cmd_launch(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() < 2 {
        shell_print(print, ctx, "Usage: launch <app_id>");
        return 1;
    }
    let mut id_buf = [0u8; 128];
    let id = args[1].as_bytes();
    let len = id.len().min(id_buf.len() - 1);
    id_buf[..len].copy_from_slice(&id[..len]);
    id_buf[len] = 0;
    let ret = unsafe { app_manager_launch(id_buf.as_ptr() as *const c_char) };
    if ret == 0 {
        sprint!(print, ctx, "Launched: {}", args[1]);
        0
    } else {
        sprint!(print, ctx, "launch: failed (error {})", ret);
        1
    }
}

/// Returns -2 as a sentinel for Terminal to clear the screen.
fn cmd_clear(_args: &[&str], _print: PrintFn, _ctx: *mut c_void) -> i32 {
    -2
}

// ---------------------------------------------------------------------------
// Network commands
// ---------------------------------------------------------------------------

fn cmd_wifi(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    let subcmd = if args.len() > 1 { args[1] } else { "status" };
    match subcmd {
        "status" => {
            let state = unsafe { wifi_manager_get_state() };
            let state_str = match state {
                0 => "Disconnected",
                1 => "Connecting",
                2 => "Connected",
                3 => "Failed",
                _ => "Unknown",
            };
            sprint!(print, ctx, "WiFi: {}", state_str);
            if state == 2 {
                let ip_ptr = unsafe { wifi_manager_get_ip() };
                if !ip_ptr.is_null() {
                    let ip = unsafe { CStr::from_ptr(ip_ptr) }
                        .to_str()
                        .unwrap_or("?");
                    sprint!(print, ctx, "IP:   {}", ip);
                }
                let rssi = unsafe { wifi_manager_get_rssi() };
                sprint!(print, ctx, "RSSI: {} dBm", rssi);
            }
            0
        }
        "scan" => {
            let ret = unsafe { wifi_manager_scan_start() };
            if ret != 0 {
                sprint!(print, ctx, "wifi scan: failed (error {})", ret);
                return 1;
            }
            let count = unsafe { wifi_manager_scan_get_count() };
            sprint!(print, ctx, "Found {} network(s)", count);
            0
        }
        _ => {
            shell_print(print, ctx, "Usage: wifi [status|scan]");
            1
        }
    }
}

fn cmd_ble(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    let subcmd = if args.len() > 1 { args[1] } else { "status" };
    match subcmd {
        "status" => {
            let state = unsafe { ble_manager_get_state() };
            let state_str = match state {
                0 => "Off",
                1 => "Advertising",
                2 => "Connected",
                _ => "Unknown",
            };
            sprint!(print, ctx, "BLE: {}", state_str);
            if state == 2 {
                let name_ptr = unsafe { ble_manager_get_peer_name() };
                if !name_ptr.is_null() {
                    let name = unsafe { CStr::from_ptr(name_ptr) }
                        .to_str()
                        .unwrap_or("?");
                    sprint!(print, ctx, "Peer: {}", name);
                }
            }
            0
        }
        _ => {
            shell_print(print, ctx, "Usage: ble [status]");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Utility commands
// ---------------------------------------------------------------------------

fn cmd_echo(args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    if args.len() > 1 {
        let text = args[1..].join(" ");
        shell_print(print, ctx, &text);
    }
    0
}

fn cmd_help(_args: &[&str], print: PrintFn, ctx: *mut c_void) -> i32 {
    shell_print(print, ctx, "ThistleOS Shell Commands:");
    shell_print(print, ctx, "");
    for cmd in COMMANDS {
        sprint!(print, ctx, "  {:10} {}", cmd.name, cmd.help);
    }
    0
}

// ---------------------------------------------------------------------------
// Command table
// ---------------------------------------------------------------------------

static COMMANDS: &[ShellCmd] = &[
    ShellCmd { name: "ls",      help: "List directory contents",    func: cmd_ls },
    ShellCmd { name: "cd",      help: "Change directory",           func: cmd_cd },
    ShellCmd { name: "pwd",     help: "Print working directory",    func: cmd_pwd },
    ShellCmd { name: "cat",     help: "Print file contents",        func: cmd_cat },
    ShellCmd { name: "mkdir",   help: "Create directory",           func: cmd_mkdir },
    ShellCmd { name: "rm",      help: "Remove file",                func: cmd_rm },
    ShellCmd { name: "rmdir",   help: "Remove directory",           func: cmd_rmdir },
    ShellCmd { name: "cp",      help: "Copy file",                  func: cmd_cp },
    ShellCmd { name: "mv",      help: "Move/rename file",           func: cmd_mv },
    ShellCmd { name: "df",      help: "Disk free space",            func: cmd_df },
    ShellCmd { name: "hexdump", help: "Hex dump file [limit]",      func: cmd_hexdump },
    ShellCmd { name: "heap",    help: "Free memory",                func: cmd_heap },
    ShellCmd { name: "uptime",  help: "Kernel uptime",              func: cmd_uptime },
    ShellCmd { name: "version", help: "OS version",                 func: cmd_version },
    ShellCmd { name: "reboot",  help: "Restart device",             func: cmd_reboot },
    ShellCmd { name: "apps",    help: "List registered apps",       func: cmd_apps },
    ShellCmd { name: "launch",  help: "Launch app by ID",           func: cmd_launch },
    ShellCmd { name: "clear",   help: "Clear terminal",             func: cmd_clear },
    ShellCmd { name: "wifi",    help: "WiFi status/scan",           func: cmd_wifi },
    ShellCmd { name: "ble",     help: "BLE status",                 func: cmd_ble },
    ShellCmd { name: "help",    help: "Show this list",             func: cmd_help },
    ShellCmd { name: "echo",    help: "Print text",                 func: cmd_echo },
];

// ---------------------------------------------------------------------------
// FFI entry point
// ---------------------------------------------------------------------------

/// Execute a shell command line. Called from Terminal (C).
/// `input` — null-terminated command string
/// `output_cb` — called for each line of output
/// `user_data` — passed through to output_cb
/// Returns 0 on success, non-zero on error. -2 means "clear terminal".
#[no_mangle]
pub unsafe extern "C" fn thistle_shell_exec(
    input: *const c_char,
    output_cb: extern "C" fn(*const c_char, *mut c_void),
    user_data: *mut c_void,
) -> i32 {
    if input.is_null() {
        return -1;
    }
    let line = match CStr::from_ptr(input).to_str() {
        Ok(s) => s.trim(),
        Err(_) => return -1,
    };
    if line.is_empty() {
        return 0;
    }

    // Simple arg splitting (split on whitespace)
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return 0;
    }

    let cmd_name = parts[0];

    for cmd in COMMANDS {
        if cmd.name == cmd_name {
            return (cmd.func)(&parts, output_cb, user_data);
        }
    }

    sprint!(output_cb, user_data, "Unknown command: {}", cmd_name);
    sprint!(output_cb, user_data, "Type 'help' for available commands.");
    1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static TEST_OUTPUT: StdMutex<Vec<String>> = StdMutex::new(Vec::new());

    extern "C" fn test_print(msg: *const c_char, _ctx: *mut c_void) {
        let s = unsafe { CStr::from_ptr(msg).to_str().unwrap_or("").to_string() };
        TEST_OUTPUT.lock().unwrap().push(s);
    }

    fn run_cmd(cmd: &str) -> (i32, Vec<String>) {
        TEST_OUTPUT.lock().unwrap().clear();
        // Reset CWD to default for test isolation
        set_cwd(DEFAULT_ROOT);
        let c_cmd = format!("{}\0", cmd);
        let ret = unsafe {
            thistle_shell_exec(
                c_cmd.as_ptr() as *const c_char,
                test_print,
                std::ptr::null_mut(),
            )
        };
        let output = TEST_OUTPUT.lock().unwrap().clone();
        (ret, output)
    }

    #[test]
    fn test_resolve_path_absolute() {
        assert_eq!(resolve_path("/foo/bar"), "/foo/bar");
    }

    #[test]
    fn test_resolve_path_relative() {
        set_cwd("/tmp/thistle_sdcard");
        assert_eq!(
            resolve_path("config"),
            "/tmp/thistle_sdcard/config"
        );
    }

    #[test]
    fn test_resolve_path_relative_trailing_slash() {
        set_cwd("/tmp/thistle_sdcard/");
        assert_eq!(
            resolve_path("config"),
            "/tmp/thistle_sdcard/config"
        );
    }

    #[test]
    fn test_unknown_command() {
        let (ret, output) = run_cmd("nosuchcmd");
        assert_eq!(ret, 1);
        assert!(output.iter().any(|s| s.contains("Unknown command")));
    }

    #[test]
    fn test_empty_input() {
        let (ret, output) = run_cmd("");
        assert_eq!(ret, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn test_help_produces_output() {
        let (ret, output) = run_cmd("help");
        assert_eq!(ret, 0);
        assert!(output.len() > 5); // Should have header + many commands
        assert!(output.iter().any(|s| s.contains("ls")));
        assert!(output.iter().any(|s| s.contains("help")));
    }

    #[test]
    fn test_echo() {
        let (ret, output) = run_cmd("echo hello world");
        assert_eq!(ret, 0);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0], "hello world");
    }

    #[test]
    fn test_echo_empty() {
        let (ret, output) = run_cmd("echo");
        assert_eq!(ret, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn test_pwd() {
        let (ret, output) = run_cmd("pwd");
        assert_eq!(ret, 0);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0], DEFAULT_ROOT);
    }

    #[test]
    fn test_clear_returns_sentinel() {
        let (ret, _output) = run_cmd("clear");
        assert_eq!(ret, -2);
    }

    #[test]
    fn test_null_input() {
        let ret = unsafe {
            thistle_shell_exec(
                std::ptr::null(),
                test_print,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_ls_nonexistent() {
        let (ret, output) = run_cmd("ls /nonexistent_path_12345");
        assert_eq!(ret, 1);
        assert!(output.iter().any(|s| s.contains("ls:")));
    }

    #[test]
    fn test_cat_missing_arg() {
        let (ret, output) = run_cmd("cat");
        assert_eq!(ret, 1);
        assert!(output.iter().any(|s| s.contains("Usage")));
    }

    #[test]
    fn test_cat_nonexistent() {
        let (ret, output) = run_cmd("cat /nonexistent_file_12345");
        assert_eq!(ret, 1);
        assert!(output.iter().any(|s| s.contains("cat:")));
    }

    #[test]
    fn test_cd_nonexistent() {
        let (ret, output) = run_cmd("cd /nonexistent_dir_12345");
        assert_eq!(ret, 1);
        assert!(output.iter().any(|s| s.contains("cd:")));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
    }
}
