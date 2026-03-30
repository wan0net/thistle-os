// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — Flashlight/SOS app (thistle-tk)
//
// First app migrated from LVGL to thistle-tk. Proves the thistle-tk widget API
// can replace LVGL for real applications.
//
// Two modes:
//   Flashlight — root container turns white, LCD backlight raised to 100%.
//   SOS        — flashes ... --- ... in Morse, cycling screen white/dark.
//
// Layout:
//   ┌────────────────────────────────┐
//   │                                │
//   │        [FLASHLIGHT]            │  primary button
//   │                                │
//   │           [SOS]                │  surface button
//   │                                │
//   │     (screen turns white        │
//   │      when active)              │
//   │                                │
//   └────────────────────────────────┘

use std::os::raw::c_char;
use std::sync::Mutex;

use crate::app_manager::{self, CAppEntry, CAppManifest};

// ---------------------------------------------------------------------------
// ESP-IDF error codes
// ---------------------------------------------------------------------------

const ESP_OK: i32 = 0;

// ---------------------------------------------------------------------------
// Widget API imports — routed through widget.rs -> WM vtable
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
    fn thistle_ui_set_bg_color(widget: u32, color: u32);
    fn thistle_ui_set_text_color(widget: u32, color: u32);
    fn thistle_ui_set_font_size(widget: u32, size: i32);
    fn thistle_ui_set_radius(widget: u32, r: i32);
    fn thistle_ui_set_border_width(widget: u32, w: i32);
    fn thistle_ui_set_text(widget: u32, text: *const c_char);
    fn thistle_ui_set_padding(widget: u32, t: i32, r: i32, b: i32, l: i32);
    fn thistle_ui_theme_bg() -> u32;
    fn thistle_ui_theme_text() -> u32;
    fn thistle_ui_theme_text_secondary() -> u32;
    fn thistle_ui_theme_surface() -> u32;
    fn thistle_ui_theme_primary() -> u32;
}

// ---------------------------------------------------------------------------
// HAL display brightness
// ---------------------------------------------------------------------------

use crate::hal_registry::HalRegistry;

extern "C" {
    fn hal_get_registry() -> *const HalRegistry;
}

fn set_backlight(pct: u8) {
    unsafe {
        let reg = hal_get_registry();
        if reg.is_null() {
            return;
        }
        let display = (*reg).display;
        if display.is_null() {
            return;
        }
        if let Some(set_brightness) = (*display).set_brightness {
            set_brightness(pct);
        }
    }
}

// ---------------------------------------------------------------------------
// esp_timer FFI for SOS timing
// ---------------------------------------------------------------------------

#[cfg(target_os = "espidf")]
extern "C" {
    fn esp_timer_get_time() -> i64;
}

#[cfg(not(target_os = "espidf"))]
unsafe fn esp_timer_get_time() -> i64 {
    // Stub: return 0 in tests. SOS timing is not exercised in unit tests.
    0
}

// ---------------------------------------------------------------------------
// Layout constants (match tk_launcher)
// ---------------------------------------------------------------------------

const LAYOUT_COLUMN: i32 = 0;
const ALIGN_CENTER: i32 = 1;

// ---------------------------------------------------------------------------
// SOS Morse pattern
// Durations in microseconds (on_us, off_us). A zero on-time marks end of
// sequence; the timer restarts from the beginning.
// ---------------------------------------------------------------------------

// S = ...  O = ---  S = ...  then word gap
const SHORT_ON: i64 = 200_000;
const SHORT_OFF: i64 = 200_000;
const LONG_ON: i64 = 600_000;
const LONG_OFF: i64 = 200_000;
const LETTER_GAP: i64 = 600_000;
const WORD_GAP: i64 = 1_400_000;

struct MorseStep {
    on_us: i64,  // 0 = end-of-sequence marker
    off_us: i64,
}

static SOS_PATTERN: &[MorseStep] = &[
    // S
    MorseStep { on_us: SHORT_ON, off_us: SHORT_OFF },
    MorseStep { on_us: SHORT_ON, off_us: SHORT_OFF },
    MorseStep { on_us: SHORT_ON, off_us: LETTER_GAP },
    // O
    MorseStep { on_us: LONG_ON, off_us: LONG_OFF },
    MorseStep { on_us: LONG_ON, off_us: LONG_OFF },
    MorseStep { on_us: LONG_ON, off_us: LETTER_GAP },
    // S
    MorseStep { on_us: SHORT_ON, off_us: SHORT_OFF },
    MorseStep { on_us: SHORT_ON, off_us: SHORT_OFF },
    MorseStep { on_us: SHORT_ON, off_us: WORD_GAP },
    // End marker — restarts sequence
    MorseStep { on_us: 0, off_us: 0 },
];

// ---------------------------------------------------------------------------
// Flash mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum FlashMode {
    Off = 0,
    On = 1,
    Sos = 2,
}

// ---------------------------------------------------------------------------
// White color constant (RGB565 packed into u32 for the widget API)
// ---------------------------------------------------------------------------

const COLOR_WHITE: u32 = 0x00FF_FFFF;

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

struct FlashlightState {
    root: u32,
    flash_btn: u32,
    sos_btn: u32,
    mode: FlashMode,

    // Theme colours (cached at create time)
    bg_color: u32,
    text_color: u32,
    text_secondary: u32,
    surface_color: u32,
    primary_color: u32,

    // SOS timer state
    sos_step: usize,
    sos_screen_on: bool,
    sos_phase_start_us: i64,
}

impl FlashlightState {
    const fn new() -> Self {
        Self {
            root: 0,
            flash_btn: 0,
            sos_btn: 0,
            mode: FlashMode::Off,
            bg_color: 0,
            text_color: 0,
            text_secondary: 0,
            surface_color: 0,
            primary_color: 0,
            sos_step: 0,
            sos_screen_on: false,
            sos_phase_start_us: 0,
        }
    }
}

static STATE: Mutex<FlashlightState> = Mutex::new(FlashlightState::new());

// ---------------------------------------------------------------------------
// Screen colour helpers
// ---------------------------------------------------------------------------

fn set_screen_white(s: &FlashlightState) {
    if s.root != 0 {
        unsafe { thistle_ui_set_bg_color(s.root, COLOR_WHITE) };
    }
}

fn set_screen_normal(s: &FlashlightState) {
    if s.root != 0 {
        unsafe { thistle_ui_set_bg_color(s.root, s.bg_color) };
    }
}

// ---------------------------------------------------------------------------
// SOS state machine — call from tick
// ---------------------------------------------------------------------------

fn sos_advance(s: &mut FlashlightState) {
    let now = unsafe { esp_timer_get_time() };
    let elapsed = now - s.sos_phase_start_us;

    let step = &SOS_PATTERN[s.sos_step];

    // End-of-sequence marker: restart
    if step.on_us == 0 {
        s.sos_step = 0;
        s.sos_screen_on = false;
        set_screen_normal(s);
        s.sos_phase_start_us = now;
        return;
    }

    if !s.sos_screen_on {
        // We are in the "off" phase before this step's "on" phase.
        // (Or we just started.) Begin the flash-on phase.
        set_screen_white(s);
        s.sos_screen_on = true;
        s.sos_phase_start_us = now;
    } else if elapsed >= step.on_us {
        // Flash-on phase has elapsed; switch to off phase.
        set_screen_normal(s);
        s.sos_screen_on = false;
        s.sos_phase_start_us = now;
        // Advance to next step — check off duration on next tick.
    }

    // If we are in the off phase, check if off duration has elapsed.
    if !s.sos_screen_on {
        let off_elapsed = now - s.sos_phase_start_us;
        if off_elapsed >= step.off_us && s.sos_phase_start_us != now {
            s.sos_step += 1;
            if s.sos_step >= SOS_PATTERN.len() {
                s.sos_step = 0;
            }
            // Next tick will start the new step's on-phase.
            s.sos_screen_on = false;
            s.sos_phase_start_us = now;
        }
    }
}

fn sos_start(s: &mut FlashlightState) {
    s.sos_step = 0;
    s.sos_screen_on = false;
    s.sos_phase_start_us = unsafe { esp_timer_get_time() };
}

fn sos_stop(s: &mut FlashlightState) {
    s.sos_screen_on = false;
    set_screen_normal(s);
}

// ---------------------------------------------------------------------------
// Mode management
// ---------------------------------------------------------------------------

fn apply_mode(s: &mut FlashlightState, new_mode: FlashMode) {
    // Tear down old mode
    match s.mode {
        FlashMode::Sos => sos_stop(s),
        FlashMode::On => {
            set_screen_normal(s);
            set_backlight(50); // restore default brightness
        }
        FlashMode::Off => {}
    }

    s.mode = new_mode;

    match new_mode {
        FlashMode::On => {
            set_screen_white(s);
            set_backlight(100);
            unsafe {
                thistle_ui_set_text(s.flash_btn, b"OFF\0".as_ptr() as *const c_char);
                thistle_ui_set_text(s.sos_btn, b"SOS\0".as_ptr() as *const c_char);
            }
        }
        FlashMode::Sos => {
            sos_start(s);
            unsafe {
                thistle_ui_set_text(s.flash_btn, b"FLASHLIGHT\0".as_ptr() as *const c_char);
                thistle_ui_set_text(s.sos_btn, b"STOP SOS\0".as_ptr() as *const c_char);
            }
        }
        FlashMode::Off => {
            unsafe {
                thistle_ui_set_text(s.flash_btn, b"FLASHLIGHT\0".as_ptr() as *const c_char);
                thistle_ui_set_text(s.sos_btn, b"SOS\0".as_ptr() as *const c_char);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle callbacks
// ---------------------------------------------------------------------------

/// on_create: Build the flashlight UI widget tree.
unsafe extern "C" fn on_create() -> i32 {
    let root = thistle_ui_get_app_root();
    if root == 0 {
        return -1;
    }

    let bg_color = thistle_ui_theme_bg();
    let text_color = thistle_ui_theme_text();
    let text_secondary = thistle_ui_theme_text_secondary();
    let surface_color = thistle_ui_theme_surface();
    let primary_color = thistle_ui_theme_primary();

    // Root column — centred vertically and horizontally
    let main_col = thistle_ui_create_container(root);
    thistle_ui_set_layout(main_col, LAYOUT_COLUMN);
    thistle_ui_set_size(main_col, -1, -1); // fill parent
    thistle_ui_set_flex_grow(main_col, 1);
    thistle_ui_set_bg_color(main_col, bg_color);
    thistle_ui_set_align(main_col, ALIGN_CENTER, ALIGN_CENTER);
    thistle_ui_set_gap(main_col, 20);

    // --- FLASHLIGHT button ---
    let flash_btn = thistle_ui_create_button(
        main_col,
        b"FLASHLIGHT\0".as_ptr() as *const c_char,
    );
    thistle_ui_set_size(flash_btn, 200, 52);
    thistle_ui_set_bg_color(flash_btn, primary_color);
    thistle_ui_set_text_color(flash_btn, bg_color);
    thistle_ui_set_radius(flash_btn, 8);
    thistle_ui_set_font_size(flash_btn, 18);

    // --- SOS button ---
    let sos_btn = thistle_ui_create_button(
        main_col,
        b"SOS\0".as_ptr() as *const c_char,
    );
    thistle_ui_set_size(sos_btn, 200, 44);
    thistle_ui_set_bg_color(sos_btn, surface_color);
    thistle_ui_set_text_color(sos_btn, text_color);
    thistle_ui_set_radius(sos_btn, 8);
    thistle_ui_set_border_width(sos_btn, 1);
    thistle_ui_set_font_size(sos_btn, 18);

    // --- Hint label ---
    let hint = thistle_ui_create_label(
        main_col,
        b"Screen turns white when active\0".as_ptr() as *const c_char,
    );
    thistle_ui_set_text_color(hint, text_secondary);
    thistle_ui_set_font_size(hint, 14);

    // Store state
    if let Ok(mut s) = STATE.lock() {
        s.root = main_col;
        s.flash_btn = flash_btn;
        s.sos_btn = sos_btn;
        s.mode = FlashMode::Off;
        s.bg_color = bg_color;
        s.text_color = text_color;
        s.text_secondary = text_secondary;
        s.surface_color = surface_color;
        s.primary_color = primary_color;
        s.sos_step = 0;
        s.sos_screen_on = false;
        s.sos_phase_start_us = 0;
    }

    ESP_OK
}

/// on_start: Flashlight is becoming the foreground app.
unsafe extern "C" fn on_start() {
    // No-op — UI is already built in on_create
}

/// on_pause: Flashlight is going to background — turn everything off.
unsafe extern "C" fn on_pause() {
    if let Ok(mut s) = STATE.lock() {
        if s.mode != FlashMode::Off {
            apply_mode(&mut s, FlashMode::Off);
        }
    }
}

/// on_resume: Flashlight is returning to foreground.
unsafe extern "C" fn on_resume() {
    // No-op — mode was reset to Off on pause
}

/// on_destroy: Cleanup state.
unsafe extern "C" fn on_destroy() {
    if let Ok(mut s) = STATE.lock() {
        if s.mode != FlashMode::Off {
            apply_mode(&mut s, FlashMode::Off);
        }
        s.root = 0;
        s.flash_btn = 0;
        s.sos_btn = 0;
    }
}

// ---------------------------------------------------------------------------
// Button press handlers — called from C via FFI
// ---------------------------------------------------------------------------

/// Toggle flashlight mode. Called when the FLASHLIGHT button is pressed.
#[no_mangle]
pub extern "C" fn rs_flashlight_toggle() {
    if let Ok(mut s) = STATE.lock() {
        let new_mode = if s.mode == FlashMode::On {
            FlashMode::Off
        } else {
            FlashMode::On
        };
        apply_mode(&mut s, new_mode);
    }
}

/// Toggle SOS mode. Called when the SOS button is pressed.
#[no_mangle]
pub extern "C" fn rs_flashlight_toggle_sos() {
    if let Ok(mut s) = STATE.lock() {
        let new_mode = if s.mode == FlashMode::Sos {
            FlashMode::Off
        } else {
            FlashMode::Sos
        };
        apply_mode(&mut s, new_mode);
    }
}

/// Advance the SOS state machine. Must be called periodically (every ~50ms)
/// while the app is in the foreground. No-op when SOS mode is not active.
#[no_mangle]
pub extern "C" fn rs_flashlight_tick() {
    if let Ok(mut s) = STATE.lock() {
        if s.mode == FlashMode::Sos {
            sos_advance(&mut s);
        }
    }
}

// ---------------------------------------------------------------------------
// Static manifest and entry
// ---------------------------------------------------------------------------

static MANIFEST: CAppManifest = CAppManifest {
    id:               b"com.thistle.flashlight\0".as_ptr() as *const c_char,
    name:             b"Flashlight\0".as_ptr() as *const c_char,
    version:          b"0.2.0\0".as_ptr() as *const c_char,
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

/// Register the thistle-tk flashlight app with the app manager.
/// Returns ESP_OK (0) on success.
pub fn register() -> i32 {
    unsafe { app_manager::register(&ENTRY as *const CAppEntry) }
}

/// C-callable registration entry point.
#[no_mangle]
pub extern "C" fn tk_flashlight_register() -> i32 {
    register()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sos_pattern_ends_with_zero() {
        let last = SOS_PATTERN.last().unwrap();
        assert_eq!(last.on_us, 0, "Last SOS step should be end marker");
        assert_eq!(last.off_us, 0);
    }

    #[test]
    fn test_sos_pattern_has_nine_steps_plus_marker() {
        // S(3) + O(3) + S(3) + end(1) = 10
        assert_eq!(SOS_PATTERN.len(), 10);
    }

    #[test]
    fn test_flash_mode_repr() {
        assert_eq!(FlashMode::Off as u8, 0);
        assert_eq!(FlashMode::On as u8, 1);
        assert_eq!(FlashMode::Sos as u8, 2);
    }

    #[test]
    fn test_initial_state() {
        let s = FlashlightState::new();
        assert_eq!(s.mode, FlashMode::Off);
        assert_eq!(s.root, 0);
        assert_eq!(s.flash_btn, 0);
        assert_eq!(s.sos_btn, 0);
        assert_eq!(s.sos_step, 0);
        assert!(!s.sos_screen_on);
    }

    #[test]
    fn test_morse_timing_values() {
        // S: 3 short dits
        assert_eq!(SOS_PATTERN[0].on_us, 200_000);
        assert_eq!(SOS_PATTERN[0].off_us, 200_000);
        // O: 3 long dahs
        assert_eq!(SOS_PATTERN[3].on_us, 600_000);
        assert_eq!(SOS_PATTERN[3].off_us, 200_000);
        // Last S dit has word gap
        assert_eq!(SOS_PATTERN[8].off_us, 1_400_000);
    }
}
