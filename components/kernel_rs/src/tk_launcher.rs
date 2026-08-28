// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — thistle-tk Launcher App
//
// A built-in launcher for the thistle-tk window manager. Registers as
// "com.thistle.tk_launcher" and builds its UI purely through the
// thistle_ui_* widget API (which dispatches through the WM vtable).
//
// Ports the built-in launcher home screen to thistle-tk:
//   - centered clock and ThistleOS branding
//   - Apps button
//   - favorites/installed-app dock
//   - full-screen app drawer

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

use crate::app_manager::{self, CAppEntry, CAppManifest};
use thistle_tk::widget::IconKind;

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
    fn thistle_ui_set_text(widget: u32, text: *const c_char);
    fn thistle_ui_set_size(widget: u32, w: i32, h: i32);
    fn thistle_ui_set_pos(widget: u32, x: i32, y: i32);
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
    fn thistle_ui_set_border_width(widget: u32, width: i32);
    fn thistle_ui_set_visible(widget: u32, visible: bool);
    fn thistle_ui_destroy(widget: u32);
    fn thistle_ui_theme_bg() -> u32;
    fn thistle_ui_theme_text() -> u32;
    fn thistle_ui_theme_text_secondary() -> u32;
    fn thistle_ui_theme_surface() -> u32;
    fn app_manager_list_apps(out: *mut *const CAppManifest, max_count: i32) -> i32;
    fn wifi_manager_get_time_str(buf: *mut c_char, buf_len: usize);
}

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

const LAYOUT_COLUMN: i32 = 1;
const LAYOUT_ROW: i32 = 2;
const ALIGN_CENTER: i32 = 1;
const ALIGN_START: i32 = 0;
const ALIGN_SPACE_BETWEEN: i32 = 3;
const STATUS_BAR_HEIGHT: i32 = 24;
const DOCK_HEIGHT: i32 = 50;
const GRID_ROW_HEIGHT: i32 = 93;
const TILE_WIDTH: i32 = 74;
const TILE_HEIGHT: i32 = 88;
const FOLDER_TILE_WIDTH: i32 = 62;
const FOLDER_TILE_HEIGHT: i32 = 66;

// ---------------------------------------------------------------------------
// App button metadata — stores the CStrings so they live long enough
// ---------------------------------------------------------------------------

/// Launcher state protected by a mutex. Stores app ID strings and button
/// widget IDs so the on_press callback can reference them later.
struct LauncherState {
    shell: u32,
    home: u32,
    drawer: u32,
    folder: u32,
    clock: u32,
    status_clock: u32,
    drawer_visible: bool,
    folder_visible: bool,
    last_time: [u8; 6],
    app_ids: Vec<CString>,
    app_buttons: Vec<u32>,
}

static LAUNCHER: Mutex<LauncherState> = Mutex::new(LauncherState {
    shell: 0,
    home: 0,
    drawer: 0,
    folder: 0,
    clock: 0,
    status_clock: 0,
    drawer_visible: false,
    folder_visible: false,
    last_time: [0; 6],
    app_ids: Vec::new(),
    app_buttons: Vec::new(),
});

/// Deferred launch queue. on_press runs while TK_WM and the dispatch path
/// are locked; calling app_manager_launch directly from there would deadlock
/// when the target app's on_create acquires TK_WM. Instead the press handler
/// just stows the app id here, and tk_wm_on_input drains it after dropping
/// its locks.
enum PendingAction {
    ShowHome,
    ShowDrawer,
    ShowFolder,
    Launch(CString),
}

static PENDING_ACTION: Mutex<Option<PendingAction>> = Mutex::new(None);

fn show_view(action: PendingAction) {
    let (home, drawer, folder, show_home, show_drawer, show_folder) = match LAUNCHER.lock() {
        Ok(mut state) => {
            let (show_home, show_drawer, show_folder) = match action {
                PendingAction::ShowHome => (true, false, false),
                PendingAction::ShowDrawer => (false, true, false),
                PendingAction::ShowFolder => (true, false, true),
                PendingAction::Launch(_) => return,
            };
            state.drawer_visible = show_drawer;
            state.folder_visible = show_folder;
            (
                state.home,
                state.drawer,
                state.folder,
                show_home,
                show_drawer,
                show_folder,
            )
        }
        Err(_) => return,
    };
    unsafe {
        thistle_ui_set_visible(home, show_home);
        thistle_ui_set_visible(drawer, show_drawer);
        thistle_ui_set_visible(folder, show_folder);
    }
    let visible_root = if show_drawer {
        drawer
    } else if show_folder {
        folder
    } else {
        home
    };
    crate::tk_wm::focus_first_descendant(visible_root);
}

/// Called by the WM input dispatcher after a key event has been processed
/// and all locks have been released. Launches whichever app the most recent
/// button press queued, if any.
pub fn process_pending_launch() {
    let pending = match PENDING_ACTION.lock() {
        Ok(mut p) => p.take(),
        Err(_) => return,
    };
    match pending {
        Some(PendingAction::ShowHome) => show_view(PendingAction::ShowHome),
        Some(PendingAction::ShowDrawer) => show_view(PendingAction::ShowDrawer),
        Some(PendingAction::ShowFolder) => show_view(PendingAction::ShowFolder),
        Some(PendingAction::Launch(app_id)) => {
            show_view(PendingAction::ShowHome);
            let id_str = app_id.to_str().unwrap_or("");
            #[cfg(target_os = "espidf")]
            unsafe {
                esp_log_write(
                    3,
                    b"thistle\0".as_ptr(),
                    b"tk_launcher: launching app id=%s\0".as_ptr(),
                    app_id.as_ptr(),
                );
            }
            let rc = crate::app_manager::launch(id_str);
            #[cfg(target_os = "espidf")]
            unsafe {
                esp_log_write(
                    3,
                    b"thistle\0".as_ptr(),
                    b"tk_launcher: launch returned %d\0".as_ptr(),
                    rc,
                );
            }
            #[cfg(not(target_os = "espidf"))]
            let _ = rc;
        }
        None => {}
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
    if let Ok(mut pending) = PENDING_ACTION.lock() {
        *pending = Some(PendingAction::Launch(app_id));
    }
}

fn apps_on_press(_widget: thistle_tk::widget::WidgetId) {
    if let Ok(mut pending) = PENDING_ACTION.lock() {
        *pending = Some(PendingAction::ShowDrawer);
    }
}

fn home_on_press(_widget: thistle_tk::widget::WidgetId) {
    if let Ok(mut pending) = PENDING_ACTION.lock() {
        *pending = Some(PendingAction::ShowHome);
    }
}

fn folder_on_press(_widget: thistle_tk::widget::WidgetId) {
    if let Ok(mut pending) = PENDING_ACTION.lock() {
        *pending = Some(PendingAction::ShowFolder);
    }
}

/// Queue the phone-style Back transition when a launcher layer is open.
/// Returns false on Home (or while another app is foreground) so the caller
/// can continue normal key dispatch.
pub fn handle_back_key() -> bool {
    let should_close = LAUNCHER
        .lock()
        .map(|state| state.drawer_visible || state.folder_visible)
        .unwrap_or(false);
    if should_close {
        if let Ok(mut pending) = PENDING_ACTION.lock() {
            *pending = Some(PendingAction::ShowHome);
            return true;
        }
    }
    false
}

fn app_icon(app_id: &str) -> IconKind {
    if app_id.contains("settings") {
        IconKind::Settings
    } else if app_id.contains("filemgr") {
        IconKind::Folder
    } else if app_id.contains("reader") {
        IconKind::Reader
    } else if app_id.contains("messenger") {
        IconKind::Message
    } else if app_id.contains("navigator") {
        IconKind::Map
    } else if app_id.contains("notes") {
        IconKind::Note
    } else if app_id.contains("assistant") {
        IconKind::Assistant
    } else if app_id.contains("appstore") {
        IconKind::Store
    } else if app_id.contains("radio") || app_id.contains("wifi") {
        IconKind::Radio
    } else if app_id.contains("terminal") {
        IconKind::Terminal
    } else {
        IconKind::Unknown
    }
}

fn short_label(app_id: &str, fallback: &str) -> CString {
    let label = if app_id.contains("settings") {
        "Settings"
    } else if app_id.contains("filemgr") {
        "Files"
    } else if app_id.contains("reader") {
        "Reader"
    } else if app_id.contains("messenger") {
        "Messages"
    } else if app_id.contains("navigator") {
        "Maps"
    } else if app_id.contains("notes") {
        "Notes"
    } else if app_id.contains("assistant") {
        "Assistant"
    } else if app_id.contains("appstore") {
        "Store"
    } else if app_id.contains("wifiscanner") {
        "WiFi"
    } else if app_id.contains("flashlight") {
        "Light"
    } else if app_id.contains("weather") {
        "Weather"
    } else if app_id.contains("terminal") {
        "Terminal"
    } else {
        fallback
    };
    CString::new(label).unwrap_or_else(|_| CString::new("App").unwrap())
}

fn is_tool_app(app_id: &str) -> bool {
    app_id.contains("settings")
        || app_id.contains("terminal")
        || app_id.contains("wifiscanner")
        || app_id.contains("flashlight")
}

/// Refresh the clock only when its displayed minute changes. The render task
/// calls this frequently, but unchanged time leaves the e-paper completely
/// static and does not mark the widget tree dirty.
#[no_mangle]
pub extern "C" fn tk_launcher_tick() {
    let mut time_buf = [0 as c_char; 8];
    unsafe { wifi_manager_get_time_str(time_buf.as_mut_ptr(), time_buf.len()) };

    let mut display_time = [0u8; 6];
    for (dst, src) in display_time.iter_mut().take(5).zip(time_buf.iter()) {
        *dst = *src as u8;
    }

    let (clock, status_clock) = match LAUNCHER.lock() {
        Ok(mut state) => {
            if state.clock == 0 || state.last_time == display_time {
                return;
            }
            state.last_time = display_time;
            (state.clock, state.status_clock)
        }
        Err(_) => return,
    };

    unsafe {
        thistle_ui_set_text(clock, display_time.as_ptr() as *const c_char);
        if status_clock != 0 {
            thistle_ui_set_text(status_clock, display_time.as_ptr() as *const c_char);
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle callbacks
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn esp_log_write(level: i32, tag: *const u8, format: *const u8, ...);
}

#[derive(Clone)]
struct AppInfo {
    id: CString,
    label: CString,
    icon: IconKind,
}

unsafe fn create_icon_button(
    parent: u32,
    label: *const c_char,
    icon: IconKind,
    width: i32,
    height: i32,
    bg: u32,
    text: u32,
    on_press: thistle_tk::widget::OnPress,
) -> u32 {
    let button = thistle_ui_create_button(parent, label);
    thistle_ui_set_size(button, width, height);
    thistle_ui_set_bg_color(button, bg);
    thistle_ui_set_text_color(button, text);
    thistle_ui_set_border_width(button, 1);
    thistle_ui_set_radius(button, 8);
    crate::tk_wm::set_button_icon(button, icon);
    crate::tk_wm::set_button_on_press(button, on_press);
    button
}

unsafe fn create_grid_row(parent: u32, bg: u32) -> u32 {
    let row = thistle_ui_create_container(parent);
    thistle_ui_set_layout(row, LAYOUT_ROW);
    thistle_ui_set_size(row, -1, GRID_ROW_HEIGHT);
    thistle_ui_set_align(row, ALIGN_SPACE_BETWEEN, ALIGN_CENTER);
    thistle_ui_set_padding(row, 2, 4, 2, 4);
    thistle_ui_set_bg_color(row, bg);
    row
}

unsafe fn create_view_header(
    parent: u32,
    title: &[u8],
    bg: u32,
    text: u32,
    with_rule: bool,
) -> u32 {
    let frame = thistle_ui_create_container(parent);
    thistle_ui_set_layout(frame, LAYOUT_COLUMN);
    thistle_ui_set_size(frame, -1, 30);
    thistle_ui_set_bg_color(frame, bg);

    let header = thistle_ui_create_container(frame);
    thistle_ui_set_layout(header, LAYOUT_ROW);
    thistle_ui_set_size(header, -1, if with_rule { 29 } else { 30 });
    thistle_ui_set_align(header, ALIGN_SPACE_BETWEEN, ALIGN_CENTER);
    thistle_ui_set_padding(header, 3, 6, 3, 8);
    thistle_ui_set_bg_color(header, bg);

    let title_widget = thistle_ui_create_label(header, title.as_ptr() as *const c_char);
    thistle_ui_set_font_size(title_widget, 16);
    thistle_ui_set_text_color(title_widget, text);

    let close = create_icon_button(
        header,
        b"\0".as_ptr() as *const c_char,
        IconKind::Close,
        22,
        22,
        bg,
        text,
        home_on_press,
    );
    thistle_ui_set_radius(close, 0);
    if with_rule {
        let rule = thistle_ui_create_container(frame);
        thistle_ui_set_size(rule, -1, 1);
        thistle_ui_set_bg_color(rule, text);
    }
    header
}

/// on_create: Build the entire launcher UI
unsafe extern "C" fn on_create() -> i32 {
    // The thistle-tk WM uses WidgetId 0 as the tree root; 0 is a valid id
    // here, not a sentinel. (An earlier `if root == 0 { return -1 }` check
    // bailed on every successful call.)
    let root = thistle_ui_get_app_root();

    let bg_color = thistle_ui_theme_bg();
    let text_color = thistle_ui_theme_text();
    let secondary_color = thistle_ui_theme_text_secondary();
    let surface_color = thistle_ui_theme_surface();

    // Mirror the LCD WM chrome exactly. The e-paper launcher gets the same
    // 24 px system bar above the 296 px app area; only its palette maps to
    // strict monochrome.
    thistle_ui_set_layout(root, LAYOUT_COLUMN);
    thistle_ui_set_gap(root, 0);
    thistle_ui_set_bg_color(root, bg_color);

    let status_frame = thistle_ui_create_container(root);
    thistle_ui_set_layout(status_frame, LAYOUT_COLUMN);
    thistle_ui_set_size(status_frame, -1, STATUS_BAR_HEIGHT);
    thistle_ui_set_bg_color(status_frame, surface_color);

    let status_bar = thistle_ui_create_container(status_frame);
    thistle_ui_set_layout(status_bar, LAYOUT_ROW);
    thistle_ui_set_size(status_bar, -1, STATUS_BAR_HEIGHT - 1);
    thistle_ui_set_align(status_bar, ALIGN_SPACE_BETWEEN, ALIGN_CENTER);
    thistle_ui_set_padding(status_bar, 2, 4, 2, 4);
    thistle_ui_set_bg_color(status_bar, surface_color);

    let status_title = thistle_ui_create_label(status_bar, b"Launcher\0".as_ptr() as *const c_char);
    thistle_ui_set_size(status_title, 70, 18);
    thistle_ui_set_font_size(status_title, 10);
    thistle_ui_set_text_color(status_title, text_color);

    let mut status_time_buf = [0 as c_char; 8];
    wifi_manager_get_time_str(status_time_buf.as_mut_ptr(), status_time_buf.len());
    let status_clock = thistle_ui_create_label(status_bar, status_time_buf.as_ptr());
    thistle_ui_set_size(status_clock, 44, 18);
    thistle_ui_set_font_size(status_clock, 10);
    thistle_ui_set_text_color(status_clock, text_color);

    let status_battery =
        thistle_ui_create_label(status_bar, b"-- BAT:--%\0".as_ptr() as *const c_char);
    thistle_ui_set_size(status_battery, 74, 18);
    thistle_ui_set_font_size(status_battery, 10);
    thistle_ui_set_text_color(status_battery, text_color);

    let status_rule = thistle_ui_create_container(status_frame);
    thistle_ui_set_size(status_rule, -1, 1);
    thistle_ui_set_bg_color(status_rule, secondary_color);

    let mut manifests: [*const CAppManifest; 20] = [std::ptr::null(); 20];
    let count = app_manager_list_apps(manifests.as_mut_ptr(), manifests.len() as i32);
    let mut apps = Vec::new();
    for manifest in manifests.iter().take(count.max(0) as usize) {
        if manifest.is_null() {
            continue;
        }
        let m = &**manifest;
        if m.id.is_null() {
            continue;
        }
        let id = CStr::from_ptr(m.id).to_str().unwrap_or("");
        if matches!(id, "com.thistle.tk_launcher" | "com.thistle.launcher") {
            continue;
        }
        let fallback = if m.name.is_null() {
            id
        } else {
            CStr::from_ptr(m.name).to_str().unwrap_or(id)
        };
        apps.push(AppInfo {
            id: CString::new(id).unwrap_or_else(|_| CString::new("com.thistle.unknown").unwrap()),
            label: short_label(id, fallback),
            icon: app_icon(id),
        });
    }

    // Shell holds three mutually-exclusive full-screen views. Hidden children
    // are excluded by thistle-tk layout, so each view fills the panel.
    let shell = thistle_ui_create_container(root);
    thistle_ui_set_layout(shell, LAYOUT_COLUMN);
    thistle_ui_set_size(shell, -1, -1);
    thistle_ui_set_flex_grow(shell, 1);
    thistle_ui_set_bg_color(shell, bg_color);
    thistle_ui_set_gap(shell, 0);

    // -- Home ---------------------------------------------------------------
    let home = thistle_ui_create_container(shell);
    thistle_ui_set_layout(home, LAYOUT_COLUMN);
    thistle_ui_set_flex_grow(home, 1);
    thistle_ui_set_bg_color(home, bg_color);
    thistle_ui_set_gap(home, 0);

    let tools: Vec<usize> = apps
        .iter()
        .enumerate()
        .filter_map(|(idx, app)| is_tool_app(app.id.to_str().unwrap_or("")).then_some(idx))
        .collect();
    let mut bindings: Vec<(u32, CString)> = Vec::new();

    // Home is intentionally empty. A flexible blank canvas keeps the dock at
    // the bottom; the dock's Apps button is the only route into the app grid.
    let home_canvas = thistle_ui_create_container(home);
    thistle_ui_set_flex_grow(home_canvas, 1);
    thistle_ui_set_bg_color(home_canvas, bg_color);

    let dock_frame = thistle_ui_create_container(home);
    thistle_ui_set_layout(dock_frame, LAYOUT_COLUMN);
    thistle_ui_set_size(dock_frame, -1, DOCK_HEIGHT);
    thistle_ui_set_bg_color(dock_frame, surface_color);

    let dock_rule = thistle_ui_create_container(dock_frame);
    thistle_ui_set_size(dock_rule, -1, 1);
    thistle_ui_set_bg_color(dock_rule, secondary_color);

    let dock = thistle_ui_create_container(dock_frame);
    thistle_ui_set_layout(dock, LAYOUT_ROW);
    thistle_ui_set_size(dock, -1, DOCK_HEIGHT - 1);
    thistle_ui_set_align(dock, ALIGN_CENTER, ALIGN_CENTER);
    thistle_ui_set_gap(dock, 8);
    thistle_ui_set_padding(dock, 6, 6, 6, 6);
    thistle_ui_set_bg_color(dock, surface_color);

    let mut dock_indices: Vec<usize> = Vec::new();
    for needle in ["messenger", "navigator", "notes", "filemgr", "reader"] {
        if let Some(idx) = (0..apps.len()).find(|idx| {
            apps[*idx].id.to_str().unwrap_or("").contains(needle) && !dock_indices.contains(idx)
        }) {
            dock_indices.push(idx);
        }
        if dock_indices.len() == 4 {
            break;
        }
    }
    for idx in 0..apps.len() {
        if dock_indices.len() == 4 {
            break;
        }
        if !dock_indices.contains(&idx) {
            dock_indices.push(idx);
        }
    }
    let apps_dock_button = |dock: u32| {
        let button = create_icon_button(
            dock,
            b"\0".as_ptr() as *const c_char,
            IconKind::Apps,
            38,
            38,
            surface_color,
            text_color,
            apps_on_press,
        );
        thistle_ui_set_radius(button, 0);
    };

    let dock_count = dock_indices.len();
    let apps_position = dock_count.min(2);
    for (position, idx) in dock_indices.into_iter().enumerate() {
        if position == apps_position {
            apps_dock_button(dock);
        }
        let app = &apps[idx];
        let button = create_icon_button(
            dock,
            b"\0".as_ptr() as *const c_char,
            app.icon,
            38,
            38,
            surface_color,
            text_color,
            launcher_on_press,
        );
        thistle_ui_set_radius(button, 0);
        bindings.push((button, app.id.clone()));
    }
    if apps_position == dock_count {
        apps_dock_button(dock);
    }

    // -- Drawer -------------------------------------------------------------
    let drawer = thistle_ui_create_container(shell);
    thistle_ui_set_pos(drawer, 0, 0);
    thistle_ui_set_layout(drawer, LAYOUT_COLUMN);
    thistle_ui_set_flex_grow(drawer, 1);
    thistle_ui_set_bg_color(drawer, bg_color);
    thistle_ui_set_gap(drawer, 0);

    create_view_header(drawer, b"All Apps\0", surface_color, text_color, true);

    let drawer_grid = thistle_ui_create_container(drawer);
    thistle_ui_set_layout(drawer_grid, LAYOUT_COLUMN);
    thistle_ui_set_flex_grow(drawer_grid, 1);
    thistle_ui_set_gap(drawer_grid, 0);
    thistle_ui_set_bg_color(drawer_grid, bg_color);
    thistle_ui_set_scrollable(drawer_grid, true);
    for chunk in apps.chunks(3) {
        let row = create_grid_row(drawer_grid, bg_color);
        for app in chunk {
            let button = create_icon_button(
                row,
                app.label.as_ptr(),
                app.icon,
                TILE_WIDTH,
                TILE_HEIGHT,
                bg_color,
                text_color,
                launcher_on_press,
            );
            bindings.push((button, app.id.clone()));
        }
    }

    // -- Tools folder -------------------------------------------------------
    let folder = thistle_ui_create_container(shell);
    thistle_ui_set_pos(folder, 0, 0);
    thistle_ui_set_layout(folder, LAYOUT_COLUMN);
    thistle_ui_set_flex_grow(folder, 1);
    thistle_ui_set_align(folder, ALIGN_CENTER, ALIGN_CENTER);

    // E-paper analogue of the LCD folder overlay: an opaque centred sheet.
    // A white sheet avoids expensive halftone/dimming while retaining the
    // same phone-style spatial model.
    let folder_frame = thistle_ui_create_container(folder);
    thistle_ui_set_layout(folder_frame, LAYOUT_COLUMN);
    thistle_ui_set_size(folder_frame, 218, 186);
    thistle_ui_set_align(folder_frame, ALIGN_CENTER, ALIGN_CENTER);
    thistle_ui_set_bg_color(folder_frame, text_color);
    thistle_ui_set_radius(folder_frame, 12);

    let folder_panel = thistle_ui_create_container(folder_frame);
    thistle_ui_set_layout(folder_panel, LAYOUT_COLUMN);
    thistle_ui_set_size(folder_panel, 216, 184);
    thistle_ui_set_bg_color(folder_panel, surface_color);
    thistle_ui_set_radius(folder_panel, 11);
    thistle_ui_set_padding(folder_panel, 6, 6, 6, 6);
    thistle_ui_set_gap(folder_panel, 0);
    create_view_header(folder_panel, b"Tools\0", surface_color, text_color, false);

    let folder_grid = thistle_ui_create_container(folder_panel);
    thistle_ui_set_layout(folder_grid, LAYOUT_COLUMN);
    thistle_ui_set_flex_grow(folder_grid, 1);
    thistle_ui_set_padding(folder_grid, 2, 2, 2, 2);
    thistle_ui_set_bg_color(folder_grid, surface_color);
    for chunk in tools.chunks(3) {
        let row = create_grid_row(folder_grid, bg_color);
        for idx in chunk {
            let app = &apps[*idx];
            let button = create_icon_button(
                row,
                app.label.as_ptr(),
                app.icon,
                FOLDER_TILE_WIDTH,
                FOLDER_TILE_HEIGHT,
                bg_color,
                text_color,
                launcher_on_press,
            );
            bindings.push((button, app.id.clone()));
        }
    }

    let mut state = LAUNCHER.lock().unwrap();
    state.app_buttons.clear();
    state.app_ids.clear();
    for (button, id) in bindings {
        state.app_buttons.push(button);
        state.app_ids.push(id);
    }

    state.shell = shell;
    state.home = home;
    state.drawer = drawer;
    state.folder = folder;
    state.clock = status_clock;
    state.status_clock = 0;
    state.drawer_visible = false;
    state.folder_visible = false;
    state.last_time = [0; 6];
    for (dst, src) in state
        .last_time
        .iter_mut()
        .take(5)
        .zip(status_time_buf.iter())
    {
        *dst = *src as u8;
    }
    drop(state);
    thistle_ui_set_visible(drawer, false);
    thistle_ui_set_visible(folder, false);

    ESP_OK
}

/// on_start: Launcher is becoming the foreground app
unsafe extern "C" fn on_start() {
    // No-op for now — the UI is already built in on_create
}

/// on_pause: Launcher is going to background
unsafe extern "C" fn on_pause() {
    let shell = LAUNCHER.lock().map(|state| state.shell).unwrap_or(0);
    if shell != 0 {
        thistle_ui_set_visible(shell, false);
    }
}

/// on_resume: Launcher is returning to foreground
unsafe extern "C" fn on_resume() {
    let shell = LAUNCHER.lock().map(|s| s.shell).unwrap_or(0);
    if shell != 0 {
        thistle_ui_set_visible(shell, true);
        show_view(PendingAction::ShowHome);
    }
}

/// on_destroy: Cleanup
unsafe extern "C" fn on_destroy() {
    if let Ok(mut state) = LAUNCHER.lock() {
        if state.shell != 0 {
            thistle_ui_destroy(state.shell);
        }
        state.shell = 0;
        state.home = 0;
        state.drawer = 0;
        state.folder = 0;
        state.status_clock = 0;
        state.clock = 0;
        state.drawer_visible = false;
        state.folder_visible = false;
        state.last_time = [0; 6];
        state.app_ids.clear();
        state.app_buttons.clear();
    }
}

// ---------------------------------------------------------------------------
// Static manifest and entry (must live for the lifetime of the kernel)
// ---------------------------------------------------------------------------

static MANIFEST: CAppManifest = CAppManifest {
    id: b"com.thistle.tk_launcher\0".as_ptr() as *const c_char,
    name: b"Launcher\0".as_ptr() as *const c_char,
    version: b"0.1.0\0".as_ptr() as *const c_char,
    allow_background: false,
    min_memory_kb: 0,
};

static ENTRY: CAppEntry = CAppEntry {
    on_create: Some(on_create),
    on_start: Some(on_start),
    on_pause: Some(on_pause),
    on_resume: Some(on_resume),
    on_destroy: Some(on_destroy),
    manifest: &MANIFEST as *const CAppManifest,
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
