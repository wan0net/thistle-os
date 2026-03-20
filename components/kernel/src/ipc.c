#include "thistle/ipc.h"
#include "thistle/kernel.h"

#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"

#include <string.h>

static const char *TAG = "ipc";

#define IPC_HANDLER_MAX 16

typedef struct {
    uint32_t      msg_type;
    ipc_handler_t handler;
    void         *user_data;
    bool          active;
} ipc_handler_entry_t;

static QueueHandle_t        s_queue = NULL;
static ipc_handler_entry_t  s_handlers[IPC_HANDLER_MAX];
static int                  s_handler_count = 0;

esp_err_t ipc_init(void)
{
    s_queue = xQueueCreate(IPC_QUEUE_DEPTH, sizeof(ipc_message_t));
    if (s_queue == NULL) {
        ESP_LOGE(TAG, "Failed to create IPC queue");
        return ESP_ERR_NO_MEM;
    }

    memset(s_handlers, 0, sizeof(s_handlers));
    s_handler_count = 0;

    ESP_LOGI(TAG, "IPC initialized (queue depth: %d, max handlers: %d)",
             IPC_QUEUE_DEPTH, IPC_HANDLER_MAX);
    return ESP_OK;
}

esp_err_t ipc_send(const ipc_message_t *msg)
{
    if (msg == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    if (s_queue == NULL) {
        ESP_LOGE(TAG, "ipc_send: IPC not initialized");
        return ESP_ERR_INVALID_STATE;
    }

    /* Dispatch to registered handlers for this message type first */
    for (int i = 0; i < IPC_HANDLER_MAX; i++) {
        if (s_handlers[i].active && s_handlers[i].msg_type == msg->msg_type) {
            s_handlers[i].handler(msg, s_handlers[i].user_data);
        }
    }

    /* Also enqueue for ipc_recv() callers */
    if (xQueueSend(s_queue, msg, pdMS_TO_TICKS(10)) != pdTRUE) {
        ESP_LOGW(TAG, "ipc_send: queue full, message type %" PRIu32 " dropped", msg->msg_type);
        return ESP_ERR_TIMEOUT;
    }

    return ESP_OK;
}

esp_err_t ipc_recv(ipc_message_t *msg, uint32_t timeout_ms)
{
    if (msg == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    if (s_queue == NULL) {
        ESP_LOGE(TAG, "ipc_recv: IPC not initialized");
        return ESP_ERR_INVALID_STATE;
    }

    TickType_t ticks = (timeout_ms == 0) ? 0 : pdMS_TO_TICKS(timeout_ms);
    if (xQueueReceive(s_queue, msg, ticks) != pdTRUE) {
        return ESP_ERR_TIMEOUT;
    }

    return ESP_OK;
}

esp_err_t ipc_register_handler(uint32_t msg_type, ipc_handler_t handler, void *user_data)
{
    if (handler == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    for (int i = 0; i < IPC_HANDLER_MAX; i++) {
        if (!s_handlers[i].active) {
            s_handlers[i].msg_type  = msg_type;
            s_handlers[i].handler   = handler;
            s_handlers[i].user_data = user_data;
            s_handlers[i].active    = true;
            s_handler_count++;
            ESP_LOGI(TAG, "Registered handler for msg_type %" PRIu32 " (slot %d)", msg_type, i);
            return ESP_OK;
        }
    }

    ESP_LOGE(TAG, "ipc_register_handler: no free slots (max %d)", IPC_HANDLER_MAX);
    return ESP_ERR_NO_MEM;
}
