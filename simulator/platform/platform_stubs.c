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
#include <stdarg.h>

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

/* ── FreeRTOS (real pthreads) ──────────────────────────────────────── */
#include <pthread.h>
#include <unistd.h>

typedef void (*TaskFunction_t)(void *);

typedef struct {
    pthread_t thread;
    TaskFunction_t func;
    void *param;
    char name[32];
} sim_task_t;

static void *task_wrapper(void *arg) {
    sim_task_t *task = (sim_task_t *)arg;
    task->func(task->param);
    /* Task returned — clean up (FreeRTOS tasks that return are deleted) */
    return NULL;
}

void vTaskDelay(unsigned int ticks) {
    usleep(ticks * 1000);  /* 1 tick = 1ms */
}

int xTaskCreatePinnedToCore(void *fn, const char *n, unsigned int s, void *p, unsigned int pr, void **h, int c) {
    (void)pr; (void)c;
    sim_task_t *task = calloc(1, sizeof(sim_task_t));
    if (!task) return 0;
    task->func = (TaskFunction_t)fn;
    task->param = p;
    if (n) strncpy(task->name, n, sizeof(task->name) - 1);
    if (h) *h = task;

    pthread_attr_t attr;
    pthread_attr_init(&attr);
    if (s > 0) pthread_attr_setstacksize(&attr, s < 16384 ? 16384 : s);
    int ret = pthread_create(&task->thread, &attr, task_wrapper, task);
    pthread_attr_destroy(&attr);
    pthread_detach(task->thread);  /* Auto-cleanup on exit */

    if (ret != 0) {
        free(task);
        if (h) *h = NULL;
        return 0;
    }
    return 1;  /* pdPASS */
}

int xTaskCreate(void *fn, const char *n, unsigned int s, void *p, unsigned int pr, void **h) {
    return xTaskCreatePinnedToCore(fn, n, s, p, pr, h, 0);
}

void vTaskDelete(void *t) {
    if (t) {
        sim_task_t *task = (sim_task_t *)t;
        pthread_cancel(task->thread);
        free(task);
    }
    /* vTaskDelete(NULL) means delete self — just exit the thread */
}

/* ── Thread-safe queues (pthread mutex + condvar) ─────────────────── */
typedef struct {
    uint8_t *buf;
    size_t item_size;
    size_t capacity;
    size_t head;
    size_t tail;
    size_t count;
    pthread_mutex_t mutex;
    pthread_cond_t not_empty;
    pthread_cond_t not_full;
} sim_queue_impl_t;

void *xQueueGenericCreate(unsigned int len, unsigned int sz, unsigned char type) {
    (void)type;
    sim_queue_impl_t *q = calloc(1, sizeof(sim_queue_impl_t));
    if (!q) return NULL;
    q->buf = calloc(len, sz);
    if (!q->buf) { free(q); return NULL; }
    q->item_size = sz;
    q->capacity = len;
    pthread_mutex_init(&q->mutex, NULL);
    pthread_cond_init(&q->not_empty, NULL);
    pthread_cond_init(&q->not_full, NULL);
    return q;
}

int xQueueGenericSend(void *queue, const void *item, unsigned int ticks, int pos) {
    (void)pos;
    sim_queue_impl_t *q = (sim_queue_impl_t *)queue;
    if (!q || !item) return 0;

    pthread_mutex_lock(&q->mutex);
    if (q->count >= q->capacity) {
        if (ticks == 0) {
            pthread_mutex_unlock(&q->mutex);
            return 0;
        }
        /* Wait with timeout */
        struct timespec ts;
        clock_gettime(CLOCK_REALTIME, &ts);
        ts.tv_nsec += (long)(ticks) * 1000000L;
        ts.tv_sec += ts.tv_nsec / 1000000000L;
        ts.tv_nsec %= 1000000000L;
        while (q->count >= q->capacity) {
            if (pthread_cond_timedwait(&q->not_full, &q->mutex, &ts) != 0) {
                pthread_mutex_unlock(&q->mutex);
                return 0;
            }
        }
    }
    memcpy(q->buf + q->head * q->item_size, item, q->item_size);
    q->head = (q->head + 1) % q->capacity;
    q->count++;
    pthread_cond_signal(&q->not_empty);
    pthread_mutex_unlock(&q->mutex);
    return 1;
}

int xQueueReceive(void *queue, void *buf, unsigned int ticks) {
    sim_queue_impl_t *q = (sim_queue_impl_t *)queue;
    if (!q || !buf) return 0;

    pthread_mutex_lock(&q->mutex);
    if (q->count == 0) {
        if (ticks == 0) {
            pthread_mutex_unlock(&q->mutex);
            return 0;
        }
        struct timespec ts;
        clock_gettime(CLOCK_REALTIME, &ts);
        ts.tv_nsec += (long)(ticks) * 1000000L;
        ts.tv_sec += ts.tv_nsec / 1000000000L;
        ts.tv_nsec %= 1000000000L;
        while (q->count == 0) {
            if (pthread_cond_timedwait(&q->not_empty, &q->mutex, &ts) != 0) {
                pthread_mutex_unlock(&q->mutex);
                return 0;
            }
        }
    }
    memcpy(buf, q->buf + q->tail * q->item_size, q->item_size);
    q->tail = (q->tail + 1) % q->capacity;
    q->count--;
    pthread_cond_signal(&q->not_full);
    pthread_mutex_unlock(&q->mutex);
    return 1;
}

/* Wrappers for C callers (headers declare xQueueCreate/xQueueSend) */
void *xQueueCreate(unsigned int length, unsigned int item_size) {
    return xQueueGenericCreate(length, item_size, 0);
}

int xQueueSend(void *queue, const void *item, unsigned int ticks) {
    return xQueueGenericSend(queue, item, ticks, 0);
}

/* ── esp_elf (ELF loader) ─────────────────────────────────────────── */
void *esp_elf_init(void *elf) { (void)elf; return NULL; }
int esp_elf_relocate(void *elf, void *buf) { (void)elf;(void)buf; return -1; }
int esp_elf_request(void *elf, int opt, int argc, void *argv) { (void)elf;(void)opt;(void)argc;(void)argv; return -1; }
void esp_elf_deinit(void *elf) { (void)elf; }
void elf_set_symbol_resolver(void *resolver) { (void)resolver; }

/* ── Logging ───────────────────────────────────────────────────────── */
void esp_log_write(int level, const char *tag, const char *fmt, ...) {
    (void)level;
    char buf[512];
    va_list args;
    va_start(args, fmt);
    vsnprintf(buf, sizeof(buf), fmt, args);
    va_end(args);

    /* Print to stdout */
    if (tag && tag[0]) {
        printf("[%s] %s", tag, buf);
    } else {
        printf("%s", buf);
    }
    /* Ensure newline */
    size_t len = strlen(buf);
    if (len == 0 || buf[len - 1] != '\n') {
        printf("\n");
    }
    fflush(stdout);

    /* Check assertions */
    extern void sim_assert_check_line(const char *line);
    sim_assert_check_line(buf);
}

/* ── Functions moved to Rust (kernel_rs) ──────────────────────────── */
/* hal_crypto_get, hal_display_get_width/height_helper, libc_malloc/free/realloc,
 * thistle_fs_*_impl, thistle_input/radio/gps/power_*_impl,
 * nvs_flash_init_safe, spiffs_mount, hal_*_register, hal_get_registry,
 * hal_bus_*, hal_registry_start_all/stop_all — all in Rust now. */

/* ── HTTP client stubs (Rust appstore_client calls these) ──────────── */
void *esp_http_client_init(const void *config) { (void)config; return NULL; }
int esp_http_client_perform(void *c) { (void)c; return -1; }
int esp_http_client_open(void *c, int l) { (void)c;(void)l; return -1; }
int esp_http_client_fetch_headers(void *c) { (void)c; return -1; }
int esp_http_client_read(void *c, void *b, int l) { (void)c;(void)b;(void)l; return -1; }
int esp_http_client_get_status_code(void *c) { (void)c; return 0; }
int esp_http_client_close(void *c) { (void)c; return 0; }
int esp_http_client_cleanup(void *c) { (void)c; return 0; }

/* ── Crypto driver stub ───────────────────────────────────────────── */
void *drv_crypto_mbedtls_get(void) { return NULL; }

/* ── GPIO stubs ───────────────────────────────────────────────────── */
#include "driver/gpio.h"
esp_err_t gpio_config(const gpio_config_t *cfg) { (void)cfg; return 0; }
esp_err_t gpio_set_level(gpio_num_t pin, uint32_t level) { (void)pin; (void)level; return 0; }
int gpio_get_level(gpio_num_t pin) { (void)pin; return 0; }
esp_err_t gpio_install_isr_service(int flags) { (void)flags; return 0; }
esp_err_t gpio_isr_handler_add(gpio_num_t pin, void(*fn)(void*), void *arg) { (void)pin; (void)fn; (void)arg; return 0; }
esp_err_t gpio_isr_handler_remove(gpio_num_t pin) { (void)pin; return 0; }
esp_err_t gpio_set_direction(gpio_num_t pin, gpio_mode_t mode) { (void)pin; (void)mode; return 0; }
esp_err_t gpio_set_pull_mode(gpio_num_t pin, int mode) { (void)pin; (void)mode; return 0; }
esp_err_t gpio_set_intr_type(gpio_num_t pin, gpio_intr_type_t type) { (void)pin; (void)type; return 0; }
esp_err_t gpio_intr_enable(gpio_num_t pin) { (void)pin; return 0; }

/* ── Modem PPP stubs ───────────────────────────────────────────────── */
int drv_a7682e_start_ppp(void) { return -1; }
int drv_a7682e_stop_ppp(void) { return 0; }
int drv_a7682e_ppp_connected(void) { return 0; }
