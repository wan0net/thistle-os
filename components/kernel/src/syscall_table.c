#include "thistle/syscall.h"
#include "thistle/kernel.h"
#include "thistle/ipc.h"
#include "thistle/event.h"

#include "hal/board.h"

#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include <string.h>
#include <stdlib.h>
#include <inttypes.h>

static const char *TAG = "syscall";

/* TODO: Permission enforcement
 * Each syscall that accesses a sensitive resource should check
 * permissions_check(current_app_id, PERM_xxx) before proceeding.
 * Requires: task-to-app mapping (get current app from FreeRTOS task handle).
 * Syscalls needing permission checks:
 *   - thistle_radio_send/recv -> PERM_RADIO
 *   - thistle_gps_* -> PERM_GPS
 *   - thistle_fs_* -> PERM_STORAGE
 *   - thistle_wifi, thistle_http, thistle_ble -> PERM_NETWORK
 *   - thistle_audio_* -> PERM_AUDIO
 */

/* --------------------------------------------------------------------------
 * System syscall implementations
 * -------------------------------------------------------------------------- */

static void thistle_log(const char *tag, const char *msg)
{
    ESP_LOGI(tag ? tag : "app", "%s", msg ? msg : "");
}

static uint32_t thistle_millis(void)
{
    return kernel_uptime_ms();
}

static void thistle_delay(uint32_t ms)
{
    vTaskDelay(pdMS_TO_TICKS(ms));
}

static void *thistle_malloc(size_t size)
{
    return malloc(size);
}

static void thistle_free(void *ptr)
{
    free(ptr);
}

static void *thistle_realloc(void *ptr, size_t size)
{
    return realloc(ptr, size);
}

/* --------------------------------------------------------------------------
 * Display syscall stubs
 * -------------------------------------------------------------------------- */

static uint16_t thistle_display_get_width(void)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->display == NULL) {
        return 0;
    }
    return reg->display->width;
}

static uint16_t thistle_display_get_height(void)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->display == NULL) {
        return 0;
    }
    return reg->display->height;
}

/* --------------------------------------------------------------------------
 * Input syscall stubs
 * -------------------------------------------------------------------------- */

static esp_err_t thistle_input_register_cb(void *cb, void *user_data)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->input_count == 0 || reg->inputs[0] == NULL) {
        ESP_LOGW(TAG, "thistle_input_register_cb: no input driver available");
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (reg->inputs[0]->register_callback == NULL) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    return reg->inputs[0]->register_callback((hal_input_cb_t)cb, user_data);
}

/* --------------------------------------------------------------------------
 * Radio syscall stubs
 * -------------------------------------------------------------------------- */

static esp_err_t thistle_radio_send(const uint8_t *data, size_t len)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->radio == NULL) {
        ESP_LOGW(TAG, "thistle_radio_send: no radio driver");
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (reg->radio->send == NULL) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    return reg->radio->send(data, len);
}

static esp_err_t thistle_radio_start_rx(void *cb, void *user_data)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->radio == NULL) {
        ESP_LOGW(TAG, "thistle_radio_start_rx: no radio driver");
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (reg->radio->start_receive == NULL) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    return reg->radio->start_receive((hal_radio_rx_cb_t)cb, user_data);
}

static esp_err_t thistle_radio_set_freq(uint32_t freq_hz)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->radio == NULL) {
        ESP_LOGW(TAG, "thistle_radio_set_freq: no radio driver");
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (reg->radio->set_frequency == NULL) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    return reg->radio->set_frequency(freq_hz);
}

/* --------------------------------------------------------------------------
 * GPS syscall stubs
 * -------------------------------------------------------------------------- */

static esp_err_t thistle_gps_get_position(hal_gps_position_t *pos)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->gps == NULL) {
        ESP_LOGW(TAG, "thistle_gps_get_position: no GPS driver");
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (reg->gps->get_position == NULL) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    return reg->gps->get_position(pos);
}

static esp_err_t thistle_gps_enable(void)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->gps == NULL) {
        ESP_LOGW(TAG, "thistle_gps_enable: no GPS driver");
        return ESP_ERR_NOT_SUPPORTED;
    }
    if (reg->gps->enable == NULL) {
        return ESP_ERR_NOT_SUPPORTED;
    }
    return reg->gps->enable();
}

/* --------------------------------------------------------------------------
 * Storage syscall stubs (filesystem — open/read/write/close via stdio)
 * Apps use standard FILE* via newlib; these are thin wrappers to confirm
 * a storage driver is mounted before delegating to libc.
 * -------------------------------------------------------------------------- */

static void *thistle_fs_open(const char *path, const char *mode)
{
    const hal_registry_t *reg = hal_get_registry();
    bool any_mounted = false;
    for (int i = 0; i < reg->storage_count; i++) {
        if (reg->storage[i] != NULL && reg->storage[i]->is_mounted != NULL) {
            if (reg->storage[i]->is_mounted()) {
                any_mounted = true;
                break;
            }
        }
    }
    if (!any_mounted) {
        ESP_LOGW(TAG, "thistle_fs_open: no storage mounted");
        return NULL;
    }
    return (void *)fopen(path, mode);
}

static int thistle_fs_read(void *buf, size_t size, size_t count, void *stream)
{
    return (int)fread(buf, size, count, (FILE *)stream);
}

static int thistle_fs_write(const void *buf, size_t size, size_t count, void *stream)
{
    return (int)fwrite(buf, size, count, (FILE *)stream);
}

static int thistle_fs_close(void *stream)
{
    return fclose((FILE *)stream);
}

/* --------------------------------------------------------------------------
 * IPC/Event syscall shims (forward to kernel subsystems)
 * -------------------------------------------------------------------------- */

static esp_err_t thistle_msg_send(const ipc_message_t *msg)
{
    return ipc_send(msg);
}

static esp_err_t thistle_msg_recv(ipc_message_t *msg, uint32_t timeout_ms)
{
    return ipc_recv(msg, timeout_ms);
}

static esp_err_t thistle_event_subscribe(event_type_t type, event_handler_t handler, void *user_data)
{
    return event_subscribe(type, handler, user_data);
}

static esp_err_t thistle_event_publish(const event_t *event)
{
    return event_publish(event);
}

/* --------------------------------------------------------------------------
 * Power syscall stubs
 * -------------------------------------------------------------------------- */

static uint16_t thistle_power_get_battery_mv(void)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->power == NULL || reg->power->get_battery_mv == NULL) {
        ESP_LOGW(TAG, "thistle_power_get_battery_mv: no power driver");
        return 0;
    }
    return reg->power->get_battery_mv();
}

static uint8_t thistle_power_get_battery_pct(void)
{
    const hal_registry_t *reg = hal_get_registry();
    if (reg->power == NULL || reg->power->get_battery_percent == NULL) {
        ESP_LOGW(TAG, "thistle_power_get_battery_pct: no power driver");
        return 0;
    }
    return reg->power->get_battery_percent();
}

/* --------------------------------------------------------------------------
 * Syscall table
 * -------------------------------------------------------------------------- */

static syscall_entry_t s_table[] = {
    /* System */
    { "thistle_log",                    (void *)thistle_log                 },
    { "thistle_millis",                 (void *)thistle_millis              },
    { "thistle_delay",                  (void *)thistle_delay               },
    { "thistle_malloc",                 (void *)thistle_malloc              },
    { "thistle_free",                   (void *)thistle_free                },
    { "thistle_realloc",                (void *)thistle_realloc             },

    /* Display */
    { "thistle_display_get_width",      (void *)thistle_display_get_width   },
    { "thistle_display_get_height",     (void *)thistle_display_get_height  },

    /* Input */
    { "thistle_input_register_cb",      (void *)thistle_input_register_cb   },

    /* Radio */
    { "thistle_radio_send",             (void *)thistle_radio_send          },
    { "thistle_radio_start_rx",         (void *)thistle_radio_start_rx      },
    { "thistle_radio_set_freq",         (void *)thistle_radio_set_freq      },

    /* GPS */
    { "thistle_gps_get_position",       (void *)thistle_gps_get_position    },
    { "thistle_gps_enable",             (void *)thistle_gps_enable          },

    /* Storage */
    { "thistle_fs_open",                (void *)thistle_fs_open             },
    { "thistle_fs_read",                (void *)thistle_fs_read             },
    { "thistle_fs_write",               (void *)thistle_fs_write            },
    { "thistle_fs_close",               (void *)thistle_fs_close            },

    /* IPC */
    { "thistle_msg_send",               (void *)thistle_msg_send            },
    { "thistle_msg_recv",               (void *)thistle_msg_recv            },
    { "thistle_event_subscribe",        (void *)thistle_event_subscribe     },
    { "thistle_event_publish",          (void *)thistle_event_publish       },

    /* Power */
    { "thistle_power_get_battery_mv",   (void *)thistle_power_get_battery_mv  },
    { "thistle_power_get_battery_pct",  (void *)thistle_power_get_battery_pct },
};

static const size_t s_table_count = sizeof(s_table) / sizeof(s_table[0]);

/* --------------------------------------------------------------------------
 * Public API
 * -------------------------------------------------------------------------- */

esp_err_t syscall_table_init(void)
{
    ESP_LOGI(TAG, "Syscall table initialized with %zu entries", s_table_count);
    return ESP_OK;
}

const syscall_entry_t *syscall_table_get(void)
{
    return s_table;
}

size_t syscall_table_count(void)
{
    return s_table_count;
}

void *syscall_resolve(const char *name)
{
    if (name == NULL) {
        return NULL;
    }
    for (size_t i = 0; i < s_table_count; i++) {
        if (strcmp(s_table[i].name, name) == 0) {
            return s_table[i].func_ptr;
        }
    }
    ESP_LOGW(TAG, "syscall_resolve: unknown symbol '%s'", name);
    return NULL;
}
