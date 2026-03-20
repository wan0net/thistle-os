#include "thistle/event.h"
#include "thistle/kernel.h"

#include "esp_log.h"

#include <string.h>

static const char *TAG = "event";

#define EVENT_SUBSCRIBERS_MAX 8

typedef struct {
    event_handler_t handler;
    void           *user_data;
    bool            active;
} event_subscriber_t;

static event_subscriber_t s_subscribers[EVENT_MAX][EVENT_SUBSCRIBERS_MAX];

esp_err_t event_bus_init(void)
{
    memset(s_subscribers, 0, sizeof(s_subscribers));
    ESP_LOGI(TAG, "Event bus initialized (%d event types, %d subscribers each)",
             (int)EVENT_MAX, EVENT_SUBSCRIBERS_MAX);
    return ESP_OK;
}

esp_err_t event_subscribe(event_type_t type, event_handler_t handler, void *user_data)
{
    if (type < 0 || type >= EVENT_MAX) {
        ESP_LOGE(TAG, "event_subscribe: invalid event type %d", (int)type);
        return ESP_ERR_INVALID_ARG;
    }
    if (handler == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    for (int i = 0; i < EVENT_SUBSCRIBERS_MAX; i++) {
        if (!s_subscribers[type][i].active) {
            s_subscribers[type][i].handler   = handler;
            s_subscribers[type][i].user_data = user_data;
            s_subscribers[type][i].active    = true;
            return ESP_OK;
        }
    }

    ESP_LOGE(TAG, "event_subscribe: no free subscriber slots for type %d (max %d)",
             (int)type, EVENT_SUBSCRIBERS_MAX);
    return ESP_ERR_NO_MEM;
}

esp_err_t event_unsubscribe(event_type_t type, event_handler_t handler)
{
    if (type < 0 || type >= EVENT_MAX) {
        ESP_LOGE(TAG, "event_unsubscribe: invalid event type %d", (int)type);
        return ESP_ERR_INVALID_ARG;
    }
    if (handler == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    for (int i = 0; i < EVENT_SUBSCRIBERS_MAX; i++) {
        if (s_subscribers[type][i].active &&
            s_subscribers[type][i].handler == handler) {
            memset(&s_subscribers[type][i], 0, sizeof(event_subscriber_t));
            return ESP_OK;
        }
    }

    ESP_LOGW(TAG, "event_unsubscribe: handler not found for type %d", (int)type);
    return ESP_ERR_NOT_FOUND;
}

esp_err_t event_publish(const event_t *event)
{
    if (event == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    if (event->type < 0 || event->type >= EVENT_MAX) {
        ESP_LOGE(TAG, "event_publish: invalid event type %d", (int)event->type);
        return ESP_ERR_INVALID_ARG;
    }

    int dispatched = 0;
    for (int i = 0; i < EVENT_SUBSCRIBERS_MAX; i++) {
        if (s_subscribers[event->type][i].active) {
            s_subscribers[event->type][i].handler(event,
                s_subscribers[event->type][i].user_data);
            dispatched++;
        }
    }

    if (dispatched == 0) {
        /* Not an error — events may have zero subscribers */
        ESP_LOGD(TAG, "event_publish: type %d, no subscribers", (int)event->type);
    }

    return ESP_OK;
}

esp_err_t event_publish_simple(event_type_t type)
{
    event_t ev = {
        .type      = type,
        .timestamp = kernel_uptime_ms(),
        .data      = NULL,
        .data_len  = 0,
    };
    return event_publish(&ev);
}
