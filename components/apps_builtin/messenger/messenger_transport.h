/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Messenger transport abstraction layer
 *
 * Each transport backend (LoRa, SMS, BLE, Internet) implements the
 * msg_transport_driver_t interface and registers itself at init time.
 * The messenger UI selects a transport per conversation and calls
 * send/start_receive without knowing the underlying hardware.
 */
#pragma once

#include "esp_err.h"
#include <stdbool.h>

/* ------------------------------------------------------------------ */
/* Transport type enum                                                  */
/* ------------------------------------------------------------------ */

typedef enum {
    MSG_TRANSPORT_LORA     = 0,  /* Direct LoRa broadcast            */
    MSG_TRANSPORT_SMS      = 1,  /* SMS via 4G modem (A7682E)        */
    MSG_TRANSPORT_BLE      = 2,  /* BLE relay through phone companion */
    MSG_TRANSPORT_INTERNET = 3,  /* HTTP/WebSocket (future)          */
    MSG_TRANSPORT_COUNT
} msg_transport_t;

/* ------------------------------------------------------------------ */
/* RX callback — fired on the LVGL task via lv_async_call             */
/* ------------------------------------------------------------------ */

/*
 * msg_rx_cb_t — called when a message arrives on any transport.
 *
 * transport  — which backend received the message
 * sender     — human-readable sender string (e.g. "Node-1A2B", "+447700900123")
 * text       — null-terminated message text
 */
typedef void (*msg_rx_cb_t)(msg_transport_t transport,
                            const char *sender,
                            const char *text);

/* ------------------------------------------------------------------ */
/* Transport driver interface                                           */
/* ------------------------------------------------------------------ */

typedef struct {
    msg_transport_t  type;
    const char      *name;          /* Short display name: "LoRa", "SMS", "BLE", "Internet" */
    const char      *icon;          /* ASCII prefix shown in conversation list               */

    /*
     * is_available() — returns true if the hardware/connection needed by
     * this transport is present and ready.  Called on the LVGL task.
     */
    bool (*is_available)(void);

    /*
     * send() — transmit text to dest.
     *   dest  may be NULL/empty for broadcast transports (LoRa).
     *   Returns ESP_OK on success, ESP_ERR_NOT_SUPPORTED if unavailable.
     */
    esp_err_t (*send)(const char *dest, const char *text);

    /*
     * start_receive() — begin listening; cb will be called for every
     * inbound message.  Idempotent — safe to call when already active.
     */
    esp_err_t (*start_receive)(msg_rx_cb_t cb);

    /*
     * stop_receive() — stop listening.  Safe to call when already stopped.
     */
    void (*stop_receive)(void);
} msg_transport_driver_t;

/* ------------------------------------------------------------------ */
/* Registry API                                                         */
/* ------------------------------------------------------------------ */

/*
 * messenger_transport_init() — register all built-in transports.
 * Must be called once before any other transport function.
 */
void messenger_transport_init(void);

/*
 * messenger_register_transport() — add a transport to the registry.
 * driver must point to a statically-allocated driver struct.
 */
void messenger_register_transport(const msg_transport_driver_t *driver);

/*
 * messenger_get_transport() — look up a driver by type.
 * Returns NULL if the type has not been registered.
 */
const msg_transport_driver_t *messenger_get_transport(msg_transport_t type);

/*
 * messenger_get_available_transports() — fill out[] with pointers to
 * every registered driver whose is_available() returns true.
 * Returns the number of entries written (≤ max).
 */
int messenger_get_available_transports(const msg_transport_driver_t **out, int max);
