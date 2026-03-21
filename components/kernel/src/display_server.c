// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

/*
 * display_server.c — Kernel display compositor and surface manager.
 *
 * Provides toolkit-agnostic framebuffer management for swappable window
 * managers. The WM draws into surfaces, the display server composites
 * them and flushes to the hardware display.
 */

#include "thistle/display_server.h"
#include "hal/board.h"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include <string.h>

static const char *TAG = "ds";

#define MAX_SURFACES 8

typedef struct {
    surface_info_t info;
    uint8_t *buffer;           /* Framebuffer in PSRAM */
    size_t buffer_size;
    bool allocated;
    bool dirty;                /* Has dirty regions */
    hal_area_t dirty_area;     /* Bounding box of dirty region */
    ds_input_cb_t input_cb;
    void *input_user_data;
} surface_t;

static surface_t s_surfaces[MAX_SURFACES];
static uint32_t s_next_id = 1;
static const display_server_wm_t *s_wm = NULL;
static bool s_initialized = false;

/* ── Helpers ─────────────────────────────────────────────────────────── */

static surface_t *find_surface(surface_id_t id)
{
    for (int i = 0; i < MAX_SURFACES; i++) {
        if (s_surfaces[i].allocated && (surface_id_t)(i + 1) == id) {
            return &s_surfaces[i];
        }
    }
    return NULL;
}

static uint8_t bytes_per_pixel(void)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg && reg->display && reg->display->type == HAL_DISPLAY_TYPE_EPAPER) {
        return 1; /* 1 byte per 8 pixels (1-bit packed) — approximate as 1 for allocation */
    }
    return 2; /* RGB565 */
}

/* ── Public API ─────────────────────────────────────────────────────── */

esp_err_t display_server_init(void)
{
    memset(s_surfaces, 0, sizeof(s_surfaces));
    s_next_id = 1;
    s_wm = NULL;
    s_initialized = true;

    uint16_t w = display_server_get_width();
    uint16_t h = display_server_get_height();
    const char *type_str = (display_server_get_display_type() == HAL_DISPLAY_TYPE_EPAPER)
                           ? "e-paper" : "LCD";

    ESP_LOGI(TAG, "Display server initialized: %dx%d %s", w, h, type_str);
    return ESP_OK;
}

esp_err_t display_server_register_wm(const display_server_wm_t *wm)
{
    if (!wm) return ESP_ERR_INVALID_ARG;

    /* Deinit previous WM if any */
    if (s_wm && s_wm->deinit) {
        ESP_LOGI(TAG, "Deinitializing previous WM: %s", s_wm->name);
        s_wm->deinit();
    }

    s_wm = wm;
    ESP_LOGI(TAG, "Window manager registered: %s v%s", wm->name, wm->version ? wm->version : "?");

    if (wm->init) {
        esp_err_t ret = wm->init();
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "WM init failed: %s", esp_err_to_name(ret));
            s_wm = NULL;
            return ret;
        }
    }

    return ESP_OK;
}

esp_err_t display_server_load_wm(const char *wm_elf_path)
{
    /* TODO: Load .wm.elf via ELF loader, resolve wm_get_interface() symbol,
     * call it to get the display_server_wm_t vtable, then register it.
     * For now, WMs are registered at compile time via display_server_register_wm(). */
    ESP_LOGW(TAG, "Runtime WM loading not yet implemented: %s", wm_elf_path);
    return ESP_ERR_NOT_SUPPORTED;
}

const char *display_server_get_wm_name(void)
{
    return s_wm ? s_wm->name : NULL;
}

/* ── Surface management ─────────────────────────────────────────────── */

surface_id_t display_server_create_surface(const surface_info_t *info)
{
    if (!info) return SURFACE_INVALID;

    for (int i = 0; i < MAX_SURFACES; i++) {
        if (!s_surfaces[i].allocated) {
            size_t buf_size = (size_t)info->width * info->height * bytes_per_pixel();

#ifdef SIMULATOR_BUILD
            uint8_t *buf = calloc(1, buf_size);
#else
            uint8_t *buf = heap_caps_calloc(1, buf_size, MALLOC_CAP_SPIRAM);
#endif
            if (!buf) {
                ESP_LOGE(TAG, "Surface alloc failed: %ux%u (%zu bytes)",
                         info->width, info->height, buf_size);
                return SURFACE_INVALID;
            }

            s_surfaces[i].info = *info;
            s_surfaces[i].buffer = buf;
            s_surfaces[i].buffer_size = buf_size;
            s_surfaces[i].allocated = true;
            s_surfaces[i].dirty = false;
            s_surfaces[i].input_cb = NULL;
            s_surfaces[i].input_user_data = NULL;

            surface_id_t id = (surface_id_t)(i + 1);
            ESP_LOGI(TAG, "Surface %u created: %ux%u role=%d",
                     (unsigned)id, info->width, info->height, info->role);
            return id;
        }
    }

    ESP_LOGE(TAG, "No free surface slots (max %d)", MAX_SURFACES);
    return SURFACE_INVALID;
}

void display_server_destroy_surface(surface_id_t id)
{
    surface_t *s = find_surface(id);
    if (!s) return;

    if (s->buffer) {
        free(s->buffer);
    }
    memset(s, 0, sizeof(*s));
    ESP_LOGD(TAG, "Surface %u destroyed", (unsigned)id);
}

uint8_t *display_server_get_buffer(surface_id_t id)
{
    surface_t *s = find_surface(id);
    return s ? s->buffer : NULL;
}

const surface_info_t *display_server_get_info(surface_id_t id)
{
    surface_t *s = find_surface(id);
    return s ? &s->info : NULL;
}

void display_server_mark_dirty(surface_id_t id, const hal_area_t *area)
{
    surface_t *s = find_surface(id);
    if (!s || !area) return;

    if (!s->dirty) {
        s->dirty_area = *area;
        s->dirty = true;
    } else {
        /* Expand bounding box */
        if (area->x1 < s->dirty_area.x1) s->dirty_area.x1 = area->x1;
        if (area->y1 < s->dirty_area.y1) s->dirty_area.y1 = area->y1;
        if (area->x2 > s->dirty_area.x2) s->dirty_area.x2 = area->x2;
        if (area->y2 > s->dirty_area.y2) s->dirty_area.y2 = area->y2;
    }
}

void display_server_mark_dirty_full(surface_id_t id)
{
    surface_t *s = find_surface(id);
    if (!s) return;
    s->dirty = true;
    s->dirty_area = (hal_area_t){
        .x1 = 0, .y1 = 0,
        .x2 = s->info.width - 1,
        .y2 = s->info.height - 1,
    };
}

void display_server_set_visible(surface_id_t id, bool visible)
{
    surface_t *s = find_surface(id);
    if (!s) return;
    s->info.visible = visible;
    if (visible) {
        display_server_mark_dirty_full(id);
    }
}

/* ── Compositor ─────────────────────────────────────────────────────── */

esp_err_t display_server_composite(void)
{
    const hal_registry_t *reg = hal_get_registry();
    if (!reg || !reg->display) return ESP_ERR_INVALID_STATE;

    bool any_dirty = false;
    hal_area_t composite_area = { .x1 = 0xFFFF, .y1 = 0xFFFF, .x2 = 0, .y2 = 0 };

    /* Find the composite dirty area across all surfaces */
    for (int i = 0; i < MAX_SURFACES; i++) {
        if (!s_surfaces[i].allocated || !s_surfaces[i].info.visible || !s_surfaces[i].dirty) {
            continue;
        }
        any_dirty = true;

        /* Translate surface-local dirty area to screen coordinates */
        hal_area_t screen_area = {
            .x1 = s_surfaces[i].info.x + s_surfaces[i].dirty_area.x1,
            .y1 = s_surfaces[i].info.y + s_surfaces[i].dirty_area.y1,
            .x2 = s_surfaces[i].info.x + s_surfaces[i].dirty_area.x2,
            .y2 = s_surfaces[i].info.y + s_surfaces[i].dirty_area.y2,
        };

        if (screen_area.x1 < composite_area.x1) composite_area.x1 = screen_area.x1;
        if (screen_area.y1 < composite_area.y1) composite_area.y1 = screen_area.y1;
        if (screen_area.x2 > composite_area.x2) composite_area.x2 = screen_area.x2;
        if (screen_area.y2 > composite_area.y2) composite_area.y2 = screen_area.y2;

        s_surfaces[i].dirty = false;
    }

    if (!any_dirty) return ESP_OK;

    /* For now, let each surface flush its own buffer to the display.
     * A full compositor would blend surfaces in Z-order into a single
     * framebuffer, then flush once. This is sufficient for non-overlapping
     * surfaces (status bar + app content + dock). */
    for (int role = SURFACE_ROLE_BACKGROUND; role <= SURFACE_ROLE_DOCK; role++) {
        for (int i = 0; i < MAX_SURFACES; i++) {
            if (!s_surfaces[i].allocated || !s_surfaces[i].info.visible) continue;
            if (s_surfaces[i].info.role != role) continue;

            hal_area_t area = {
                .x1 = s_surfaces[i].info.x,
                .y1 = s_surfaces[i].info.y,
                .x2 = s_surfaces[i].info.x + s_surfaces[i].info.width - 1,
                .y2 = s_surfaces[i].info.y + s_surfaces[i].info.height - 1,
            };

            reg->display->flush(&area, s_surfaces[i].buffer);
        }
    }

    return ESP_OK;
}

uint16_t display_server_get_width(void)
{
    const hal_registry_t *reg = hal_get_registry();
    return (reg && reg->display) ? reg->display->width : 320;
}

uint16_t display_server_get_height(void)
{
    const hal_registry_t *reg = hal_get_registry();
    return (reg && reg->display) ? reg->display->height : 240;
}

hal_display_type_t display_server_get_display_type(void)
{
    const hal_registry_t *reg = hal_get_registry();
    return (reg && reg->display) ? reg->display->type : HAL_DISPLAY_TYPE_LCD;
}

/* ── Input routing ──────────────────────────────────────────────────── */

static void ds_input_handler(const hal_input_event_t *event, void *user_data)
{
    (void)user_data;
    if (!event) return;

    /* WM gets first crack at input */
    if (s_wm && s_wm->on_input) {
        if (s_wm->on_input(event)) {
            return; /* WM consumed the event */
        }
    }

    /* Route touch events to the surface that contains the touch point */
    if (event->type == HAL_INPUT_EVENT_TOUCH_DOWN ||
        event->type == HAL_INPUT_EVENT_TOUCH_UP ||
        event->type == HAL_INPUT_EVENT_TOUCH_MOVE) {

        uint16_t tx = event->touch.x;
        uint16_t ty = event->touch.y;

        /* Check surfaces in reverse Z-order (overlays first) */
        for (int role = SURFACE_ROLE_DOCK; role >= SURFACE_ROLE_BACKGROUND; role--) {
            for (int i = 0; i < MAX_SURFACES; i++) {
                surface_t *s = &s_surfaces[i];
                if (!s->allocated || !s->info.visible || !s->input_cb) continue;
                if (s->info.role != role) continue;

                if (tx >= s->info.x && tx < s->info.x + s->info.width &&
                    ty >= s->info.y && ty < s->info.y + s->info.height) {
                    /* Translate to surface-local coordinates */
                    hal_input_event_t local = *event;
                    local.touch.x = tx - s->info.x;
                    local.touch.y = ty - s->info.y;
                    s->input_cb(&local, s->input_user_data);
                    return;
                }
            }
        }
    }

    /* Key events go to the foreground app's surface */
    if (event->type == HAL_INPUT_EVENT_KEY_DOWN ||
        event->type == HAL_INPUT_EVENT_KEY_UP) {
        for (int i = 0; i < MAX_SURFACES; i++) {
            surface_t *s = &s_surfaces[i];
            if (s->allocated && s->info.visible && s->input_cb &&
                s->info.role == SURFACE_ROLE_APP_CONTENT) {
                s->input_cb(event, s->input_user_data);
                return;
            }
        }
    }
}

esp_err_t display_server_surface_input_cb(surface_id_t id, ds_input_cb_t cb, void *user_data)
{
    surface_t *s = find_surface(id);
    if (!s) return ESP_ERR_NOT_FOUND;
    s->input_cb = cb;
    s->input_user_data = user_data;
    return ESP_OK;
}

/* ── Tick ───────────────────────────────────────────────────────────── */

void display_server_tick(void)
{
    if (!s_initialized) return;

    /* Let the WM render its frame */
    if (s_wm && s_wm->render) {
        s_wm->render();
    }

    /* Composite and flush dirty surfaces */
    display_server_composite();
}
