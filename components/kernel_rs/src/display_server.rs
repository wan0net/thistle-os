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

/// Return the raw WM vtable pointer so C widget_shims.c can call widget functions.
#[no_mangle]
pub extern "C" fn display_server_get_active_wm() -> *const c_void {
    let lock = match DS.lock() {
        Ok(l) => l,
        Err(_) => return std::ptr::null(),
    };
    match lock.as_ref().and_then(|ds| ds.wm) {
        Some(wm) => wm as *const _ as *const c_void,
        None => std::ptr::null(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test surface info
    fn test_surface(w: u16, h: u16, role: SurfaceRole) -> SurfaceInfo {
        SurfaceInfo { x: 0, y: 0, width: w, height: h, role: role as u32, visible: true }
    }

    #[test]
    fn test_init_creates_server() {
        display_server_init();
        let lock = DS.lock().unwrap();
        assert!(lock.is_some(), "DS should be initialized");
        let ds = lock.as_ref().unwrap();
        assert!(ds.initialized, "DisplayServer should have initialized flag set");
        assert_eq!(ds.surfaces.len(), MAX_SURFACES, "Should have MAX_SURFACES empty slots");
        assert_eq!(ds.next_id, 1, "next_id should start at 1");
    }

    #[test]
    fn test_create_surface_returns_nonzero_id() {
        display_server_init();
        let info = test_surface(100, 100, SurfaceRole::AppContent);
        let id = unsafe { display_server_create_surface(&info) };
        assert_ne!(id, 0, "create_surface should return non-zero ID");
        assert_eq!(id, 1, "first surface should have ID 1");
    }

    #[test]
    fn test_create_surface_allocates_buffer() {
        display_server_init();
        let info = test_surface(50, 40, SurfaceRole::AppContent);
        let id = unsafe { display_server_create_surface(&info) };
        assert_ne!(id, 0);

        let lock = DS.lock().unwrap();
        let ds = lock.as_ref().unwrap();
        let surface = &ds.surfaces[0];
        let expected_size = 50 * 40 * 2; // width * height * 2 bytes (16-bit color)
        assert_eq!(surface.buffer.len(), expected_size, "buffer should be width*height*2 bytes");
        assert!(surface.allocated, "surface should be marked as allocated");
    }

    #[test]
    fn test_destroy_surface_deallocates() {
        display_server_init();
        let info = test_surface(100, 100, SurfaceRole::AppContent);
        let id = unsafe { display_server_create_surface(&info) };
        assert_ne!(id, 0);

        {
            let lock = DS.lock().unwrap();
            assert!(lock.as_ref().unwrap().surfaces[0].allocated);
        }

        unsafe { display_server_destroy_surface(id); }

        let lock = DS.lock().unwrap();
        let surface = &lock.as_ref().unwrap().surfaces[0];
        assert!(!surface.allocated, "surface should be deallocated");
        assert_eq!(surface.buffer.len(), 0, "buffer should be empty");
    }

    #[test]
    fn test_max_surfaces_limit() {
        display_server_init();

        let mut ids = Vec::new();
        // Create MAX_SURFACES surfaces
        for i in 0..MAX_SURFACES {
            let info = SurfaceInfo {
                x: 0, y: 0,
                width: 10 + i as u16, height: 10,
                role: SurfaceRole::AppContent as u32,
                visible: true,
            };
            let id = unsafe { display_server_create_surface(&info) };
            assert_ne!(id, 0, "surface {} should be created", i);
            ids.push(id);
        }

        // Try to create one more — should fail
        let info = test_surface(100, 100, SurfaceRole::AppContent);
        let id = unsafe { display_server_create_surface(&info) };
        assert_eq!(id, 0, "create_surface should return 0 when MAX_SURFACES exceeded");
    }

    #[test]
    fn test_set_visible() {
        display_server_init();
        let info = test_surface(100, 100, SurfaceRole::AppContent);
        let id = unsafe { display_server_create_surface(&info) };
        assert_ne!(id, 0);

        // Initially visible (from test_surface helper)
        {
            let lock = DS.lock().unwrap();
            assert!(lock.as_ref().unwrap().surfaces[0].info.visible);
        }

        // Set invisible
        unsafe { display_server_set_visible(id, false); }
        {
            let lock = DS.lock().unwrap();
            assert!(!lock.as_ref().unwrap().surfaces[0].info.visible);
        }

        // Set visible again
        unsafe { display_server_set_visible(id, true); }
        {
            let lock = DS.lock().unwrap();
            assert!(lock.as_ref().unwrap().surfaces[0].info.visible);
        }
    }

    #[test]
    fn test_mark_dirty_merges_regions() {
        display_server_init();
        let info = test_surface(320, 240, SurfaceRole::AppContent);
        let id = unsafe { display_server_create_surface(&info) };
        assert_ne!(id, 0);

        // Mark dirty with area (10, 10, 50, 50)
        let area1 = HalArea { x1: 10, y1: 10, x2: 50, y2: 50 };
        unsafe { display_server_mark_dirty(id, &area1); }

        {
            let lock = DS.lock().unwrap();
            let surface = &lock.as_ref().unwrap().surfaces[0];
            assert!(surface.dirty);
            assert_eq!(surface.dirty_area.x1, 10);
            assert_eq!(surface.dirty_area.y1, 10);
            assert_eq!(surface.dirty_area.x2, 50);
            assert_eq!(surface.dirty_area.y2, 50);
        }

        // Mark dirty with overlapping area (30, 30, 80, 80)
        let area2 = HalArea { x1: 30, y1: 30, x2: 80, y2: 80 };
        unsafe { display_server_mark_dirty(id, &area2); }

        // Result should be merged to (10, 10, 80, 80)
        {
            let lock = DS.lock().unwrap();
            let surface = &lock.as_ref().unwrap().surfaces[0];
            assert!(surface.dirty);
            assert_eq!(surface.dirty_area.x1, 10, "merged x1 should be min");
            assert_eq!(surface.dirty_area.y1, 10, "merged y1 should be min");
            assert_eq!(surface.dirty_area.x2, 80, "merged x2 should be max");
            assert_eq!(surface.dirty_area.y2, 80, "merged y2 should be max");
        }
    }

    #[test]
    fn test_mark_dirty_full() {
        display_server_init();
        let info = test_surface(320, 240, SurfaceRole::AppContent);
        let id = unsafe { display_server_create_surface(&info) };
        assert_ne!(id, 0);

        unsafe { display_server_mark_dirty_full(id); }

        let lock = DS.lock().unwrap();
        let surface = &lock.as_ref().unwrap().surfaces[0];
        assert!(surface.dirty);
        assert_eq!(surface.dirty_area.x1, 0);
        assert_eq!(surface.dirty_area.y1, 0);
        assert_eq!(surface.dirty_area.x2, 319, "x2 should be width-1");
        assert_eq!(surface.dirty_area.y2, 239, "y2 should be height-1");
    }

    #[test]
    fn test_surface_role_values() {
        assert_eq!(SurfaceRole::Background as u32, 0);
        assert_eq!(SurfaceRole::StatusBar as u32, 1);
        assert_eq!(SurfaceRole::AppContent as u32, 2);
        assert_eq!(SurfaceRole::Overlay as u32, 3);
        assert_eq!(SurfaceRole::Dock as u32, 4);
    }

    #[test]
    fn test_create_multiple_surfaces_unique_ids() {
        display_server_init();
        let mut ids = Vec::new();

        for i in 0..5 {
            let info = SurfaceInfo {
                x: 0, y: 0,
                width: 100, height: 100,
                role: (i % 5) as u32,
                visible: true,
            };
            let id = unsafe { display_server_create_surface(&info) };
            assert_ne!(id, 0);
            ids.push(id);
        }

        // Check all IDs are unique and incrementing
        for i in 0..ids.len() {
            assert_eq!(ids[i], (i + 1) as u32, "IDs should be incrementing from 1");
            for j in i + 1..ids.len() {
                assert_ne!(ids[i], ids[j], "IDs should be unique");
            }
        }
    }

    #[test]
    fn test_surface_empty() {
        let surface = Surface::empty();
        assert!(!surface.allocated);
        assert!(!surface.dirty);
        assert_eq!(surface.info.x, 0);
        assert_eq!(surface.info.y, 0);
        assert_eq!(surface.info.width, 0);
        assert_eq!(surface.info.height, 0);
        assert!(!surface.info.visible);
        assert_eq!(surface.buffer.len(), 0);
    }

    #[test]
    fn test_display_server_new() {
        let ds = DisplayServer::new();
        assert!(!ds.initialized);
        assert_eq!(ds.surfaces.len(), MAX_SURFACES);
        assert_eq!(ds.next_id, 1);
        assert!(ds.wm.is_none());

        // Verify all surfaces are empty
        for surface in &ds.surfaces {
            assert!(!surface.allocated);
        }
    }

    #[test]
    fn test_get_buffer_returns_null_for_unallocated() {
        display_server_init();
        let buf = unsafe { display_server_get_buffer(1) };
        assert!(buf.is_null(), "get_buffer should return null for unallocated surface");
    }

    #[test]
    fn test_get_buffer_returns_valid_ptr() {
        display_server_init();
        let info = test_surface(100, 100, SurfaceRole::AppContent);
        let id = unsafe { display_server_create_surface(&info) };
        assert_ne!(id, 0);

        let buf = unsafe { display_server_get_buffer(id) };
        assert!(!buf.is_null(), "get_buffer should return non-null for allocated surface");
    }

    #[test]
    fn test_get_info_returns_surface_info() {
        display_server_init();
        let info = SurfaceInfo {
            x: 10, y: 20,
            width: 150, height: 200,
            role: SurfaceRole::StatusBar as u32,
            visible: true,
        };
        let id = unsafe { display_server_create_surface(&info) };
        assert_ne!(id, 0);

        let retrieved_info = unsafe { display_server_get_info(id) };
        assert!(!retrieved_info.is_null());

        let retrieved_info = unsafe { *retrieved_info };
        assert_eq!(retrieved_info.x, 10);
        assert_eq!(retrieved_info.y, 20);
        assert_eq!(retrieved_info.width, 150);
        assert_eq!(retrieved_info.height, 200);
        assert_eq!(retrieved_info.role, SurfaceRole::StatusBar as u32);
    }

    #[test]
    fn test_mark_dirty_invalid_surface() {
        display_server_init();
        let area = HalArea { x1: 0, y1: 0, x2: 100, y2: 100 };
        // Try to mark dirty on non-existent surface — should not panic
        unsafe { display_server_mark_dirty(999, &area); }
        // Test passes if no panic
    }

    #[test]
    fn test_surface_reuse_after_destroy() {
        display_server_init();

        // Create, destroy, and recreate in same slot
        let info = test_surface(100, 100, SurfaceRole::AppContent);
        let id1 = unsafe { display_server_create_surface(&info) };
        assert_eq!(id1, 1);

        unsafe { display_server_destroy_surface(id1); }

        let id2 = unsafe { display_server_create_surface(&info) };
        assert_eq!(id2, 2, "new surface should get next ID, not reuse 1");

        {
            let lock = DS.lock().unwrap();
            // Both slots should have surface 2 allocated
            assert!(lock.as_ref().unwrap().surfaces[0].allocated);
            assert!(lock.as_ref().unwrap().surfaces[0].info.visible);
        }
    }
}
