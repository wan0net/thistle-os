// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — thistle-tk Launcher App
//
// A built-in launcher for the thistle-tk window manager. Registers as
// "com.thistle.tk_launcher" and builds its UI purely through the
// thistle_ui_* widget API (which dispatches through the WM vtable).
//
// Layout:
//   - Status bar (24px row): "ThistleOS" label left, clock label right
//   - Scrollable app list below: one button per registered app

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr::addr_of_mut;
use std::sync::Mutex;

use crate::app_manager::{self, CAppEntry, CAppManifest};

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0;

// ---------------------------------------------------------------------------
// Widget API imports — these go through widget.rs -> widget_shims.c -> WM
// ---------------------------------------------------------------------------

extern "C" {
    fn thistle_ui_get_app_root() -> u32;
    fn thistle_ui_create_container(parent: u32) -> u32;
    fn thistle_ui_create_label(parent: u32, text: *const c_char) -> u32;
    fn thistle_ui_create_button(parent: u32, text: *const c_char) -> u32;
    fn thistle_ui_set_size(widget: u32, w: i32, h: i32);
    fn thistle_ui_set_layout(widget: u32, layout: i32);
    fn thistle_ui_set_align(widget: u32, main_align: i32, cross_align: i32);
    fn thistle_ui_set_gap(widget: u32, gap: i32);
    fn thistle_ui_set_flex_grow(widget: u32, grow: i32);
    fn thistle_ui_set_scrollable(widget: u32, scrollable: bool);
    fn thistle_ui_set_padding(widget: u32, t: i32, r: i32, b: i32, l: i32);
    fn thistle_ui_set_bg_color(widget: u32, color: u32);
    fn thistle_ui_set_text_color(widget: u32, color: u32);
    fn thistle_ui_set_font_size(widget: u32, size: i32);
    fn thistle_ui_set_radius(widget: u32, r: i32);
    fn thistle_ui_theme_bg() -> u32;
    fn thistle_ui_theme_text() -> u32;
    fn thistle_ui_theme_text_secondary() -> u32;
    fn thistle_ui_theme_surface() -> u32;
    fn thistle_ui_theme_primary() -> u32;

    fn app_manager_list_apps(out: *mut *const CAppManifest, max_count: i32) -> i32;
}

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

const LAYOUT_COLUMN: i32 = 0;
const LAYOUT_ROW: i32 = 1;
const ALIGN_CENTER: i32 = 1;
const ALIGN_SPACE_BETWEEN: i32 = 3;
const STATUS_BAR_HEIGHT: i32 = 24;

// ---------------------------------------------------------------------------
// App button metadata — stores the CStrings so they live long enough
// ---------------------------------------------------------------------------

/// Launcher state protected by a mutex. Stores app ID strings and button
/// widget IDs so the on_press callback can reference them later.
struct LauncherState {
    app_ids: Vec<CString>,
    app_buttons: Vec<u32>,
}

static LAUNCHER: Mutex<LauncherState> = Mutex::new(LauncherState {
    app_ids: Vec::new(),
    app_buttons: Vec::new(),
});

/// Deferred launch queue. on_press runs while TK_WM and the dispatch path
/// are locked; calling app_manager_launch directly from there would deadlock
/// when the target app's on_create acquires TK_WM. Instead the press handler
/// just stows the app id here, and tk_wm_on_input drains it after dropping
/// its locks.
static PENDING_LAUNCH: Mutex<Option<CString>> = Mutex::new(None);

/// Called by the WM input dispatcher after a key event has been processed
/// and all locks have been released. Launches whichever app the most recent
/// button press queued, if any.
pub fn process_pending_launch() {
    let pending = match PENDING_LAUNCH.lock() {
        Ok(mut p) => p.take(),
        Err(_) => return,
    };
    if let Some(app_id) = pending {
        let id_str = app_id.to_str().unwrap_or("");
        #[cfg(target_os = "espidf")]
        unsafe {
            esp_log_write(3, b"thistle\0".as_ptr(),
                          b"tk_launcher: launching app id=%s\0".as_ptr(),
                          app_id.as_ptr());
        }
        let rc = crate::app_manager::launch(id_str);
        #[cfg(target_os = "espidf")]
        unsafe {
            esp_log_write(3, b"thistle\0".as_ptr(),
                          b"tk_launcher: launch returned %d\0".as_ptr(), rc);
        }
        #[cfg(not(target_os = "espidf"))]
        let _ = rc;
    }
}

/// on_press handler attached to every launcher button. Looks up which app
/// id the button corresponds to and queues it for launch.
fn launcher_on_press(widget: thistle_tk::widget::WidgetId) {
    let widget_u32 = widget as u32;
    let app_id = {
        let state = match LAUNCHER.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        let idx = state.app_buttons.iter().position(|&id| id == widget_u32);
        match idx.and_then(|i| state.app_ids.get(i)) {
            Some(id) => id.clone(),
            None => return,
        }
    };
    if let Ok(mut pending) = PENDING_LAUNCH.lock() {
        *pending = Some(app_id);
    }
}

// ---------------------------------------------------------------------------
// Lifecycle callbacks
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

/// on_create: Build the entire launcher UI
unsafe extern "C" fn on_create() -> i32 {
    // The thistle-tk WM uses WidgetId 0 as the tree root; 0 is a valid id
    // here, not a sentinel. (An earlier `if root == 0 { return -1 }` check
    // bailed on every successful call.)
    let root = thistle_ui_get_app_root();

    let bg_color = thistle_ui_theme_bg();
    let primary_color = thistle_ui_theme_primary();

    // Root column container filling the screen
    let main_col = thistle_ui_create_container(root);
    thistle_ui_set_layout(main_col, LAYOUT_COLUMN);
    thistle_ui_set_size(main_col, -1, -1); // fill parent
    thistle_ui_set_flex_grow(main_col, 1);
    thistle_ui_set_bg_color(main_col, bg_color);
    thistle_ui_set_gap(main_col, 0);

    // -- App list (scrollable column, fills the whole panel) -----------------
    // The status bar (ThistleOS title + clock placeholder) was a development
    // placeholder; with real apps now showing up in the list it just stole
    // 24 px from every entry below. Re-add as part of a proper system-wide
    // chrome layer when there's something useful to put in it.
    let app_list = thistle_ui_create_container(main_col);
    thistle_ui_set_layout(app_list, LAYOUT_COLUMN);
    thistle_ui_set_flex_grow(app_list, 1);
    thistle_ui_set_gap(app_list, 4);
    thistle_ui_set_padding(app_list, 4, 6, 4, 6);
    thistle_ui_set_scrollable(app_list, true);
    thistle_ui_set_bg_color(app_list, bg_color);

    // -- Populate app buttons -------------------------------------------------
    let mut manifests: [*const CAppManifest; 20] = [std::ptr::null(); 20];
    let count = app_manager_list_apps(manifests.as_mut_ptr(), 20);

    let mut state = LAUNCHER.lock().unwrap();
    state.app_ids.clear();
    state.app_buttons.clear();

    for i in 0..(count as usize) {
        let manifest = manifests[i];
        if manifest.is_null() {
            continue;
        }
        let m = &*manifest;

        // Skip the launcher itself
        if !m.id.is_null() {
            let id_str = CStr::from_ptr(m.id).to_str().unwrap_or("");
            if id_str == "com.thistle.tk_launcher" {
                continue;
            }
        }

        // Get display name
        let name_ptr = if !m.name.is_null() { m.name } else { m.id };
        if name_ptr.is_null() {
            continue;
        }

        // Create button with app name
        let btn = thistle_ui_create_button(app_list, name_ptr);
        thistle_ui_set_size(btn, -1, 36);
        thistle_ui_set_bg_color(btn, primary_color);
        thistle_ui_set_text_color(btn, bg_color);
        thistle_ui_set_radius(btn, 4);

        // Store app ID for the callback
        let id_cstring = if !m.id.is_null() {
            CString::from(CStr::from_ptr(m.id))
        } else {
            CString::new("").unwrap()
        };

        state.app_buttons.push(btn);
        state.app_ids.push(id_cstring);

        // Wire press → launch. Bypasses tk_wm_widget_on_event (which is a
        // C-callback bridge stub) by attaching the Rust fn directly.
        crate::tk_wm::set_button_on_press(btn, launcher_on_press);
    }

    ESP_OK
}

/// on_start: Launcher is becoming the foreground app
unsafe extern "C" fn on_start() {
    // No-op for now — the UI is already built in on_create
}

/// on_pause: Launcher is going to background
unsafe extern "C" fn on_pause() {
    // No-op — keep UI tree intact
}

/// on_resume: Launcher is returning to foreground
unsafe extern "C" fn on_resume() {
    // No-op for now — could refresh app list here in future
}

/// on_destroy: Cleanup
unsafe extern "C" fn on_destroy() {
    if let Ok(mut state) = LAUNCHER.lock() {
        state.app_ids.clear();
        state.app_buttons.clear();
    }
}

// ---------------------------------------------------------------------------
// Static manifest and entry (must live for the lifetime of the kernel)
// ---------------------------------------------------------------------------

static MANIFEST: CAppManifest = CAppManifest {
    id:               b"com.thistle.tk_launcher\0".as_ptr() as *const c_char,
    name:             b"Launcher\0".as_ptr() as *const c_char,
    version:          b"0.1.0\0".as_ptr() as *const c_char,
    allow_background: false,
    min_memory_kb:    0,
};

static ENTRY: CAppEntry = CAppEntry {
    on_create:  Some(on_create),
    on_start:   Some(on_start),
    on_pause:   Some(on_pause),
    on_resume:  Some(on_resume),
    on_destroy: Some(on_destroy),
    manifest:   &MANIFEST as *const CAppManifest,
};

// ---------------------------------------------------------------------------
// Registration — called from kernel_boot.rs
// ---------------------------------------------------------------------------

/// Register the thistle-tk launcher with the app manager.
/// Returns ESP_OK (0) on success.
pub fn register() -> i32 {
    unsafe { app_manager::register(&ENTRY as *const CAppEntry) }
}

/// C-callable registration entry point.
#[no_mangle]
pub extern "C" fn tk_launcher_register() -> i32 {
    register()
}
