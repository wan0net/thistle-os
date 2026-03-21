// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Widget API — toolkit-agnostic UI primitives
//
// Apps call these functions to build their UI. The active window manager
// implements them using whatever toolkit it has (LVGL, embedded-graphics,
// terminal text). Apps never touch LVGL or any toolkit directly.
//
// Widgets are identified by opaque handles (uint32_t). The WM maps
// these to its internal objects (lv_obj_t*, etc.).

#pragma once

#include <stdint.h>
#include <stdbool.h>

typedef uint32_t thistle_widget_t;
#define THISTLE_WIDGET_NONE ((thistle_widget_t)0)

// ── Widget creation ─────────────────────────────────────────────────

// Get the app's root container (provided by the WM for the foreground app)
thistle_widget_t thistle_ui_get_app_root(void);

// Create widgets
thistle_widget_t thistle_ui_create_container(thistle_widget_t parent);
thistle_widget_t thistle_ui_create_label(thistle_widget_t parent, const char *text);
thistle_widget_t thistle_ui_create_button(thistle_widget_t parent, const char *text);
thistle_widget_t thistle_ui_create_text_input(thistle_widget_t parent, const char *placeholder);
thistle_widget_t thistle_ui_create_image(thistle_widget_t parent, const void *data, uint32_t len);

// Destroy a widget and all its children
void thistle_ui_destroy(thistle_widget_t widget);

// ── Widget properties ───────────────────────────────────────────────

void thistle_ui_set_text(thistle_widget_t widget, const char *text);
const char *thistle_ui_get_text(thistle_widget_t widget);

void thistle_ui_set_size(thistle_widget_t widget, int width, int height);
void thistle_ui_set_pos(thistle_widget_t widget, int x, int y);
void thistle_ui_set_visible(thistle_widget_t widget, bool visible);
void thistle_ui_set_enabled(thistle_widget_t widget, bool enabled);

// Colors as 0xRRGGBB
void thistle_ui_set_bg_color(thistle_widget_t widget, uint32_t color);
void thistle_ui_set_text_color(thistle_widget_t widget, uint32_t color);
void thistle_ui_set_border_color(thistle_widget_t widget, uint32_t color);

// Typography
#define THISTLE_FONT_SMALL  14
#define THISTLE_FONT_NORMAL 18
#define THISTLE_FONT_LARGE  22
void thistle_ui_set_font_size(thistle_widget_t widget, int size);

// Spacing
void thistle_ui_set_padding(thistle_widget_t widget, int top, int right, int bottom, int left);
void thistle_ui_set_border_width(thistle_widget_t widget, int width);
void thistle_ui_set_radius(thistle_widget_t widget, int radius);

// ── Layout ──────────────────────────────────────────────────────────

typedef enum {
    THISTLE_LAYOUT_NONE,
    THISTLE_LAYOUT_FLEX_COLUMN,
    THISTLE_LAYOUT_FLEX_ROW,
} thistle_layout_t;

typedef enum {
    THISTLE_ALIGN_START,
    THISTLE_ALIGN_CENTER,
    THISTLE_ALIGN_END,
    THISTLE_ALIGN_SPACE_BETWEEN,
} thistle_align_t;

void thistle_ui_set_layout(thistle_widget_t widget, thistle_layout_t layout);
void thistle_ui_set_align(thistle_widget_t widget, thistle_align_t main, thistle_align_t cross);
void thistle_ui_set_gap(thistle_widget_t widget, int gap);
void thistle_ui_set_flex_grow(thistle_widget_t widget, int grow);
void thistle_ui_set_scrollable(thistle_widget_t widget, bool scrollable);

// Size constants
#define THISTLE_SIZE_FULL   (-1)  // 100% of parent
#define THISTLE_SIZE_CONTENT (-2) // fit to content

// ── Events ──────────────────────────────────────────────────────────

typedef enum {
    THISTLE_EVENT_CLICK,
    THISTLE_EVENT_VALUE_CHANGED,
    THISTLE_EVENT_KEY,
} thistle_event_type_t;

typedef void (*thistle_event_cb_t)(thistle_widget_t widget, thistle_event_type_t event, void *user_data);

void thistle_ui_on_event(thistle_widget_t widget, thistle_event_type_t event,
                          thistle_event_cb_t callback, void *user_data);

// ── Text input specific ─────────────────────────────────────────────

void thistle_ui_set_password_mode(thistle_widget_t widget, bool password);
void thistle_ui_set_one_line(thistle_widget_t widget, bool one_line);
void thistle_ui_set_placeholder(thistle_widget_t widget, const char *text);

// ── Theme queries ───────────────────────────────────────────────────

// Get current theme colors (0xRRGGBB)
uint32_t thistle_ui_theme_primary(void);
uint32_t thistle_ui_theme_bg(void);
uint32_t thistle_ui_theme_surface(void);
uint32_t thistle_ui_theme_text(void);
uint32_t thistle_ui_theme_text_secondary(void);
