// SPDX-License-Identifier: BSD-3-Clause
// Display server — toolkit-agnostic compositor for swappable window managers

use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

const ESP_OK: i32 = 0;
const ESP_ERR_INVALID_ARG: i32 = 0x102;
const ESP_ERR_INVALID_STATE: i32 = 0x103;
const ESP_ERR_NOT_FOUND: i32 = 0x105;
const ESP_ERR_NOT_SUPPORTED: i32 = 0x106;
const ESP_ERR_NO_MEM: i32 = 0x101;

const MAX_SURFACES: usize = 8;

// Display types matching C enum
const DISPLAY_TYPE_LCD: u32 = 0;
const DISPLAY_TYPE_EPAPER: u32 = 1;

// Surface roles for Z-ordering
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfaceRole {
    Background = 0,
    StatusBar = 1,
    AppContent = 2,
    Overlay = 3,
    Dock = 4,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SurfaceInfo {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub role: u32,
    pub visible: bool,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HalArea {
    pub x1: u16,
    pub y1: u16,
    pub x2: u16,
    pub y2: u16,
}

// Input event matching C struct
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HalInputEvent {
    pub event_type: u32,
    pub timestamp: u32,
    pub data: [u16; 2], // key.keycode or touch.x/touch.y
}

// WM vtable matching C display_server_wm_t
#[repr(C)]
pub struct WmVtable {
    pub init: Option<unsafe extern "C" fn() -> i32>,
    pub deinit: Option<unsafe extern "C" fn()>,
    pub render: Option<unsafe extern "C" fn()>,
    pub on_theme_changed: Option<unsafe extern "C" fn(*const c_char)>,
    pub on_app_launched: Option<unsafe extern "C" fn(*const c_char, u32)>,
    pub on_app_stopped: Option<unsafe extern "C" fn(*const c_char)>,
    pub on_app_switched: Option<unsafe extern "C" fn(*const c_char)>,
    pub on_input: Option<unsafe extern "C" fn(*const HalInputEvent) -> bool>,
    pub name: *const c_char,
    pub version: *const c_char,
}

// Make WmVtable sendable (it holds raw pointers but they're static)
unsafe impl Send for WmVtable {}
unsafe impl Sync for WmVtable {}

type InputCallback = Option<unsafe extern "C" fn(*const HalInputEvent, *mut c_void)>;

struct Surface {
    info: SurfaceInfo,
    buffer: Vec<u8>,
    dirty: bool,
    dirty_area: HalArea,
    input_cb: InputCallback,
    input_user_data: *mut c_void,
    allocated: bool,
}

// Make Surface sendable
unsafe impl Send for Surface {}

impl Surface {
    const fn empty() -> Self {
        Surface {
            info: SurfaceInfo { x: 0, y: 0, width: 0, height: 0, role: 0, visible: false },
            buffer: Vec::new(),
            dirty: false,
            dirty_area: HalArea { x1: 0, y1: 0, x2: 0, y2: 0 },
            input_cb: None,
            input_user_data: std::ptr::null_mut(),
            allocated: false,
        }
    }
}

struct DisplayServer {
    surfaces: Vec<Surface>,
    wm: Option<*const WmVtable>,
    initialized: bool,
    next_id: u32,
}

unsafe impl Send for DisplayServer {}

impl DisplayServer {
    fn new() -> Self {
        let mut surfaces = Vec::with_capacity(MAX_SURFACES);
        for _ in 0..MAX_SURFACES {
            surfaces.push(Surface::empty());
        }
        DisplayServer {
            surfaces,
            wm: None,
            initialized: false,
            next_id: 1,
        }
    }
}

static DS: Mutex<Option<DisplayServer>> = Mutex::new(None);

// HAL registry FFI
extern "C" {
    fn hal_get_registry() -> *const c_void;
}

fn get_display_width() -> u16 {
    // Read from HAL registry — simplified, return default for now
    320
}

fn get_display_height() -> u16 {
    240
}

// FFI exports — same names as C functions

#[no_mangle]
pub extern "C" fn display_server_init() -> i32 {
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return ESP_ERR_INVALID_STATE,
    };
    *lock = Some(DisplayServer::new());
    if let Some(ds) = lock.as_mut() {
        ds.initialized = true;
    }
    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn display_server_register_wm(wm: *const WmVtable) -> i32 {
    if wm.is_null() { return ESP_ERR_INVALID_ARG; }
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return ESP_ERR_INVALID_STATE,
    };
    let ds = match lock.as_mut() {
        Some(ds) => ds,
        None => return ESP_ERR_INVALID_STATE,
    };

    // Deinit previous WM
    if let Some(old_wm) = ds.wm {
        if let Some(deinit) = (*old_wm).deinit {
            deinit();
        }
    }

    ds.wm = Some(wm);

    // Init new WM
    if let Some(init) = (*wm).init {
        let ret = init();
        if ret != ESP_OK {
            ds.wm = None;
            return ret;
        }
    }

    ESP_OK
}

#[no_mangle]
pub unsafe extern "C" fn display_server_load_wm(_path: *const c_char) -> i32 {
    ESP_ERR_NOT_SUPPORTED // TODO: runtime WM loading
}

#[no_mangle]
pub extern "C" fn display_server_get_wm_name() -> *const c_char {
    let lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return std::ptr::null(),
    };
    match lock.as_ref().and_then(|ds| ds.wm) {
        Some(wm) => unsafe { (*wm).name },
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn display_server_create_surface(info: *const SurfaceInfo) -> u32 {
    if info.is_null() { return 0; }
    let info = *info;
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return 0,
    };
    let ds = match lock.as_mut() {
        Some(ds) => ds,
        None => return 0,
    };

    for i in 0..MAX_SURFACES {
        if !ds.surfaces[i].allocated {
            let buf_size = (info.width as usize) * (info.height as usize) * 2;
            ds.surfaces[i] = Surface {
                info,
                buffer: vec![0u8; buf_size],
                dirty: false,
                dirty_area: HalArea { x1: 0, y1: 0, x2: 0, y2: 0 },
                input_cb: None,
                input_user_data: std::ptr::null_mut(),
                allocated: true,
            };
            let id = ds.next_id;
            ds.next_id += 1;
            return id;
        }
    }
    0 // no free slots
}

#[no_mangle]
pub unsafe extern "C" fn display_server_destroy_surface(id: u32) {
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return,
    };
    if let Some(ds) = lock.as_mut() {
        if let Some(s) = ds.surfaces.get_mut((id.wrapping_sub(1)) as usize) {
            *s = Surface::empty();
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn display_server_get_buffer(id: u32) -> *mut u8 {
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    if let Some(ds) = lock.as_mut() {
        if let Some(s) = ds.surfaces.get_mut((id.wrapping_sub(1)) as usize) {
            if s.allocated {
                return s.buffer.as_mut_ptr();
            }
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn display_server_get_info(id: u32) -> *const SurfaceInfo {
    let lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return std::ptr::null(),
    };
    if let Some(ds) = lock.as_ref() {
        if let Some(s) = ds.surfaces.get((id.wrapping_sub(1)) as usize) {
            if s.allocated {
                return &s.info as *const SurfaceInfo;
            }
        }
    }
    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn display_server_mark_dirty(id: u32, area: *const HalArea) {
    if area.is_null() { return; }
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return,
    };
    if let Some(ds) = lock.as_mut() {
        if let Some(s) = ds.surfaces.get_mut((id.wrapping_sub(1)) as usize) {
            if !s.dirty {
                s.dirty_area = *area;
                s.dirty = true;
            } else {
                if (*area).x1 < s.dirty_area.x1 { s.dirty_area.x1 = (*area).x1; }
                if (*area).y1 < s.dirty_area.y1 { s.dirty_area.y1 = (*area).y1; }
                if (*area).x2 > s.dirty_area.x2 { s.dirty_area.x2 = (*area).x2; }
                if (*area).y2 > s.dirty_area.y2 { s.dirty_area.y2 = (*area).y2; }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn display_server_mark_dirty_full(id: u32) {
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return,
    };
    if let Some(ds) = lock.as_mut() {
        if let Some(s) = ds.surfaces.get_mut((id.wrapping_sub(1)) as usize) {
            s.dirty = true;
            s.dirty_area = HalArea {
                x1: 0, y1: 0,
                x2: s.info.width.saturating_sub(1),
                y2: s.info.height.saturating_sub(1),
            };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn display_server_set_visible(id: u32, visible: bool) {
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return,
    };
    if let Some(ds) = lock.as_mut() {
        if let Some(s) = ds.surfaces.get_mut((id.wrapping_sub(1)) as usize) {
            s.info.visible = visible;
        }
    }
}

#[no_mangle]
pub extern "C" fn display_server_composite() -> i32 {
    // Compositing is handled by the WM via LVGL for now
    ESP_OK
}

#[no_mangle]
pub extern "C" fn display_server_get_width() -> u16 { get_display_width() }

#[no_mangle]
pub extern "C" fn display_server_get_height() -> u16 { get_display_height() }

#[no_mangle]
pub extern "C" fn display_server_get_display_type() -> u32 { DISPLAY_TYPE_LCD }

#[no_mangle]
pub unsafe extern "C" fn display_server_surface_input_cb(
    id: u32,
    cb: unsafe extern "C" fn(*const HalInputEvent, *mut c_void),
    user_data: *mut c_void,
) -> i32 {
    let mut lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return ESP_ERR_INVALID_STATE,
    };
    if let Some(ds) = lock.as_mut() {
        if let Some(s) = ds.surfaces.get_mut((id.wrapping_sub(1)) as usize) {
            s.input_cb = Some(cb);
            s.input_user_data = user_data;
            return ESP_OK;
        }
    }
    ESP_ERR_NOT_FOUND
}

#[no_mangle]
pub extern "C" fn display_server_tick() {
    let lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return,
    };
    if let Some(ds) = lock.as_ref() {
        if let Some(wm) = ds.wm {
            unsafe {
                if let Some(render) = (*wm).render {
                    render();
                }
            }
        }
    }
    // drop lock before composite
    drop(lock);
    display_server_composite();
}
