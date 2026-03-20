#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define IPC_MSG_MAX_DATA 256
#define IPC_QUEUE_DEPTH  16

typedef struct {
    uint32_t src_app;
    uint32_t dst_app;      // 0 = broadcast
    uint32_t msg_type;
    uint8_t  data[IPC_MSG_MAX_DATA];
    size_t   data_len;
    uint32_t timestamp;
} ipc_message_t;

typedef void (*ipc_handler_t)(const ipc_message_t *msg, void *user_data);

/* Initialize IPC subsystem */
esp_err_t ipc_init(void);

/* Send a message to a specific app or broadcast (dst_app=0) */
esp_err_t ipc_send(const ipc_message_t *msg);

/* Receive next message (blocks up to timeout_ms, 0=no wait) */
esp_err_t ipc_recv(ipc_message_t *msg, uint32_t timeout_ms);

/* Register a handler for a specific message type */
esp_err_t ipc_register_handler(uint32_t msg_type, ipc_handler_t handler, void *user_data);
