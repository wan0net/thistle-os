/*
 * Simulator platform stubs — ESP-IDF API stubs for host build.
 * The kernel is 100% Rust (libthistle_kernel.a). These stubs provide
 * the ESP-IDF C functions that the Rust kernel calls via FFI.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#undef fopen
#undef opendir
#undef stat

#include "esp_timer.h"
#include "esp_err.h"
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <sys/time.h>
#include <stddef.h>

/* ── esp_timer ─────────────────────────────────────────────────────── */
struct esp_timer { int dummy; };

int64_t esp_timer_get_time(void) {
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (int64_t)tv.tv_sec * 1000000LL + (int64_t)tv.tv_usec;
}

esp_err_t esp_timer_create(const esp_timer_create_args_t *args, esp_timer_handle_t *handle) {
    (void)args;
    *handle = (esp_timer_handle_t)calloc(1, sizeof(struct esp_timer));
    return ESP_OK;
}
esp_err_t esp_timer_start_periodic(esp_timer_handle_t h, uint64_t p) { (void)h;(void)p; return ESP_OK; }
esp_err_t esp_timer_start_once(esp_timer_handle_t h, uint64_t t) { (void)h;(void)t; return ESP_OK; }
esp_err_t esp_timer_delete(esp_timer_handle_t h) { free(h); return ESP_OK; }
esp_err_t esp_timer_stop(esp_timer_handle_t h) { (void)h; return ESP_OK; }

/* ── Heap ──────────────────────────────────────────────────────────── */
size_t heap_caps_get_free_size(unsigned int caps) { (void)caps; return 4 * 1024 * 1024; }
void *heap_caps_malloc(size_t size, unsigned int caps) { (void)caps; return malloc(size); }

/* ── FreeRTOS ──────────────────────────────────────────────────────── */
void vTaskDelay(unsigned int ticks) { (void)ticks; }
int xTaskCreatePinnedToCore(void *fn, const char *n, unsigned int s, void *p, unsigned int pr, void **h, int c) {
    (void)fn;(void)n;(void)s;(void)p;(void)pr;(void)h;(void)c; return 1;
}
int xTaskCreate(void *fn, const char *n, unsigned int s, void *p, unsigned int pr, void **h) {
    (void)fn;(void)n;(void)s;(void)p;(void)pr;(void)h; return 1;
}
void vTaskDelete(void *t) { (void)t; }
void *xQueueGenericCreate(unsigned int len, unsigned int sz, unsigned char type) {
    (void)len;(void)sz;(void)type; return NULL;
}
int xQueueGenericSend(void *q, const void *item, unsigned int ticks, int pos) {
    (void)q;(void)item;(void)ticks;(void)pos; return 1;
}
int xQueueReceive(void *q, void *buf, unsigned int ticks) {
    (void)q;(void)buf;(void)ticks; return 0;
}

/* ── esp_elf (ELF loader) ─────────────────────────────────────────── */
void *esp_elf_init(void *elf) { (void)elf; return NULL; }
int esp_elf_relocate(void *elf, void *buf) { (void)elf;(void)buf; return -1; }
int esp_elf_request(void *elf, int opt, int argc, void *argv) { (void)elf;(void)opt;(void)argc;(void)argv; return -1; }
void esp_elf_deinit(void *elf) { (void)elf; }
void elf_set_symbol_resolver(void *resolver) { (void)resolver; }

/* ── Logging ───────────────────────────────────────────────────────── */
void esp_log_write(int level, const char *tag, const char *fmt, ...) { (void)level;(void)tag;(void)fmt; }

/* ── HAL crypto accessor (no hardware in simulator) ────────────────── */
const void *hal_crypto_get(void) { return 0; }

/* ── HAL helpers (Rust syscall_table references these) ─────────────── */
unsigned short hal_display_get_width_helper(void) { return 320; }
unsigned short hal_display_get_height_helper(void) { return 240; }
/* hal_registry_start_all/stop_all — now in hal_registry.c */

/* ── C stdlib wrappers (Rust syscall_table exports these) ──────────── */
void *libc_malloc(unsigned int sz) { return malloc(sz); }
void libc_free(void *p) { free(p); }
void *libc_realloc(void *p, unsigned int sz) { return realloc(p, sz); }
void *thistle_fs_open_impl(const char *p, const char *m) { return fopen(p, m); }
int thistle_fs_read_impl(void *f, void *b, unsigned int s) { return (int)fread(b, 1, s, f); }
int thistle_fs_write_impl(void *f, const void *b, unsigned int s) { return (int)fwrite(b, 1, s, f); }
int thistle_fs_close_impl(void *f) { return fclose(f); }

/* ── HAL syscall stubs (no hardware in simulator) ──────────────────── */
void thistle_input_register_cb_impl(void *cb, void *ud) { (void)cb;(void)ud; }
int thistle_radio_send_impl(const void *d, unsigned int l) { (void)d;(void)l; return -1; }
int thistle_radio_start_rx_impl(void) { return -1; }
int thistle_radio_set_freq_impl(float f) { (void)f; return -1; }
int thistle_gps_get_position_impl(void *p) { (void)p; return -1; }
int thistle_gps_enable_impl(int e) { (void)e; return -1; }
int thistle_power_get_battery_mv_impl(void) { return 3700; }
int thistle_power_get_battery_pct_impl(void) { return 75; }

/* ── HTTP client stubs (Rust appstore_client calls these) ──────────── */
void *esp_http_client_init(const void *config) { (void)config; return NULL; }
int esp_http_client_perform(void *c) { (void)c; return -1; }
int esp_http_client_open(void *c, int l) { (void)c;(void)l; return -1; }
int esp_http_client_fetch_headers(void *c) { (void)c; return -1; }
int esp_http_client_read(void *c, void *b, int l) { (void)c;(void)b;(void)l; return -1; }
int esp_http_client_get_status_code(void *c) { (void)c; return 0; }
int esp_http_client_close(void *c) { (void)c; return 0; }
int esp_http_client_cleanup(void *c) { (void)c; return 0; }

/* ── NVS / SPIFFS stubs (simulator has no flash storage) ─────────── */
int nvs_flash_init_safe(void) { return 0; }
int spiffs_mount(void) { return 0; }

/* ── Modem PPP stubs ───────────────────────────────────────────────── */
int drv_a7682e_start_ppp(void) { return -1; }
int drv_a7682e_stop_ppp(void) { return 0; }
int drv_a7682e_ppp_connected(void) { return 0; }
