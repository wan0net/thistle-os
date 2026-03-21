#pragma once

/*
 * ThistleOS App SDK
 *
 * External apps include this header and link against the syscall stubs.
 * At runtime, the ELF loader resolves these symbols to kernel implementations.
 */

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

/* App entry point -- every app must implement this */
typedef struct {
    const char *id;
    const char *name;
    const char *version;
    bool allow_background;
    int  (*on_create)(void);
    void (*on_start)(void);
    void (*on_pause)(void);
    void (*on_resume)(void);
    void (*on_destroy)(void);
} thistle_app_t;

/* Macro to declare an app's entry point */
#define THISTLE_APP(app_var) \
    __attribute__((section(".thistle_app"))) \
    const thistle_app_t *_thistle_app_entry = &(app_var)

/* === System calls (resolved at load time by ELF loader) === */

/* System */
extern void     thistle_log(const char *tag, const char *fmt, ...);
extern uint32_t thistle_millis(void);
extern void     thistle_delay(uint32_t ms);
extern void    *thistle_malloc(size_t size);
extern void     thistle_free(void *ptr);

/* Display -- apps use LVGL directly (symbols exported by kernel) */
extern uint16_t thistle_display_get_width(void);
extern uint16_t thistle_display_get_height(void);

/* Input */
typedef void (*thistle_input_cb_t)(int event_type, int keycode, int x, int y);
extern void thistle_input_register_cb(thistle_input_cb_t cb);

/* Radio */
extern int thistle_radio_send(const uint8_t *data, size_t len);
extern int thistle_radio_set_freq(uint32_t freq_hz);

/* GPS */
extern int thistle_gps_enable(void);
extern int thistle_gps_get_lat_lon(double *lat, double *lon);

/* Storage */
extern int thistle_fs_open(const char *path, int flags);
extern int thistle_fs_read(int fd, void *buf, size_t len);
extern int thistle_fs_write(int fd, const void *buf, size_t len);
extern int thistle_fs_close(int fd);

/* IPC */
extern int thistle_msg_send(uint32_t dst_app, uint32_t type, const void *data, size_t len);
extern int thistle_msg_recv(uint32_t *src_app, uint32_t *type, void *data, size_t *len, uint32_t timeout_ms);

/* Power */
extern uint16_t thistle_power_get_battery_mv(void);
extern uint8_t  thistle_power_get_battery_pct(void);

/* === Widget API (toolkit-agnostic UI — implemented by window manager) === */

typedef uint32_t thistle_widget_t;
#define THISTLE_WIDGET_NONE ((thistle_widget_t)0)

extern thistle_widget_t thistle_ui_get_app_root(void);
extern thistle_widget_t thistle_ui_create_container(thistle_widget_t parent);
extern thistle_widget_t thistle_ui_create_label(thistle_widget_t parent, const char *text);
extern thistle_widget_t thistle_ui_create_button(thistle_widget_t parent, const char *text);
extern thistle_widget_t thistle_ui_create_text_input(thistle_widget_t parent, const char *placeholder);
extern void thistle_ui_destroy(thistle_widget_t widget);
extern void thistle_ui_set_text(thistle_widget_t widget, const char *text);
extern const char *thistle_ui_get_text(thistle_widget_t widget);
extern void thistle_ui_set_size(thistle_widget_t widget, int w, int h);
extern void thistle_ui_set_pos(thistle_widget_t widget, int x, int y);
extern void thistle_ui_set_visible(thistle_widget_t widget, _Bool visible);
extern void thistle_ui_set_bg_color(thistle_widget_t widget, uint32_t color);
extern void thistle_ui_set_text_color(thistle_widget_t widget, uint32_t color);
extern void thistle_ui_set_font_size(thistle_widget_t widget, int size);
extern void thistle_ui_set_layout(thistle_widget_t widget, int layout);
extern void thistle_ui_set_gap(thistle_widget_t widget, int gap);
extern void thistle_ui_set_flex_grow(thistle_widget_t widget, int grow);
extern void thistle_ui_set_scrollable(thistle_widget_t widget, _Bool scrollable);
extern void thistle_ui_set_padding(thistle_widget_t widget, int t, int r, int b, int l);

#define THISTLE_LAYOUT_NONE        0
#define THISTLE_LAYOUT_FLEX_COLUMN 1
#define THISTLE_LAYOUT_FLEX_ROW    2
#define THISTLE_EVENT_CLICK        0
#define THISTLE_EVENT_VALUE_CHANGED 1
#define THISTLE_EVENT_KEY          2
typedef void (*thistle_event_cb_t)(thistle_widget_t widget, int event, void *user_data);
extern void thistle_ui_on_event(thistle_widget_t widget, int event, thistle_event_cb_t cb, void *user_data);

extern uint32_t thistle_ui_theme_primary(void);
extern uint32_t thistle_ui_theme_bg(void);
extern uint32_t thistle_ui_theme_surface(void);
extern uint32_t thistle_ui_theme_text(void);
extern uint32_t thistle_ui_theme_text_secondary(void);
