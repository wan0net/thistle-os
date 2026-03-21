// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#pragma once

#include "esp_err.h"
#include "hal/display.h"
#include "hal/input.h"
#include <stdint.h>
#include <stdbool.h>

/*
 * ThistleOS Display Server
 *
 * Architecture (like Linux's Wayland/X11):
 *
 *   ┌─────────────────────────────────────────────┐
 *   │  Apps (use WM's toolkit API)                │
 *   ├─────────────────────────────────────────────┤
 *   │  Window Manager (.wm.elf — swappable)       │
 *   │  - Status bar, launcher, theme engine       │
 *   │  - Widget toolkit (LVGL, Rust UI, terminal) │
 *   ├─────────────────────────────────────────────┤
 *   │  Display Server (kernel — immutable)        │
 *   │  - Surface management (framebuffers)        │
 *   │  - Input event routing                      │
 *   │  - Compositor (dirty region tracking)       │
 *   │  - Display mode (e-paper vs LCD)            │
 *   ├─────────────────────────────────────────────┤
 *   │  HAL (display + input drivers)              │
 *   └─────────────────────────────────────────────┘
 *
 * The display server owns the framebuffer and provides surfaces
 * to the window manager. The WM draws into surfaces using whatever
 * toolkit it wants. The display server composites surfaces and
 * flushes to the hardware display driver.
 */

/* ── Surface: a rectangular drawing area ────────────────────────────── */

typedef uint32_t surface_id_t;
#define SURFACE_INVALID ((surface_id_t)0)

typedef enum {
    SURFACE_ROLE_BACKGROUND,   /* Wallpaper / desktop */
    SURFACE_ROLE_STATUS_BAR,   /* Top status bar */
    SURFACE_ROLE_APP_CONTENT,  /* Main app content area */
    SURFACE_ROLE_OVERLAY,      /* Popups, app switcher, toasts */
    SURFACE_ROLE_DOCK,         /* Bottom app dock */
} surface_role_t;

typedef struct {
    uint16_t x, y;             /* Position on screen */
    uint16_t width, height;    /* Size in pixels */
    surface_role_t role;       /* Z-ordering role */
    bool visible;              /* Whether this surface is rendered */
} surface_info_t;

/* ── Window Manager interface (vtable — implemented by loaded WM) ─── */

typedef struct {
    /* Lifecycle */
    esp_err_t (*init)(void);                    /* Called after WM is loaded */
    void (*deinit)(void);                       /* Called before WM is unloaded */

    /* Display */
    void (*render)(void);                       /* Called each frame to render all surfaces */
    void (*on_theme_changed)(const char *theme_path);  /* Theme switch notification */

    /* App lifecycle hooks */
    void (*on_app_launched)(const char *app_id, surface_id_t surface);
    void (*on_app_stopped)(const char *app_id);
    void (*on_app_switched)(const char *app_id);

    /* Input — WM gets first crack at input before apps */
    bool (*on_input)(const hal_input_event_t *event);  /* Return true to consume */

    /* Widget API — the WM implements these for apps to build UI.
     * Any function left NULL means that widget type is unsupported. */
    uint32_t (*widget_get_app_root)(void);
    uint32_t (*widget_create_container)(uint32_t parent);
    uint32_t (*widget_create_label)(uint32_t parent, const char *text);
    uint32_t (*widget_create_button)(uint32_t parent, const char *text);
    uint32_t (*widget_create_text_input)(uint32_t parent, const char *placeholder);
    void     (*widget_destroy)(uint32_t widget);
    void     (*widget_set_text)(uint32_t widget, const char *text);
    const char *(*widget_get_text)(uint32_t widget);
    void     (*widget_set_size)(uint32_t widget, int w, int h);
    void     (*widget_set_pos)(uint32_t widget, int x, int y);
    void     (*widget_set_visible)(uint32_t widget, bool visible);
    void     (*widget_set_bg_color)(uint32_t widget, uint32_t color);
    void     (*widget_set_text_color)(uint32_t widget, uint32_t color);
    void     (*widget_set_font_size)(uint32_t widget, int size);
    void     (*widget_set_layout)(uint32_t widget, int layout);
    void     (*widget_set_align)(uint32_t widget, int main_align, int cross_align);
    void     (*widget_set_gap)(uint32_t widget, int gap);
    void     (*widget_set_flex_grow)(uint32_t widget, int grow);
    void     (*widget_set_scrollable)(uint32_t widget, bool scrollable);
    void     (*widget_set_padding)(uint32_t widget, int t, int r, int b, int l);
    void     (*widget_set_border_width)(uint32_t widget, int w);
    void     (*widget_set_radius)(uint32_t widget, int r);
    void     (*widget_on_event)(uint32_t widget, int event_type, void (*cb)(uint32_t, int, void*), void *ud);
    void     (*widget_set_password_mode)(uint32_t widget, bool pw);
    void     (*widget_set_one_line)(uint32_t widget, bool one_line);
    void     (*widget_set_placeholder)(uint32_t widget, const char *text);
    uint32_t (*widget_theme_primary)(void);
    uint32_t (*widget_theme_bg)(void);
    uint32_t (*widget_theme_surface)(void);
    uint32_t (*widget_theme_text)(void);
    uint32_t (*widget_theme_text_secondary)(void);

    /* Info */
    const char *name;          /* "lvgl-wm", "rust-wm", "terminal-wm" */
    const char *version;
} display_server_wm_t;

/* ── Display Server public API ──────────────────────────────────────── */

/* Initialize the display server (called by kernel at boot) */
esp_err_t display_server_init(void);

/* Load and activate a window manager from an ELF path.
 * If a WM is already active, it is deinited first. */
esp_err_t display_server_load_wm(const char *wm_elf_path);

/* Register a compiled-in WM (for backward compat with LVGL) */
esp_err_t display_server_register_wm(const display_server_wm_t *wm);

/* Get the active WM name (or NULL if none) */
const char *display_server_get_wm_name(void);

/* ── Surface management ─────────────────────────────────────────────── */

/* Create a surface. Returns a surface ID.
 * The framebuffer is allocated in PSRAM. */
surface_id_t display_server_create_surface(const surface_info_t *info);

/* Destroy a surface */
void display_server_destroy_surface(surface_id_t id);

/* Get the framebuffer pointer for direct pixel writing.
 * Format: RGB565 (16-bit) for LCD, 1-bit packed for e-paper.
 * Returns NULL on invalid ID. */
uint8_t *display_server_get_buffer(surface_id_t id);

/* Get surface info */
const surface_info_t *display_server_get_info(surface_id_t id);

/* Mark a region of the surface as dirty (needs redraw) */
void display_server_mark_dirty(surface_id_t id, const hal_area_t *area);

/* Mark the entire surface as dirty */
void display_server_mark_dirty_full(surface_id_t id);

/* Show/hide a surface */
void display_server_set_visible(surface_id_t id, bool visible);

/* ── Compositor ─────────────────────────────────────────────────────── */

/* Composite all visible surfaces and flush to display.
 * Called by the WM's render() or by the display server tick. */
esp_err_t display_server_composite(void);

/* Get the display dimensions */
uint16_t display_server_get_width(void);
uint16_t display_server_get_height(void);

/* Get display type (LCD vs e-paper — WM adapts its rendering) */
hal_display_type_t display_server_get_display_type(void);

/* ── Input routing ──────────────────────────────────────────────────── */

/* Input callback type for apps */
typedef void (*ds_input_cb_t)(const hal_input_event_t *event, void *user_data);

/* Register an input listener for a surface.
 * Input events that land within the surface's bounds are dispatched to it. */
esp_err_t display_server_surface_input_cb(surface_id_t id, ds_input_cb_t cb, void *user_data);

/* ── Tick ───────────────────────────────────────────────────────────── */

/* Called by the kernel main loop. Drives WM render + compositor. */
void display_server_tick(void);
