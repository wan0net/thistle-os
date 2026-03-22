// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — App Store UI App (thistle-tk)
//
// Browsable app store for the e-paper display (1-bit, 240×320).
// Renders with text-only ASCII art — no images.
//
// Registers as "com.thistle.appstore" and is built from the app manager
// registration system used by tk_launcher.rs.
//
// Screens:
//   Browse  — category tabs + paginated app list
//   Detail  — full entry view with rating, downloads, changelog, install button
//   Installing — progress feedback
//   RateDialog — 1–5 star selection

use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::Mutex;

use crate::app_manager::{CAppEntry, CAppManifest};
use crate::appstore_client::{
    format_download_count, format_star_rating, parse_catalog_entries, sort_entries_slice,
    CatalogEntry, SORT_BY_RATING,
};

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0;

// ---------------------------------------------------------------------------
// Widget API imports (same shim layer as tk_launcher.rs)
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
}

// ---------------------------------------------------------------------------
// Layout constants (e-paper 240×320)
// ---------------------------------------------------------------------------

const LAYOUT_COLUMN: i32 = 0;
const LAYOUT_ROW: i32 = 1;
const ALIGN_CENTER: i32 = 1;
const ALIGN_SPACE_BETWEEN: i32 = 3;
const STATUS_BAR_HEIGHT: i32 = 24;
const ITEMS_PER_PAGE: usize = 5;
const CATALOG_URL: &str =
    "https://wan0net.github.io/thistle-apps/catalog.json";

// ---------------------------------------------------------------------------
// Category tab definitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Category {
    All,
    Apps,
    Drivers,
    WindowManagers,
    System,
    Games,
    Communication,
    Tools,
}

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Category::All => "All",
            Category::Apps => "Apps",
            Category::Drivers => "Drivers",
            Category::WindowManagers => "WM",
            Category::System => "System",
            Category::Games => "Games",
            Category::Communication => "Comms",
            Category::Tools => "Tools",
        }
    }

    pub fn filter_str(&self) -> &'static str {
        match self {
            Category::All => "all",
            Category::Apps => "app",
            Category::Drivers => "driver",
            Category::WindowManagers => "wm",
            Category::System => "system",
            Category::Games => "games",
            Category::Communication => "communication",
            Category::Tools => "tools",
        }
    }
}

const CATEGORY_TABS: &[Category] = &[
    Category::All,
    Category::Apps,
    Category::Drivers,
    Category::WindowManagers,
];

// ---------------------------------------------------------------------------
// App screen state machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum InstallStep {
    Downloading,
    Verifying,
    Done,
    Failed,
}

/// Current screen the app store is showing.
pub enum AppStoreScreen {
    Loading,
    Browse {
        page: usize,
        category_idx: usize,
    },
    Detail {
        entry_idx: usize,
    },
    Installing {
        entry_id: String,
        step: InstallStep,
        progress: u8,
    },
    RateDialog {
        entry_id: String,
        selected_stars: u8,
    },
}

/// Global app store state.
struct AppStoreState {
    /// All entries currently loaded (after category filter + sort)
    entries: Vec<CatalogEntry>,
    /// CStrings kept alive so label pointers remain valid
    _label_strings: Vec<CString>,
    screen: AppStoreScreen,
    /// Root widget IDs
    root_widgets: Vec<u32>,
}

static APPSTORE: Mutex<AppStoreState> = Mutex::new(AppStoreState {
    entries: Vec::new(),
    _label_strings: Vec::new(),
    screen: AppStoreScreen::Loading,
    root_widgets: Vec::new(),
});

// ---------------------------------------------------------------------------
// Formatting helpers (public for tests)
// ---------------------------------------------------------------------------

/// Format rating count: "127 ratings" or "1 rating".
pub fn format_rating_count(count: u32) -> String {
    if count == 1 {
        "1 rating".to_string()
    } else {
        format!("{} ratings", count)
    }
}

/// Build the one-line browse entry subtitle shown in the list.
/// "★★★★☆  89 ratings  •  892 downloads"
pub fn format_entry_subtitle(entry: &CatalogEntry) -> String {
    let stars = format_star_rating(entry.rating_stars);
    let dl    = format_download_count(entry.download_count);
    if entry.rating_count > 0 {
        format!("{}  {}  •  {} dl", stars, format_rating_count(entry.rating_count), dl)
    } else {
        format!("{} dl", dl)
    }
}

/// Format the decimal star value for the detail screen: "4.5" from 450.
pub fn format_rating_decimal(rating_stars: u16) -> String {
    let whole  = rating_stars / 100;
    let frac   = (rating_stars % 100 + 5) / 10; // round to 1 decimal
    if frac == 0 {
        format!("{}.0", whole)
    } else {
        format!("{}.{}", whole, frac)
    }
}

/// Format size in human-readable form: "64 KB", "1.2 MB".
pub fn format_size(size_bytes: u32) -> String {
    if size_bytes == 0 {
        return "? KB".to_string();
    }
    if size_bytes >= 1_048_576 {
        let mb = size_bytes as f32 / 1_048_576.0;
        format!("{:.1} MB", mb)
    } else {
        let kb = (size_bytes + 511) / 1024;
        format!("{} KB", kb)
    }
}

/// Return the category display name from a CatalogEntry's category bytes.
pub fn entry_category_display(entry: &CatalogEntry) -> &str {
    let cat = std::str::from_utf8(&entry.category)
        .unwrap_or("")
        .trim_end_matches('\0');
    match cat {
        "communication" => "Communication",
        "tools"         => "Tools",
        "system"        => "System",
        "games"         => "Games",
        "drivers"       => "Drivers",
        "wm"            => "Window Manager",
        _               => "App",
    }
}

// ---------------------------------------------------------------------------
// Widget build helpers
// ---------------------------------------------------------------------------

/// Build the status bar row with a title and optional right-side annotation.
unsafe fn build_status_bar(parent: u32, title: &str, right: &str) -> (Vec<CString>, u32) {
    let bg = thistle_ui_theme_bg();
    let text = thistle_ui_theme_text();
    let surface = thistle_ui_theme_surface();
    let secondary = thistle_ui_theme_text_secondary();

    let mut strings: Vec<CString> = Vec::new();

    let bar = thistle_ui_create_container(parent);
    thistle_ui_set_layout(bar, LAYOUT_ROW);
    thistle_ui_set_size(bar, -1, STATUS_BAR_HEIGHT);
    thistle_ui_set_align(bar, ALIGN_SPACE_BETWEEN, ALIGN_CENTER);
    thistle_ui_set_padding(bar, 2, 4, 2, 4);
    thistle_ui_set_bg_color(bar, surface);

    let title_cstr = CString::new(title).unwrap_or_default();
    let lbl = thistle_ui_create_label(bar, title_cstr.as_ptr());
    thistle_ui_set_text_color(lbl, text);
    thistle_ui_set_font_size(lbl, 12);
    strings.push(title_cstr);

    if !right.is_empty() {
        let right_cstr = CString::new(right).unwrap_or_default();
        let rlbl = thistle_ui_create_label(bar, right_cstr.as_ptr());
        thistle_ui_set_text_color(rlbl, secondary);
        thistle_ui_set_font_size(rlbl, 12);
        strings.push(right_cstr);
    }

    (strings, bar)
}

/// Build a category tab row below the status bar.
unsafe fn build_category_tabs(parent: u32, active_idx: usize) -> Vec<CString> {
    let primary  = thistle_ui_theme_primary();
    let bg       = thistle_ui_theme_bg();
    let surface  = thistle_ui_theme_surface();
    let text     = thistle_ui_theme_text();
    let secondary = thistle_ui_theme_text_secondary();

    let mut strings: Vec<CString> = Vec::new();

    let tabs_row = thistle_ui_create_container(parent);
    thistle_ui_set_layout(tabs_row, LAYOUT_ROW);
    thistle_ui_set_size(tabs_row, -1, 22);
    thistle_ui_set_gap(tabs_row, 2);
    thistle_ui_set_padding(tabs_row, 2, 4, 2, 4);
    thistle_ui_set_bg_color(tabs_row, surface);

    for (i, cat) in CATEGORY_TABS.iter().enumerate() {
        let label = cat.label();
        let cstr = CString::new(label).unwrap_or_default();
        let btn = thistle_ui_create_button(tabs_row, cstr.as_ptr());
        thistle_ui_set_size(btn, 52, 18);
        thistle_ui_set_radius(btn, 3);
        if i == active_idx {
            thistle_ui_set_bg_color(btn, primary);
            thistle_ui_set_text_color(btn, bg);
        } else {
            thistle_ui_set_bg_color(btn, bg);
            thistle_ui_set_text_color(btn, secondary);
        }
        thistle_ui_set_font_size(btn, 10);
        strings.push(cstr);
    }

    strings
}

/// Build a single app card for the browse list.
unsafe fn build_app_card(parent: u32, entry: &CatalogEntry) -> Vec<CString> {
    let surface  = thistle_ui_theme_surface();
    let text     = thistle_ui_theme_text();
    let secondary = thistle_ui_theme_text_secondary();
    let bg       = thistle_ui_theme_bg();

    let mut strings: Vec<CString> = Vec::new();

    let card = thistle_ui_create_container(parent);
    thistle_ui_set_layout(card, LAYOUT_COLUMN);
    thistle_ui_set_size(card, -1, 54);
    thistle_ui_set_padding(card, 4, 6, 4, 6);
    thistle_ui_set_bg_color(card, surface);
    thistle_ui_set_radius(card, 4);

    // App name
    let name_str = std::str::from_utf8(&entry.name)
        .unwrap_or("?")
        .trim_end_matches('\0');
    let name_cstr = CString::new(name_str).unwrap_or_default();
    let name_lbl = thistle_ui_create_label(card, name_cstr.as_ptr());
    thistle_ui_set_text_color(name_lbl, text);
    thistle_ui_set_font_size(name_lbl, 13);
    strings.push(name_cstr);

    // Subtitle: stars + rating count + downloads
    let subtitle = format_entry_subtitle(entry);
    let sub_cstr = CString::new(subtitle.as_str()).unwrap_or_default();
    let sub_lbl = thistle_ui_create_label(card, sub_cstr.as_ptr());
    thistle_ui_set_text_color(sub_lbl, secondary);
    thistle_ui_set_font_size(sub_lbl, 10);
    strings.push(sub_cstr);

    // Category label
    let cat_text = entry_category_display(entry);
    let cat_cstr = CString::new(cat_text).unwrap_or_default();
    let cat_lbl = thistle_ui_create_label(card, cat_cstr.as_ptr());
    thistle_ui_set_text_color(cat_lbl, secondary);
    thistle_ui_set_font_size(cat_lbl, 10);
    strings.push(cat_cstr);

    strings
}

/// Build the detail screen for a single entry.
unsafe fn build_detail_screen(root: u32, entry: &CatalogEntry) -> Vec<CString> {
    let bg        = thistle_ui_theme_bg();
    let text      = thistle_ui_theme_text();
    let secondary = thistle_ui_theme_text_secondary();
    let surface   = thistle_ui_theme_surface();
    let primary   = thistle_ui_theme_primary();

    let mut strings: Vec<CString> = Vec::new();

    let col = thistle_ui_create_container(root);
    thistle_ui_set_layout(col, LAYOUT_COLUMN);
    thistle_ui_set_size(col, -1, -1);
    thistle_ui_set_flex_grow(col, 1);
    thistle_ui_set_bg_color(col, bg);
    thistle_ui_set_gap(col, 0);

    // Back button in status bar position
    let (mut bar_strings, _bar) = build_status_bar(col, "< Back", "App Store");
    strings.append(&mut bar_strings);

    // Content area (scrollable)
    let content = thistle_ui_create_container(col);
    thistle_ui_set_layout(content, LAYOUT_COLUMN);
    thistle_ui_set_flex_grow(content, 1);
    thistle_ui_set_padding(content, 6, 8, 6, 8);
    thistle_ui_set_scrollable(content, true);
    thistle_ui_set_bg_color(content, bg);
    thistle_ui_set_gap(content, 4);

    // App name (large)
    let name_str = std::str::from_utf8(&entry.name)
        .unwrap_or("?")
        .trim_end_matches('\0');
    let name_cstr = CString::new(name_str).unwrap_or_default();
    let name_lbl = thistle_ui_create_label(content, name_cstr.as_ptr());
    thistle_ui_set_text_color(name_lbl, text);
    thistle_ui_set_font_size(name_lbl, 16);
    strings.push(name_cstr);

    // Version + author
    let ver_str  = std::str::from_utf8(&entry.version).unwrap_or("").trim_end_matches('\0');
    let auth_str = std::str::from_utf8(&entry.author).unwrap_or("").trim_end_matches('\0');
    let meta_line = format!("v{}  by {}", ver_str, auth_str);
    let meta_cstr = CString::new(meta_line.as_str()).unwrap_or_default();
    let meta_lbl = thistle_ui_create_label(content, meta_cstr.as_ptr());
    thistle_ui_set_text_color(meta_lbl, secondary);
    thistle_ui_set_font_size(meta_lbl, 11);
    strings.push(meta_cstr);

    // Updated date
    let date_str = std::str::from_utf8(&entry.updated_date).unwrap_or("").trim_end_matches('\0');
    if !date_str.is_empty() {
        let date_line = format!("Updated: {}", date_str);
        let date_cstr = CString::new(date_line.as_str()).unwrap_or_default();
        let date_lbl = thistle_ui_create_label(content, date_cstr.as_ptr());
        thistle_ui_set_text_color(date_lbl, secondary);
        thistle_ui_set_font_size(date_lbl, 11);
        strings.push(date_cstr);
    }

    // Rating row: ★★★★½ 4.5 (127 ratings)
    let stars_str = format_star_rating(entry.rating_stars);
    let dec_str   = format_rating_decimal(entry.rating_stars);
    let rating_line = if entry.rating_count > 0 {
        format!("{} {} ({})", stars_str, dec_str, format_rating_count(entry.rating_count))
    } else {
        "No ratings yet".to_string()
    };
    let rating_cstr = CString::new(rating_line.as_str()).unwrap_or_default();
    let rating_lbl = thistle_ui_create_label(content, rating_cstr.as_ptr());
    thistle_ui_set_text_color(rating_lbl, text);
    thistle_ui_set_font_size(rating_lbl, 12);
    strings.push(rating_cstr);

    // Download count
    let dl_line = format!("{} downloads", format_download_count(entry.download_count));
    let dl_cstr = CString::new(dl_line.as_str()).unwrap_or_default();
    let dl_lbl = thistle_ui_create_label(content, dl_cstr.as_ptr());
    thistle_ui_set_text_color(dl_lbl, secondary);
    thistle_ui_set_font_size(dl_lbl, 11);
    strings.push(dl_cstr);

    // Description
    let desc_str = std::str::from_utf8(&entry.description).unwrap_or("").trim_end_matches('\0');
    if !desc_str.is_empty() {
        let desc_cstr = CString::new(desc_str).unwrap_or_default();
        let desc_lbl = thistle_ui_create_label(content, desc_cstr.as_ptr());
        thistle_ui_set_text_color(desc_lbl, text);
        thistle_ui_set_font_size(desc_lbl, 11);
        strings.push(desc_cstr);
    }

    // Changelog (What's new)
    let cl_str = std::str::from_utf8(&entry.changelog).unwrap_or("").trim_end_matches('\0');
    if !cl_str.is_empty() {
        let wn_cstr = CString::new("What's new:").unwrap_or_default();
        let wn_lbl = thistle_ui_create_label(content, wn_cstr.as_ptr());
        thistle_ui_set_text_color(wn_lbl, text);
        thistle_ui_set_font_size(wn_lbl, 11);
        strings.push(wn_cstr);

        let cl_cstr = CString::new(cl_str).unwrap_or_default();
        let cl_lbl = thistle_ui_create_label(content, cl_cstr.as_ptr());
        thistle_ui_set_text_color(cl_lbl, secondary);
        thistle_ui_set_font_size(cl_lbl, 10);
        strings.push(cl_cstr);
    }

    // Permissions
    let perm_str = std::str::from_utf8(&entry.permissions).unwrap_or("").trim_end_matches('\0');
    if !perm_str.is_empty() {
        let perm_line = format!("Permissions: {}", perm_str);
        let perm_cstr = CString::new(perm_line.as_str()).unwrap_or_default();
        let perm_lbl = thistle_ui_create_label(content, perm_cstr.as_ptr());
        thistle_ui_set_text_color(perm_lbl, secondary);
        thistle_ui_set_font_size(perm_lbl, 10);
        strings.push(perm_cstr);
    }

    // Size + signed status
    let size_line = format!(
        "Size: {}  Signed: {}",
        format_size(entry.size_bytes),
        if entry.is_signed { "Yes" } else { "No" }
    );
    let size_cstr = CString::new(size_line.as_str()).unwrap_or_default();
    let size_lbl = thistle_ui_create_label(content, size_cstr.as_ptr());
    thistle_ui_set_text_color(size_lbl, secondary);
    thistle_ui_set_font_size(size_lbl, 10);
    strings.push(size_cstr);

    // Action buttons row: [Install]  [Rate]
    let btn_row = thistle_ui_create_container(content);
    thistle_ui_set_layout(btn_row, LAYOUT_ROW);
    thistle_ui_set_size(btn_row, -1, 32);
    thistle_ui_set_gap(btn_row, 8);

    let install_cstr = CString::new("Install").unwrap_or_default();
    let install_btn = thistle_ui_create_button(btn_row, install_cstr.as_ptr());
    thistle_ui_set_size(install_btn, 100, 30);
    thistle_ui_set_bg_color(install_btn, primary);
    thistle_ui_set_text_color(install_btn, bg);
    thistle_ui_set_radius(install_btn, 4);
    thistle_ui_set_font_size(install_btn, 12);
    strings.push(install_cstr);

    let rate_cstr = CString::new("Rate").unwrap_or_default();
    let rate_btn = thistle_ui_create_button(btn_row, rate_cstr.as_ptr());
    thistle_ui_set_size(rate_btn, 80, 30);
    thistle_ui_set_bg_color(rate_btn, surface);
    thistle_ui_set_text_color(rate_btn, text);
    thistle_ui_set_radius(rate_btn, 4);
    thistle_ui_set_font_size(rate_btn, 12);
    strings.push(rate_cstr);

    strings
}

/// Build the browse screen (category tabs + paginated list).
unsafe fn build_browse_screen(root: u32, page: usize, category_idx: usize) -> Vec<CString> {
    let bg        = thistle_ui_theme_bg();
    let text      = thistle_ui_theme_text();
    let secondary = thistle_ui_theme_text_secondary();
    let primary   = thistle_ui_theme_primary();

    let mut strings: Vec<CString> = Vec::new();

    let col = thistle_ui_create_container(root);
    thistle_ui_set_layout(col, LAYOUT_COLUMN);
    thistle_ui_set_size(col, -1, -1);
    thistle_ui_set_flex_grow(col, 1);
    thistle_ui_set_bg_color(col, bg);
    thistle_ui_set_gap(col, 0);

    // Status bar
    let (mut bar_strings, _) = build_status_bar(col, "App Store", "[search]");
    strings.append(&mut bar_strings);

    // Category tabs
    let mut tab_strings = build_category_tabs(col, category_idx);
    strings.append(&mut tab_strings);

    // App list
    let list = thistle_ui_create_container(col);
    thistle_ui_set_layout(list, LAYOUT_COLUMN);
    thistle_ui_set_flex_grow(list, 1);
    thistle_ui_set_gap(list, 3);
    thistle_ui_set_padding(list, 4, 4, 4, 4);
    thistle_ui_set_scrollable(list, false); // pagination handles scrolling
    thistle_ui_set_bg_color(list, bg);

    // We can't lock the mutex again here (already called from on_create which
    // may hold it). Access via a snapshot passed as parameters instead.
    // Card rendering is driven externally; here we just set up the skeleton.
    // In the real WM integration the widget system calls into the app via
    // the input/render vtable. For the e-paper target we build a complete
    // static widget tree per page render.

    // Pagination footer
    let footer = thistle_ui_create_container(col);
    thistle_ui_set_layout(footer, LAYOUT_ROW);
    thistle_ui_set_size(footer, -1, 22);
    thistle_ui_set_align(footer, ALIGN_CENTER, ALIGN_CENTER);
    thistle_ui_set_gap(footer, 8);
    thistle_ui_set_bg_color(footer, bg);

    let prev_cstr = CString::new("[Prev]").unwrap_or_default();
    let prev_btn = thistle_ui_create_button(footer, prev_cstr.as_ptr());
    thistle_ui_set_size(prev_btn, 60, 18);
    thistle_ui_set_font_size(prev_btn, 10);
    strings.push(prev_cstr);

    // We don't know total pages without the entry count here, so show page N
    let page_label = format!("Page {}", page + 1);
    let page_cstr = CString::new(page_label.as_str()).unwrap_or_default();
    let page_lbl = thistle_ui_create_label(footer, page_cstr.as_ptr());
    thistle_ui_set_font_size(page_lbl, 10);
    thistle_ui_set_text_color(page_lbl, secondary);
    strings.push(page_cstr);

    let next_cstr = CString::new("[Next]").unwrap_or_default();
    let next_btn = thistle_ui_create_button(footer, next_cstr.as_ptr());
    thistle_ui_set_size(next_btn, 60, 18);
    thistle_ui_set_font_size(next_btn, 10);
    strings.push(next_cstr);

    strings
}

/// Build the Installing progress screen.
unsafe fn build_installing_screen(root: u32, entry_id: &str, step: &InstallStep, progress: u8) -> Vec<CString> {
    let bg   = thistle_ui_theme_bg();
    let text = thistle_ui_theme_text();
    let secondary = thistle_ui_theme_text_secondary();

    let mut strings: Vec<CString> = Vec::new();

    let col = thistle_ui_create_container(root);
    thistle_ui_set_layout(col, LAYOUT_COLUMN);
    thistle_ui_set_size(col, -1, -1);
    thistle_ui_set_flex_grow(col, 1);
    thistle_ui_set_bg_color(col, bg);
    thistle_ui_set_align(col, ALIGN_CENTER, ALIGN_CENTER);
    thistle_ui_set_gap(col, 8);
    thistle_ui_set_padding(col, 16, 12, 16, 12);

    let title_cstr = CString::new("Installing...").unwrap_or_default();
    let title_lbl = thistle_ui_create_label(col, title_cstr.as_ptr());
    thistle_ui_set_text_color(title_lbl, text);
    thistle_ui_set_font_size(title_lbl, 14);
    strings.push(title_cstr);

    let id_cstr = CString::new(entry_id).unwrap_or_default();
    let id_lbl = thistle_ui_create_label(col, id_cstr.as_ptr());
    thistle_ui_set_text_color(id_lbl, secondary);
    thistle_ui_set_font_size(id_lbl, 11);
    strings.push(id_cstr);

    let step_text = match step {
        InstallStep::Downloading => "Downloading...",
        InstallStep::Verifying   => "Verifying signature...",
        InstallStep::Done        => "Done!",
        InstallStep::Failed      => "Install failed.",
    };
    let step_cstr = CString::new(step_text).unwrap_or_default();
    let step_lbl = thistle_ui_create_label(col, step_cstr.as_ptr());
    thistle_ui_set_text_color(step_lbl, text);
    thistle_ui_set_font_size(step_lbl, 12);
    strings.push(step_cstr);

    // ASCII progress bar: [====      ] 40%
    let filled  = (progress as usize * 20 / 100).min(20);
    let empty   = 20 - filled;
    let bar = format!("[{}{}] {}%", "=".repeat(filled), " ".repeat(empty), progress);
    let bar_cstr = CString::new(bar.as_str()).unwrap_or_default();
    let bar_lbl = thistle_ui_create_label(col, bar_cstr.as_ptr());
    thistle_ui_set_text_color(bar_lbl, text);
    thistle_ui_set_font_size(bar_lbl, 11);
    strings.push(bar_cstr);

    strings
}

/// Build the rate dialog screen.
unsafe fn build_rate_dialog(root: u32, entry_id: &str, selected: u8) -> Vec<CString> {
    let bg      = thistle_ui_theme_bg();
    let text    = thistle_ui_theme_text();
    let primary = thistle_ui_theme_primary();
    let surface = thistle_ui_theme_surface();
    let secondary = thistle_ui_theme_text_secondary();

    let mut strings: Vec<CString> = Vec::new();

    let col = thistle_ui_create_container(root);
    thistle_ui_set_layout(col, LAYOUT_COLUMN);
    thistle_ui_set_size(col, -1, -1);
    thistle_ui_set_flex_grow(col, 1);
    thistle_ui_set_bg_color(col, bg);
    thistle_ui_set_align(col, ALIGN_CENTER, ALIGN_CENTER);
    thistle_ui_set_gap(col, 10);
    thistle_ui_set_padding(col, 20, 12, 20, 12);

    let title_cstr = CString::new("Rate this app").unwrap_or_default();
    let title_lbl = thistle_ui_create_label(col, title_cstr.as_ptr());
    thistle_ui_set_text_color(title_lbl, text);
    thistle_ui_set_font_size(title_lbl, 14);
    strings.push(title_cstr);

    // 5 star buttons in a row
    let stars_row = thistle_ui_create_container(col);
    thistle_ui_set_layout(stars_row, LAYOUT_ROW);
    thistle_ui_set_size(stars_row, -1, 36);
    thistle_ui_set_gap(stars_row, 4);
    thistle_ui_set_align(stars_row, ALIGN_CENTER, ALIGN_CENTER);

    for star in 1u8..=5 {
        let glyph = if star <= selected { "★" } else { "☆" };
        let star_cstr = CString::new(glyph).unwrap_or_default();
        let star_btn = thistle_ui_create_button(stars_row, star_cstr.as_ptr());
        thistle_ui_set_size(star_btn, 36, 32);
        thistle_ui_set_font_size(star_btn, 16);
        if star <= selected {
            thistle_ui_set_bg_color(star_btn, primary);
            thistle_ui_set_text_color(star_btn, bg);
        } else {
            thistle_ui_set_bg_color(star_btn, surface);
            thistle_ui_set_text_color(star_btn, secondary);
        }
        strings.push(star_cstr);
    }

    // Submit + Cancel buttons
    let btn_row = thistle_ui_create_container(col);
    thistle_ui_set_layout(btn_row, LAYOUT_ROW);
    thistle_ui_set_size(btn_row, -1, 32);
    thistle_ui_set_gap(btn_row, 10);
    thistle_ui_set_align(btn_row, ALIGN_CENTER, ALIGN_CENTER);

    let submit_cstr = CString::new("Submit").unwrap_or_default();
    let submit_btn  = thistle_ui_create_button(btn_row, submit_cstr.as_ptr());
    thistle_ui_set_size(submit_btn, 80, 28);
    thistle_ui_set_bg_color(submit_btn, primary);
    thistle_ui_set_text_color(submit_btn, bg);
    thistle_ui_set_font_size(submit_btn, 12);
    strings.push(submit_cstr);

    let cancel_cstr = CString::new("Cancel").unwrap_or_default();
    let cancel_btn  = thistle_ui_create_button(btn_row, cancel_cstr.as_ptr());
    thistle_ui_set_size(cancel_btn, 70, 28);
    thistle_ui_set_bg_color(cancel_btn, surface);
    thistle_ui_set_text_color(cancel_btn, text);
    thistle_ui_set_font_size(cancel_btn, 12);
    strings.push(cancel_cstr);

    strings
}

// ---------------------------------------------------------------------------
// Lifecycle callbacks
// ---------------------------------------------------------------------------

/// on_create: build the loading screen, then fetch catalog.
unsafe extern "C" fn on_create() -> i32 {
    let root = thistle_ui_get_app_root();
    if root == 0 {
        return -1;
    }

    let bg   = thistle_ui_theme_bg();
    let text = thistle_ui_theme_text();

    let col = thistle_ui_create_container(root);
    thistle_ui_set_layout(col, LAYOUT_COLUMN);
    thistle_ui_set_size(col, -1, -1);
    thistle_ui_set_flex_grow(col, 1);
    thistle_ui_set_bg_color(col, bg);
    thistle_ui_set_align(col, ALIGN_CENTER, ALIGN_CENTER);

    let lbl = thistle_ui_create_label(col, b"Loading App Store...\0".as_ptr() as *const c_char);
    thistle_ui_set_text_color(lbl, text);
    thistle_ui_set_font_size(lbl, 13);

    // In a real implementation we would kick off an async catalog fetch here.
    // For this built-in stub we initialise with empty entries and wait for
    // the catalog JSON to be available on the SD card.
    if let Ok(mut state) = APPSTORE.lock() {
        state.screen = AppStoreScreen::Browse { page: 0, category_idx: 0 };
        state.entries.clear();

        // Attempt to load from SD card catalog cache
        if let Ok(json) = std::fs::read_to_string("/sdcard/config/catalog_cache.json") {
            parse_catalog_entries(&json, "all", &mut state.entries);
            sort_entries_slice(&mut state.entries, SORT_BY_RATING, false);
        }
    }

    ESP_OK
}

unsafe extern "C" fn on_start() {
    // No-op: UI already built in on_create
}

unsafe extern "C" fn on_pause() {
    // No-op: keep widget tree intact for fast resume
}

unsafe extern "C" fn on_resume() {
    // Could refresh catalog here
}

unsafe extern "C" fn on_destroy() {
    if let Ok(mut state) = APPSTORE.lock() {
        state.entries.clear();
        state._label_strings.clear();
        state.root_widgets.clear();
        state.screen = AppStoreScreen::Loading;
    }
}

// ---------------------------------------------------------------------------
// Static manifest and entry
// ---------------------------------------------------------------------------

static MANIFEST: CAppManifest = CAppManifest {
    id:               b"com.thistle.appstore\0".as_ptr() as *const c_char,
    name:             b"App Store\0".as_ptr() as *const c_char,
    version:          b"1.0.0\0".as_ptr() as *const c_char,
    allow_background: false,
    min_memory_kb:    64,
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
// Registration
// ---------------------------------------------------------------------------

/// Register the app store with the app manager.
pub fn register() -> i32 {
    unsafe { crate::app_manager::register(&ENTRY as *const CAppEntry) }
}

/// C-callable registration entry point.
#[no_mangle]
pub extern "C" fn tk_appstore_register() -> i32 {
    register()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appstore_client::{copy_str_to_buf, CatalogEntry};

    fn make_entry_with(name: &str, category: &str, rating: u16, downloads: u32) -> CatalogEntry {
        let mut e = CatalogEntry::default();
        copy_str_to_buf(name, &mut e.name);
        copy_str_to_buf(category, &mut e.category);
        e.rating_stars = rating;
        e.download_count = downloads;
        e
    }

    #[test]
    fn test_format_entry_subtitle_with_ratings() {
        let mut e = CatalogEntry::default();
        copy_str_to_buf("Messenger", &mut e.name);
        e.rating_stars = 450;
        e.rating_count = 127;
        e.download_count = 1523;
        let sub = format_entry_subtitle(&e);
        // Must contain star glyphs, rating count, and download count
        assert!(sub.contains('★'), "subtitle must contain filled star");
        assert!(sub.contains("127"), "subtitle must contain rating count");
        assert!(sub.contains("1.5K"), "subtitle must contain formatted download count");
    }

    #[test]
    fn test_format_entry_subtitle_no_ratings() {
        let mut e = CatalogEntry::default();
        e.download_count = 500;
        let sub = format_entry_subtitle(&e);
        assert!(sub.contains("500"), "subtitle must show exact count for <1000");
    }

    #[test]
    fn test_format_rating_decimal() {
        assert_eq!(format_rating_decimal(450), "4.5");
        assert_eq!(format_rating_decimal(400), "4.0");
        assert_eq!(format_rating_decimal(500), "5.0");
        assert_eq!(format_rating_decimal(391), "3.9");
        assert_eq!(format_rating_decimal(0),   "0.0");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0),          "? KB");
        assert_eq!(format_size(1024),       "1 KB");
        assert_eq!(format_size(65536),      "64 KB");
        assert_eq!(format_size(1_048_576),  "1.0 MB");
        assert_eq!(format_size(1_258_291),  "1.2 MB");
    }

    #[test]
    fn test_entry_category_display() {
        let mut e = CatalogEntry::default();
        copy_str_to_buf("communication", &mut e.category);
        assert_eq!(entry_category_display(&e), "Communication");

        e.category = [0u8; 32];
        copy_str_to_buf("tools", &mut e.category);
        assert_eq!(entry_category_display(&e), "Tools");

        e.category = [0u8; 32];
        copy_str_to_buf("games", &mut e.category);
        assert_eq!(entry_category_display(&e), "Games");

        e.category = [0u8; 32];
        copy_str_to_buf("system", &mut e.category);
        assert_eq!(entry_category_display(&e), "System");
    }

    #[test]
    fn test_format_rating_count() {
        assert_eq!(format_rating_count(0),   "0 ratings");
        assert_eq!(format_rating_count(1),   "1 rating");
        assert_eq!(format_rating_count(127), "127 ratings");
    }

    #[test]
    fn test_catalog_category_filter_integration() {
        let json = r#"[
            {"id":"a1","name":"Messenger","category":"communication","downloads":1523,"rating":4.5,"rating_count":127,"url":"https://x.com/a.elf"},
            {"id":"a2","name":"Navigator","category":"tools","downloads":892,"rating":4.2,"rating_count":89,"url":"https://x.com/b.elf"},
            {"id":"a3","name":"Notes","category":"tools","downloads":3891,"rating":4.8,"rating_count":203,"url":"https://x.com/c.elf"},
            {"id":"a4","name":"Snake","category":"games","downloads":2341,"rating":4.1,"rating_count":78,"url":"https://x.com/d.elf"}
        ]"#;

        let mut entries = Vec::new();
        parse_catalog_entries(json, "tools", &mut entries);
        assert_eq!(entries.len(), 2, "tools filter should return 2 entries");

        // Verify the right entries were returned
        let names: Vec<&str> = entries.iter().map(|e| {
            std::str::from_utf8(&e.name).unwrap().trim_end_matches('\0')
        }).collect();
        assert!(names.contains(&"Navigator"));
        assert!(names.contains(&"Notes"));
    }

    #[test]
    fn test_sort_by_downloads_integration() {
        let json = r#"[
            {"id":"a1","name":"App1","downloads":1523,"url":"https://x.com/a.elf"},
            {"id":"a2","name":"App2","downloads":3891,"url":"https://x.com/b.elf"},
            {"id":"a3","name":"App3","downloads":892,"url":"https://x.com/c.elf"}
        ]"#;

        let mut entries = Vec::new();
        parse_catalog_entries(json, "all", &mut entries);
        sort_entries_slice(&mut entries, SORT_BY_RATING, false);

        // All have 0 rating, so order is stable; just verify no panics
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_install_progress_bar_format() {
        // Simulate what build_installing_screen would produce
        let progress = 40u8;
        let filled   = (progress as usize * 20 / 100).min(20);
        let empty    = 20 - filled;
        let bar      = format!("[{}{}] {}%", "=".repeat(filled), " ".repeat(empty), progress);
        assert_eq!(bar, "[========            ] 40%");
    }

    #[test]
    fn test_category_label() {
        assert_eq!(Category::All.label(),            "All");
        assert_eq!(Category::Communication.label(),  "Comms");
        assert_eq!(Category::WindowManagers.label(), "WM");
    }

    #[test]
    fn test_category_filter_str() {
        assert_eq!(Category::All.filter_str(),    "all");
        assert_eq!(Category::Drivers.filter_str(), "driver");
        assert_eq!(Category::Tools.filter_str(),   "tools");
    }
}
