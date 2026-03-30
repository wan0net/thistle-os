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
/* esp_log_write provided by Rust kernel (hal_registry.rs) on non-test builds.
 * WASM needs a C stub because the Rust version uses variadic FFI. */
void esp_log_write(int level, const char *tag, const char *fmt, ...) { (void)level;(void)tag;(void)fmt; }

/* ── Functions moved to Rust (kernel_rs) ──────────────────────────── */
/* The following are now implemented in Rust modules and linked from
 * libthistle_kernel.a. Do NOT re-declare here:
 *   hal_crypto_get, hal_display_get_width/height_helper,
 *   libc_malloc/free/realloc, thistle_fs_*_impl,
 *   thistle_input/radio/gps/power_*_impl,
 *   nvs_flash_init_safe, spiffs_mount,
 *   hal_*_register, hal_get_registry, hal_bus_*,
 *   hal_registry_start_all, hal_registry_stop_all
 */

/* ── esp_http_client stubs (for Rust kernel FFI that calls these directly) ── */
/* Do NOT include esp_http_client.h — it has static inline versions that would
 * conflict. The Rust kernel extern "C" FFI only needs the symbol names.
 * Use void* to avoid type conflicts.
 * The Rust kernel calls these by symbol name via extern "C" FFI. */
void *esp_http_client_init(const void *c) { (void)c; return NULL; }
int esp_http_client_perform(void *c) { (void)c; return -1; }
int esp_http_client_open(void *c, int l) { (void)c;(void)l; return -1; }
int esp_http_client_fetch_headers(void *c) { (void)c; return -1; }
int esp_http_client_read(void *c, char *b, int l) { (void)c;(void)b;(void)l; return -1; }
int esp_http_client_get_status_code(void *c) { (void)c; return 0; }
int esp_http_client_close(void *c) { (void)c; return 0; }
void esp_http_client_cleanup(void *c) { (void)c; }

/* ── sim_http stubs (backing for simulator/platform/esp_http_client.h inlines) */
#include "sim_http.h"
sim_http_client_handle_t sim_http_client_init(const sim_http_client_config_t *c) { (void)c; return NULL; }
int sim_http_client_perform(sim_http_client_handle_t c) { (void)c; return -1; }
int sim_http_client_get_status_code(sim_http_client_handle_t c) { (void)c; return 0; }
int sim_http_client_get_content_length(sim_http_client_handle_t c) { (void)c; return 0; }
const char *sim_http_client_get_response_data(sim_http_client_handle_t c) { (void)c; return ""; }
size_t sim_http_client_get_response_length(sim_http_client_handle_t c) { (void)c; return 0; }
int sim_http_client_open(sim_http_client_handle_t c, int l) { (void)c;(void)l; return -1; }
int sim_http_client_fetch_headers(sim_http_client_handle_t c) { (void)c; return -1; }
int sim_http_client_read(sim_http_client_handle_t c, char *b, int l) { (void)c;(void)b;(void)l; return -1; }
int sim_http_client_close(sim_http_client_handle_t c) { (void)c; return 0; }
void sim_http_client_cleanup(sim_http_client_handle_t c) { (void)c; }

/* ── Modem PPP stubs ───────────────────────────────────────────────── */
int drv_a7682e_start_ppp(void) { return -1; }
int drv_a7682e_stop_ppp(void) { return 0; }
int drv_a7682e_ppp_connected(void) { return 0; }

/* ── Simulator compat stubs ─────────��──────────────────────────────── */
#include <stdbool.h>
bool sim_is_headless(void) { return false; }
/* wifi_manager_scan_start/get_count now in Rust thistle_shell.rs */

/* ── Crypto driver stub ───────────────────────────────────────────── */
void *drv_crypto_mbedtls_get(void) { return NULL; }

/* ── GPIO stubs ───────────────────────────────────────────────────── */
int gpio_config(const void *cfg) { (void)cfg; return 0; }
int gpio_set_level(int pin, unsigned int level) { (void)pin; (void)level; return 0; }
int gpio_get_level(int pin) { (void)pin; return 0; }
int gpio_install_isr_service(int flags) { (void)flags; return 0; }
int gpio_isr_handler_add(int pin, void(*fn)(void*), void *arg) { (void)pin; (void)fn; (void)arg; return 0; }
int gpio_isr_handler_remove(int pin) { (void)pin; return 0; }
int gpio_set_direction(int pin, int mode) { (void)pin; (void)mode; return 0; }
int gpio_set_pull_mode(int pin, int mode) { (void)pin; (void)mode; return 0; }
int gpio_set_intr_type(int pin, int type) { (void)pin; (void)type; return 0; }
int gpio_intr_enable(int pin) { (void)pin; return 0; }

/* ── UART stubs ───────────────────────────────────────────────────── */
int uart_param_config(int num, const void *cfg) { (void)num; (void)cfg; return 0; }
int uart_set_pin(int num, int tx, int rx, int rts, int cts) { (void)num; (void)tx; (void)rx; (void)rts; (void)cts; return 0; }
int uart_driver_install(int num, int rx, int tx, int q, void *qh, int f) { (void)num; (void)rx; (void)tx; (void)q; (void)qh; (void)f; return 0; }
int uart_driver_delete(int num) { (void)num; return 0; }
int uart_write_bytes(int num, const void *data, int len) { (void)num; (void)data; return len; }
int uart_read_bytes(int num, void *buf, int len, int timeout) { (void)num; (void)buf; (void)len; (void)timeout; return 0; }

/* ── Queue wrappers (C callers) ───────────────────────────────────── */
void *xQueueCreate(unsigned int len, unsigned int sz) { return xQueueGenericCreate(len, sz, 0); }
int xQueueSend(void *q, const void *item, unsigned int ticks) { return xQueueGenericSend(q, item, ticks, 0); }

/* ── Kernel function stubs needed by new modules ──────────────────── */
unsigned int esp_get_free_heap_size(void) { return 4 * 1024 * 1024; }
void esp_restart(void) { printf("esp_restart called (WASM: no-op)\n"); }
/* app_manager_get_count now provided by Rust app_manager.rs */
/* hal_storage_get_total/free_bytes now in Rust thistle_shell.rs */
