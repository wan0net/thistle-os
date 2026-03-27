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

/* Crypto */
extern int thistle_crypto_sha256(const uint8_t *data, size_t len, uint8_t *hash_out);
extern int thistle_crypto_hmac_sha256(const uint8_t *key, size_t key_len, const uint8_t *data, size_t data_len, uint8_t *mac_out);
extern int thistle_crypto_hmac_verify(const uint8_t *key, size_t key_len, const uint8_t *data, size_t data_len, const uint8_t *expected_mac);
extern int thistle_crypto_aes256_cbc_encrypt(const uint8_t *key, const uint8_t *iv, const uint8_t *plaintext, size_t len, uint8_t *ciphertext_out);
extern int thistle_crypto_aes256_cbc_decrypt(const uint8_t *key, const uint8_t *iv, const uint8_t *ciphertext, size_t len, uint8_t *plaintext_out);
extern int thistle_crypto_aes128_ecb_encrypt(const uint8_t *key, const uint8_t *plaintext, size_t len, uint8_t *ciphertext_out);
extern int thistle_crypto_aes128_ecb_decrypt(const uint8_t *key, const uint8_t *ciphertext, size_t len, uint8_t *plaintext_out);
extern int thistle_crypto_pbkdf2_sha256(const char *password, const uint8_t *salt, size_t salt_len, uint32_t iterations, uint8_t *key_out, size_t key_len);
extern int thistle_crypto_random(uint8_t *buf, size_t len);
extern int thistle_crypto_ed25519_keygen(uint8_t *private_key_out, uint8_t *public_key_out);
extern int thistle_crypto_ed25519_sign(const uint8_t *private_key, const uint8_t *message, size_t msg_len, uint8_t *signature_out);
extern int thistle_crypto_ed25519_verify(const uint8_t *public_key, const uint8_t *message, size_t msg_len, const uint8_t *signature);
extern int thistle_crypto_ed25519_derive_public(const uint8_t *private_key, uint8_t *public_key_out);

/* Mesh service */

typedef struct {
    uint8_t  pub_key[32];
    uint8_t  name[32];
    uint8_t  name_len;
    uint8_t  node_type;
    int8_t   last_rssi;
    uint8_t  path_len;
    uint32_t last_seen;
    double   lat;
    double   lon;
    _Bool    has_position;
} thistle_mesh_contact_t;

typedef struct {
    uint8_t  sender_key[32];
    uint8_t  sender_name[32];
    uint8_t  sender_name_len;
    uint32_t timestamp;
    uint8_t  text[200];
    uint16_t text_len;
} thistle_mesh_message_t;

typedef struct {
    uint32_t packets_sent;
    uint32_t packets_received;
    uint32_t packets_forwarded;
    uint32_t messages_sent;
    uint32_t messages_received;
    uint32_t contacts_discovered;
} thistle_mesh_stats_t;

extern int         thistle_mesh_init(const char *name, uint8_t node_type);
extern int         thistle_mesh_deinit(void);
extern int         thistle_mesh_loop(void);
extern int         thistle_mesh_send(const uint8_t *dest_key, const char *text);
extern int         thistle_mesh_send_advert(void);
extern int         thistle_mesh_send_advert_pos(double lat, double lon);
extern int         thistle_mesh_get_contact_count(void);
extern int         thistle_mesh_get_contact(int index, thistle_mesh_contact_t *out);
extern int         thistle_mesh_find_contact(const uint8_t *pub_key);
extern int         thistle_mesh_get_inbox_count(void);
extern int         thistle_mesh_get_inbox_message(int index, thistle_mesh_message_t *out);
extern int         thistle_mesh_clear_inbox(void);
extern int         thistle_mesh_get_self_key(uint8_t *out);
extern const char *thistle_mesh_get_self_name(void);
extern int         thistle_mesh_get_stats(thistle_mesh_stats_t *out);

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
extern void thistle_ui_set_border_width(thistle_widget_t widget, int w);
extern void thistle_ui_set_radius(thistle_widget_t widget, int r);
extern void thistle_ui_set_align(thistle_widget_t widget, int main_align, int cross_align);
extern void thistle_ui_set_password_mode(thistle_widget_t widget, _Bool pw);
extern void thistle_ui_set_one_line(thistle_widget_t widget, _Bool one_line);
extern void thistle_ui_set_placeholder(thistle_widget_t widget, const char *text);

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
