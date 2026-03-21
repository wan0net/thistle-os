/*
 * WASM platform stubs — ESP-IDF API stubs for Emscripten build.
 * Same as simulator/platform/platform_stubs.c but for WASM target.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include "esp_timer.h"
#include "esp_err.h"
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <stddef.h>
#include <emscripten.h>
#include <emscripten/eventloop.h>

/* ── esp_timer (real implementation using Emscripten) ──────────────── */
struct esp_timer {
    esp_timer_cb_t callback;
    void *arg;
    int em_id;       /* emscripten_set_timeout ID for cancellation */
    int periodic;
    uint64_t period_us;
};

int64_t esp_timer_get_time(void) {
    return (int64_t)(emscripten_get_now() * 1000.0); /* ms to us */
}

static void timer_trampoline(void *user_data) {
    struct esp_timer *t = (struct esp_timer *)user_data;
    if (t && t->callback) {
        t->callback(t->arg);
        if (t->periodic) {
            t->em_id = emscripten_set_timeout(timer_trampoline, (double)t->period_us / 1000.0, t);
        }
    }
}

esp_err_t esp_timer_create(const esp_timer_create_args_t *args, esp_timer_handle_t *handle) {
    struct esp_timer *t = calloc(1, sizeof(struct esp_timer));
    if (!t) return ESP_ERR_NO_MEM;
    t->callback = args->callback;
    t->arg = args->arg;
    t->em_id = 0;
    t->periodic = 0;
    *handle = t;
    return ESP_OK;
}

esp_err_t esp_timer_start_periodic(esp_timer_handle_t h, uint64_t period_us) {
    if (!h) return ESP_ERR_INVALID_ARG;
    h->periodic = 1;
    h->period_us = period_us;
    h->em_id = emscripten_set_timeout(timer_trampoline, (double)period_us / 1000.0, h);
    return ESP_OK;
}

esp_err_t esp_timer_start_once(esp_timer_handle_t h, uint64_t timeout_us) {
    if (!h) return ESP_ERR_INVALID_ARG;
    h->periodic = 0;
    h->em_id = emscripten_set_timeout(timer_trampoline, (double)timeout_us / 1000.0, h);
    return ESP_OK;
}

esp_err_t esp_timer_stop(esp_timer_handle_t h) {
    if (!h) return ESP_ERR_INVALID_ARG;
    if (h->em_id) {
        emscripten_clear_timeout(h->em_id);
        h->em_id = 0;
    }
    return ESP_OK;
}

esp_err_t esp_timer_delete(esp_timer_handle_t h) {
    if (!h) return ESP_ERR_INVALID_ARG;
    esp_timer_stop(h);
    free(h);
    return ESP_OK;
}

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

/* ── esp_elf ───────────────────────────────────────────────────────── */
void *esp_elf_init(void *elf) { (void)elf; return NULL; }
int esp_elf_relocate(void *elf, void *buf) { (void)elf;(void)buf; return -1; }
int esp_elf_request(void *elf, int opt, int argc, void *argv) { (void)elf;(void)opt;(void)argc;(void)argv; return -1; }
void esp_elf_deinit(void *elf) { (void)elf; }
void elf_set_symbol_resolver(void *resolver) { (void)resolver; }

/* ── Logging ───────────────────────────────────────────────────────── */
void esp_log_write(int level, const char *tag, const char *fmt, ...) { (void)level;(void)tag;(void)fmt; }

/* ── HAL helpers ───────────────────────────────────────────────────── */
unsigned short hal_display_get_width_helper(void) { return 320; }
unsigned short hal_display_get_height_helper(void) { return 240; }
/* hal_registry_start_all/stop_all — now in hal_registry.c */
/* board_init provided by board_simulator.c */

/* ── C stdlib wrappers ─────────────────────────────────────────────── */
void *libc_malloc(unsigned int sz) { return malloc(sz); }
void libc_free(void *p) { free(p); }
void *libc_realloc(void *p, unsigned int sz) { return realloc(p, sz); }
void *thistle_fs_open_impl(const char *p, const char *m) { return fopen(p, m); }
int thistle_fs_read_impl(void *f, void *b, unsigned int s) { return (int)fread(b, 1, s, f); }
int thistle_fs_write_impl(void *f, const void *b, unsigned int s) { return (int)fwrite(b, 1, s, f); }
int thistle_fs_close_impl(void *f) { return fclose(f); }

/* ── HAL syscall stubs ─────────────────────────────────────────────── */
void thistle_input_register_cb_impl(void *cb, void *ud) { (void)cb;(void)ud; }
int thistle_radio_send_impl(const void *d, unsigned int l) { (void)d;(void)l; return -1; }
int thistle_radio_start_rx_impl(void) { return -1; }
int thistle_radio_set_freq_impl(float f) { (void)f; return -1; }
int thistle_gps_get_position_impl(void *p) { (void)p; return -1; }
int thistle_gps_enable_impl(int e) { (void)e; return -1; }
int thistle_power_get_battery_mv_impl(void) { return 3700; }
int thistle_power_get_battery_pct_impl(void) { return 75; }

/* ── HTTP client stubs ─────────────────────────────────────────────── */
void *esp_http_client_init(const void *config) { (void)config; return NULL; }
int esp_http_client_perform(void *c) { (void)c; return -1; }
int esp_http_client_open(void *c, int l) { (void)c;(void)l; return -1; }
int esp_http_client_fetch_headers(void *c) { (void)c; return -1; }
int esp_http_client_read(void *c, void *b, int l) { (void)c;(void)b;(void)l; return -1; }
int esp_http_client_get_status_code(void *c) { (void)c; return 0; }
int esp_http_client_close(void *c) { (void)c; return 0; }
int esp_http_client_cleanup(void *c) { (void)c; return 0; }

/* ── Modem PPP stubs ───────────────────────────────────────────────── */
int drv_a7682e_start_ppp(void) { return -1; }
int drv_a7682e_stop_ppp(void) { return 0; }
int drv_a7682e_ppp_connected(void) { return 0; }
