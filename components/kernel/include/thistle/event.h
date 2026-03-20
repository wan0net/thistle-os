#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>

/* System event types */
typedef enum {
    EVENT_SYSTEM_BOOT = 0,
    EVENT_SYSTEM_SHUTDOWN,
    EVENT_APP_LAUNCHED,
    EVENT_APP_STOPPED,
    EVENT_APP_SWITCHED,
    EVENT_INPUT_KEY,
    EVENT_INPUT_TOUCH,
    EVENT_RADIO_RX,
    EVENT_GPS_FIX,
    EVENT_BATTERY_LOW,
    EVENT_BATTERY_CHARGING,
    EVENT_SD_MOUNTED,
    EVENT_SD_UNMOUNTED,
    EVENT_WIFI_CONNECTED,
    EVENT_WIFI_DISCONNECTED,
    EVENT_MAX
} event_type_t;

typedef struct {
    event_type_t type;
    uint32_t timestamp;
    void *data;
    size_t data_len;
} event_t;

typedef void (*event_handler_t)(const event_t *event, void *user_data);

/* Initialize event bus */
esp_err_t event_bus_init(void);

/* Subscribe to an event type */
esp_err_t event_subscribe(event_type_t type, event_handler_t handler, void *user_data);

/* Unsubscribe */
esp_err_t event_unsubscribe(event_type_t type, event_handler_t handler);

/* Publish an event (dispatches to all subscribers) */
esp_err_t event_publish(const event_t *event);

/* Publish a simple event with no data */
esp_err_t event_publish_simple(event_type_t type);
