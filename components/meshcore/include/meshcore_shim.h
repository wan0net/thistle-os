// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — MeshCore C shim API
//
// Exposes the MeshCore mesh protocol via a flat C ABI so the Rust kernel
// and ThistleOS apps can call it through the syscall table.
//
// The shim bridges MeshCore's C++ classes to our HAL radio vtable,
// meaning mesh routing works with ANY registered radio driver.
#pragma once

#include <stdint.h>
#include <stdbool.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

// ── Mesh node types ─────────────────────────────────────────────────

#define MESHCORE_NODE_CLIENT    0
#define MESHCORE_NODE_REPEATER  1

// ── Message send result ─────────────────────────────────────────────

#define MESHCORE_SEND_FAILED       0
#define MESHCORE_SEND_FLOOD        1
#define MESHCORE_SEND_DIRECT       2

// ── Contact info (C-compatible) ─────────────────────────────────────

#define MESHCORE_NAME_MAX       32
#define MESHCORE_PUBKEY_SIZE    32
#define MESHCORE_MAX_CONTACTS   32

typedef struct {
    uint8_t  pub_key[MESHCORE_PUBKEY_SIZE];
    char     name[MESHCORE_NAME_MAX];
    uint8_t  name_len;
    uint8_t  type;          // node type (client/repeater)
    int8_t   last_rssi;
    uint8_t  path_len;      // hops to this contact
    uint32_t last_seen;     // timestamp
    double   lat;           // last known position
    double   lon;
    bool     has_position;
} meshcore_contact_t;

// ── Message received callback ───────────────────────────────────────

typedef void (*meshcore_msg_cb_t)(
    const meshcore_contact_t* sender,
    uint32_t timestamp,
    const char* text,
    void* user_data
);

typedef void (*meshcore_contact_cb_t)(
    const meshcore_contact_t* contact,
    bool is_new,
    void* user_data
);

// ── Core API ────────────────────────────────────────────────────────

/**
 * Initialize the MeshCore mesh protocol.
 * Uses the registered HAL radio driver for packet transmission.
 * @param node_name  Display name for this node (max 32 chars)
 * @param node_type  MESHCORE_NODE_CLIENT or MESHCORE_NODE_REPEATER
 * @return ESP_OK on success
 */
esp_err_t meshcore_init(const char* node_name, uint8_t node_type);

/**
 * Shut down mesh protocol, release resources.
 */
esp_err_t meshcore_deinit(void);

/**
 * Must be called periodically (e.g. every 10ms) to process
 * incoming packets, retransmit, and maintain routing tables.
 */
esp_err_t meshcore_loop(void);

/**
 * Send a text message to a specific contact.
 * @return MESHCORE_SEND_FAILED, MESHCORE_SEND_FLOOD, or MESHCORE_SEND_DIRECT
 */
int meshcore_send_message(const uint8_t* dest_pub_key, const char* text);

/**
 * Broadcast a self-advertisement so other nodes discover us.
 */
esp_err_t meshcore_send_advert(void);

/**
 * Broadcast a self-advertisement with GPS position.
 */
esp_err_t meshcore_send_advert_with_position(double lat, double lon);

// ── Contact management ──────────────────────────────────────────────

/**
 * Get number of known contacts.
 */
int meshcore_get_contact_count(void);

/**
 * Get contact by index (0-based).
 * @return ESP_OK on success, ESP_ERR_NOT_FOUND if index out of range
 */
esp_err_t meshcore_get_contact(int index, meshcore_contact_t* out);

/**
 * Find contact by public key.
 * @return index on success, -1 if not found
 */
int meshcore_find_contact(const uint8_t* pub_key);

// ── Callbacks ───────────────────────────────────────────────────────

/**
 * Register callback for incoming messages.
 */
void meshcore_set_message_callback(meshcore_msg_cb_t cb, void* user_data);

/**
 * Register callback for discovered/updated contacts.
 */
void meshcore_set_contact_callback(meshcore_contact_cb_t cb, void* user_data);

// ── Identity ────────────────────────────────────────────────────────

/**
 * Get this node's public key.
 */
esp_err_t meshcore_get_self_pub_key(uint8_t* out_key);

/**
 * Get this node's display name.
 */
const char* meshcore_get_self_name(void);

// ── Stats ───────────────────────────────────────────────────────────

typedef struct {
    uint32_t packets_sent;
    uint32_t packets_received;
    uint32_t packets_forwarded;
    uint32_t messages_sent;
    uint32_t messages_received;
    uint32_t contacts_discovered;
} meshcore_stats_t;

esp_err_t meshcore_get_stats(meshcore_stats_t* out);

#ifdef __cplusplus
}
#endif
