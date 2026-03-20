#include "ui/manager.h"
#include "ui/theme.h"
#include "ui/statusbar.h"
#include "ui/epaper_refresh.h"
#include "hal/board.h"
#include "hal/input.h"
#include "thistle/app_manager.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include <string.h>
#include <inttypes.h>

static const char *TAG = "ui_mgr";

#define DISPLAY_WIDTH   320
#define DISPLAY_HEIGHT  240
#define STATUSBAR_H     24
#define LVGL_TASK_PERIOD_MS 10

/* Draw buffer — L8 (8-bit grayscale) format for e-paper compatibility.
 * Double-buffer: allocate two halves so LVGL can render while we flush. */
static uint8_t s_draw_buf1[DISPLAY_WIDTH * DISPLAY_HEIGHT / 2];
static uint8_t s_draw_buf2[DISPLAY_WIDTH * DISPLAY_HEIGHT / 2];

static lv_display_t   *s_display  = NULL;
static lv_obj_t       *s_screen   = NULL;
static lv_obj_t       *s_statusbar_cont = NULL;
static lv_obj_t       *s_app_area = NULL;
static SemaphoreHandle_t s_lvgl_mutex = NULL;

/* -------------------------------------------------------------------------
 * Input device state — written by HAL callbacks, read by LVGL read callbacks.
 * Both callbacks run from lvgl_task (poll loop → lv_timer_handler), so no
 * additional locking is required.
 * ------------------------------------------------------------------------- */
static lv_indev_t *s_touch_indev = NULL;
static struct {
    int16_t x, y;
    lv_indev_state_t state;
} s_touch_state = { 0, 0, LV_INDEV_STATE_RELEASED };

static lv_indev_t *s_kbd_indev = NULL;
static struct {
    uint32_t key;
    lv_indev_state_t state;
} s_kbd_state = { 0, LV_INDEV_STATE_RELEASED };

/* HAL input callback — handles all event types from any registered driver.
 * Touch events update s_touch_state; key events update s_kbd_state.
 * A single callback is registered on every driver so that combined drivers
 * (e.g. the SDL2 simulator driver) work correctly regardless of is_touch. */
static void ui_input_hal_cb(const hal_input_event_t *event, void *user_data)
{
    (void)user_data;
    switch (event->type) {
        case HAL_INPUT_EVENT_TOUCH_DOWN:
        case HAL_INPUT_EVENT_TOUCH_MOVE:
            s_touch_state.x     = (int16_t)event->touch.x;
            s_touch_state.y     = (int16_t)event->touch.y;
            s_touch_state.state = LV_INDEV_STATE_PRESSED;
            break;
        case HAL_INPUT_EVENT_TOUCH_UP:
            s_touch_state.state = LV_INDEV_STATE_RELEASED;
            break;
        case HAL_INPUT_EVENT_KEY_DOWN: {
            uint32_t lv_key = event->key.keycode;
            if      (lv_key == '\n')  lv_key = LV_KEY_ENTER;
            else if (lv_key == '\b')  lv_key = LV_KEY_BACKSPACE;
            else if (lv_key == 0x1B) lv_key = LV_KEY_ESC;
            else if (lv_key == '\t')  lv_key = LV_KEY_NEXT;

            /* Global ESC handler: if foreground app is not the launcher,
             * switch back to the launcher regardless of focus. */
            if (lv_key == LV_KEY_ESC) {
                app_manager_launch("com.thistle.launcher");
            }

            s_kbd_state.key   = lv_key;
            s_kbd_state.state = LV_INDEV_STATE_PRESSED;
            break;
        }
        case HAL_INPUT_EVENT_KEY_UP:
            s_kbd_state.state = LV_INDEV_STATE_RELEASED;
            break;
        default:
            break;
    }
}

static void ui_touch_read_cb(lv_indev_t *indev, lv_indev_data_t *data)
{
    (void)indev;
    data->point.x = s_touch_state.x;
    data->point.y = s_touch_state.y;
    data->state   = s_touch_state.state;
}

static void ui_kbd_read_cb(lv_indev_t *indev, lv_indev_data_t *data)
{
    (void)indev;
    data->key   = s_kbd_state.key;
    data->state = s_kbd_state.state;
}

/* -------------------------------------------------------------------------
 * LVGL flush callback — forwards pixel data to HAL display driver
 * ------------------------------------------------------------------------- */
static void ui_flush_cb(lv_display_t *disp, const lv_area_t *area, uint8_t *px_map)
{
    const hal_registry_t *reg = hal_get_registry();

    if (reg && reg->display && reg->display->flush) {
        hal_area_t hal_area = {
            .x1 = (uint16_t)area->x1,
            .y1 = (uint16_t)area->y1,
            .x2 = (uint16_t)area->x2,
            .y2 = (uint16_t)area->y2,
        };

        esp_err_t err = reg->display->flush(&hal_area, px_map);
        if (err != ESP_OK) {
            ESP_LOGE(TAG, "HAL flush failed: %s", esp_err_to_name(err));
        }

        /* Track dirty region for e-paper batching */
        epaper_refresh_mark_dirty(hal_area.x1, hal_area.y1, hal_area.x2, hal_area.y2);
    }

    lv_display_flush_ready(disp);
}

/* -------------------------------------------------------------------------
 * LVGL tick provider — called by esp_timer every 1 ms
 * ------------------------------------------------------------------------- */
static void lvgl_tick_cb(void *arg)
{
    (void)arg;
    lv_tick_inc(1);
}

/* -------------------------------------------------------------------------
 * LVGL timer handler task — runs lv_timer_handler() every 10 ms
 * ------------------------------------------------------------------------- */
static void lvgl_task(void *arg)
{
    (void)arg;
    const hal_registry_t *reg = hal_get_registry();
    while (1) {
        /* Poll all registered input drivers so they fire HAL callbacks */
        if (reg) {
            for (int i = 0; i < reg->input_count; i++) {
                if (reg->inputs[i] && reg->inputs[i]->poll) {
                    reg->inputs[i]->poll();
                }
            }
        }

        if (xSemaphoreTake(s_lvgl_mutex, pdMS_TO_TICKS(10)) == pdTRUE) {
            lv_timer_handler();
            xSemaphoreGive(s_lvgl_mutex);
        }
        vTaskDelay(pdMS_TO_TICKS(LVGL_TASK_PERIOD_MS));
    }
}

/* -------------------------------------------------------------------------
 * Public API
 * ------------------------------------------------------------------------- */

esp_err_t ui_manager_init(void)
{
    ESP_LOGI(TAG, "initializing UI manager");

    /* 1. Initialize LVGL */
    lv_init();

    /* 2. Create LVGL display */
    s_display = lv_display_create(DISPLAY_WIDTH, DISPLAY_HEIGHT);
    if (s_display == NULL) {
        ESP_LOGE(TAG, "lv_display_create failed");
        return ESP_FAIL;
    }

    /* 3. Set draw buffers (double-buffered, partial) */
    lv_display_set_buffers(s_display,
                           s_draw_buf1, s_draw_buf2,
                           sizeof(s_draw_buf1),
                           LV_DISPLAY_RENDER_MODE_PARTIAL);

    /* 4. Set flush callback */
    lv_display_set_flush_cb(s_display, ui_flush_cb);

    /* 4a. Register LVGL pointer input device (touch / mouse) */
    s_touch_indev = lv_indev_create();
    lv_indev_set_type(s_touch_indev, LV_INDEV_TYPE_POINTER);
    lv_indev_set_read_cb(s_touch_indev, ui_touch_read_cb);

    /* 4b. Register LVGL keypad input device (hardware keyboard) */
    s_kbd_indev = lv_indev_create();
    lv_indev_set_type(s_kbd_indev, LV_INDEV_TYPE_KEYPAD);
    lv_indev_set_read_cb(s_kbd_indev, ui_kbd_read_cb);

    /* 4c. Wire HAL input callbacks to every registered input driver.
     *     A single combined callback handles both touch and key events so
     *     that drivers which emit both types (e.g. SDL2 simulator) work
     *     correctly regardless of their is_touch flag. */
    {
        const hal_registry_t *reg = hal_get_registry();
        if (reg) {
            for (int i = 0; i < reg->input_count; i++) {
                if (reg->inputs[i] && reg->inputs[i]->register_callback) {
                    reg->inputs[i]->register_callback(ui_input_hal_cb, NULL);
                }
            }
        }
    }

    /* 5. Create mutex */
    s_lvgl_mutex = xSemaphoreCreateMutex();
    if (s_lvgl_mutex == NULL) {
        ESP_LOGE(TAG, "failed to create LVGL mutex");
        return ESP_ERR_NO_MEM;
    }

    /* 6a. Start 1 ms tick timer */
    const esp_timer_create_args_t tick_timer_args = {
        .callback        = lvgl_tick_cb,
        .name            = "lvgl_tick",
        .dispatch_method = ESP_TIMER_TASK,
    };
    esp_timer_handle_t tick_timer;
    esp_err_t err = esp_timer_create(&tick_timer_args, &tick_timer);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "failed to create tick timer: %s", esp_err_to_name(err));
        return err;
    }
    esp_timer_start_periodic(tick_timer, 1000); /* 1 ms */

    /* 6b. Start LVGL handler task */
    BaseType_t ret = xTaskCreate(lvgl_task, "lvgl", 4096, NULL, 5, NULL);
    if (ret != pdPASS) {
        ESP_LOGE(TAG, "failed to create LVGL task");
        return ESP_ERR_NO_MEM;
    }

    /* 7. Build screen layout */
    s_screen = lv_display_get_screen_active(s_display);
    lv_obj_set_size(s_screen, DISPLAY_WIDTH, DISPLAY_HEIGHT);

    /* Status bar container — top 24 px */
    s_statusbar_cont = lv_obj_create(s_screen);
    lv_obj_set_pos(s_statusbar_cont, 0, 0);
    lv_obj_set_size(s_statusbar_cont, DISPLAY_WIDTH, STATUSBAR_H);
    lv_obj_set_style_pad_all(s_statusbar_cont, 0, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_statusbar_cont, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_statusbar_cont, 0, LV_PART_MAIN);

    /* App content area — below status bar */
    s_app_area = lv_obj_create(s_screen);
    lv_obj_set_pos(s_app_area, 0, STATUSBAR_H);
    lv_obj_set_size(s_app_area, DISPLAY_WIDTH, DISPLAY_HEIGHT - STATUSBAR_H);
    lv_obj_set_style_pad_all(s_app_area, 0, LV_PART_MAIN);
    lv_obj_set_style_border_width(s_app_area, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(s_app_area, 0, LV_PART_MAIN);

    /* 8. Apply default theme FIRST (statusbar reads theme colors) */
    err = theme_init(s_display);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "theme_init failed: %s", esp_err_to_name(err));
        return err;
    }

    /* 9. Create status bar widgets (uses theme colors) */
    err = statusbar_create(s_statusbar_cont);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "statusbar_create failed: %s", esp_err_to_name(err));
        return err;
    }

    /* 10. Initialize e-paper refresh tracker */
    err = epaper_refresh_init(DISPLAY_WIDTH, DISPLAY_HEIGHT);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "epaper_refresh_init failed: %s", esp_err_to_name(err));
        return err;
    }

    ESP_LOGI(TAG, "UI manager ready (%dx%d)", DISPLAY_WIDTH, DISPLAY_HEIGHT);

    /* 11. Show splash screen for 2 seconds */
    ui_manager_show_splash(2000);

    return ESP_OK;
}

/* -------------------------------------------------------------------------
 * Splash screen — full-screen overlay that auto-dismisses via esp_timer
 * ------------------------------------------------------------------------- */

static esp_timer_handle_t s_splash_timer = NULL;

static void splash_dismiss_cb(void *arg)
{
    lv_obj_t *splash = (lv_obj_t *)arg;

    ui_manager_lock();
    lv_obj_delete(splash);
    ui_manager_unlock();

    /* Clean up the timer handle so it does not leak. */
    if (s_splash_timer != NULL) {
        esp_timer_delete(s_splash_timer);
        s_splash_timer = NULL;
    }
}

void ui_manager_show_splash(uint32_t duration_ms)
{
    if (s_screen == NULL) {
        ESP_LOGW(TAG, "ui_manager_show_splash called before screen ready");
        return;
    }

    /* Full-screen overlay on top of everything */
    lv_obj_t *splash = lv_obj_create(s_screen);
    lv_obj_set_pos(splash, 0, 0);
    lv_obj_set_size(splash, DISPLAY_WIDTH, DISPLAY_HEIGHT);
    lv_obj_set_style_bg_color(splash, lv_color_white(), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(splash, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_width(splash, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(splash, 0, LV_PART_MAIN);
    lv_obj_set_style_pad_all(splash, 0, LV_PART_MAIN);
    lv_obj_clear_flag(splash, LV_OBJ_FLAG_SCROLLABLE);

    /* Center column layout */
    lv_obj_set_flex_flow(splash, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(splash,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER,
                          LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_row(splash, 8, LV_PART_MAIN);

    /* "ThistleOS" title in large font */
    lv_obj_t *lbl_title = lv_label_create(splash);
    lv_label_set_text(lbl_title, "ThistleOS");
    lv_obj_set_style_text_font(lbl_title, &lv_font_montserrat_22, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_title, lv_color_black(), LV_PART_MAIN);

    /* Version subtitle */
    lv_obj_t *lbl_ver = lv_label_create(splash);
    lv_label_set_text(lbl_ver, "v0.1.0");
    lv_obj_set_style_text_font(lbl_ver, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_set_style_text_color(lbl_ver, lv_color_black(), LV_PART_MAIN);

    /* Bring to front so it covers the app area and status bar */
    lv_obj_move_foreground(splash);

    /* One-shot timer to delete the splash after duration_ms */
    const esp_timer_create_args_t splash_timer_args = {
        .callback        = splash_dismiss_cb,
        .arg             = (void *)splash,
        .name            = "splash_dismiss",
        .dispatch_method = ESP_TIMER_TASK,
    };
    esp_err_t err = esp_timer_create(&splash_timer_args, &s_splash_timer);
    if (err == ESP_OK) {
        esp_timer_start_once(s_splash_timer, (uint64_t)duration_ms * 1000ULL);
    } else {
        ESP_LOGE(TAG, "failed to create splash timer: %s", esp_err_to_name(err));
        s_splash_timer = NULL;
    }

    ESP_LOGI(TAG, "splash screen shown for %" PRIu32 " ms", duration_ms);
}

lv_obj_t *ui_manager_get_app_area(void)
{
    return s_app_area;
}

lv_obj_t *ui_manager_get_screen(void)
{
    return s_screen;
}

void ui_manager_request_refresh(void)
{
    /* Mark full screen dirty — LVGL's next timer tick will trigger flush */
    epaper_refresh_mark_full();
    if (s_display != NULL) {
        lv_obj_invalidate(s_screen);
    }
}

void ui_manager_force_refresh(void)
{
    const hal_registry_t *reg = hal_get_registry();

    epaper_refresh_mark_full();

    if (reg && reg->display) {
        if (reg->display->set_refresh_mode) {
            reg->display->set_refresh_mode(HAL_DISPLAY_REFRESH_FULL);
        }
    }

    if (s_display != NULL) {
        lv_obj_invalidate(s_screen);
    }
}

void ui_manager_lock(void)
{
    if (s_lvgl_mutex != NULL) {
        xSemaphoreTake(s_lvgl_mutex, portMAX_DELAY);
    }
}

void ui_manager_unlock(void)
{
    if (s_lvgl_mutex != NULL) {
        xSemaphoreGive(s_lvgl_mutex);
    }
}
