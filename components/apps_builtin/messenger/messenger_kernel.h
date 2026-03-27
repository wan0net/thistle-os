/*
 * SPDX-License-Identifier: BSD-3-Clause
 * ThistleOS — Messenger kernel module FFI declarations
 *
 * Declares the extern "C" functions exported by the Rust kernel modules:
 *   contact_manager.rs, burn_timer.rs, msg_crypto.rs, msg_queue.rs
 *
 * These are called from the messenger C code to integrate with the kernel's
 * contact resolution, disappearing messages, encryption, and message queue.
 */
#pragma once

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

/* ------------------------------------------------------------------ */
/* CContactInfo — matches #[repr(C)] struct in contact_manager.rs     */
/* ------------------------------------------------------------------ */

typedef struct {
    uint32_t id;
    uint8_t  name[64];
    uint8_t  callsign[16];
    uint32_t device_id;
    uint8_t  phone[24];
    uint8_t  ble_addr[24];
    uint8_t  public_key[32];
    bool     is_emergency;
} CContactInfo;

/* ------------------------------------------------------------------ */
/* CBurnExpired — matches #[repr(C)] struct in burn_timer.rs          */
/* ------------------------------------------------------------------ */

typedef struct {
    uint8_t conversation_id;
    uint8_t message_index;
} CBurnExpired;

/* ------------------------------------------------------------------ */
/* CQueuedMsgInfo — matches #[repr(C)] struct in msg_queue.rs        */
/* ------------------------------------------------------------------ */

typedef struct {
    uint32_t id;
    uint8_t  transport;
    uint8_t  dest[32];
    uint8_t  dest_len;
    uint16_t payload_len;
    uint8_t  priority;
    uint32_t retry_count;
    uint32_t max_retries;
    uint8_t  status;
    uint64_t ttl_ms;
} CQueuedMsgInfo;

/* ------------------------------------------------------------------ */
/* Contact manager (contact_manager.rs)                                */
/* ------------------------------------------------------------------ */

extern int rs_contact_manager_init(void);
extern int rs_contact_count(void);
extern int rs_contact_get_at(uint32_t index, CContactInfo *out);
extern int rs_contact_find_by_device_id(uint32_t device_id, CContactInfo *out);
extern int rs_contact_find_by_phone(const char *phone, CContactInfo *out);

/* ------------------------------------------------------------------ */
/* Burn timer (burn_timer.rs)                                          */
/* ------------------------------------------------------------------ */

extern int rs_burn_timer_init(void);
extern int rs_burn_timer_set(uint8_t conv_id, uint8_t msg_idx, uint64_t burn_after_ms);
extern int rs_burn_timer_tick(uint64_t now_ms);
extern int rs_burn_timer_get_expired(CBurnExpired *out, uint32_t max);
extern int rs_burn_timer_cancel_conversation(uint8_t conv_id);

/* ------------------------------------------------------------------ */
/* Message crypto (msg_crypto.rs)                                      */
/* ------------------------------------------------------------------ */

extern int  rs_msg_crypto_init(void);
extern bool rs_msg_crypto_is_active(uint32_t contact_id);
extern int  rs_msg_crypto_encrypt(uint32_t contact_id, const uint8_t *pt, size_t pt_len,
                                  uint8_t *ct, size_t ct_max);
extern int  rs_msg_crypto_decrypt(uint32_t contact_id, const uint8_t *ct, size_t ct_len,
                                  uint8_t *pt, size_t pt_max);

/* ------------------------------------------------------------------ */
/* Message queue (msg_queue.rs)                                        */
/* ------------------------------------------------------------------ */

extern int rs_msg_queue_init(void);
extern int rs_msg_queue_enqueue(uint8_t transport, const uint8_t *dest, uint8_t dest_len,
                                const uint8_t *payload, uint16_t payload_len, uint8_t priority);
extern int rs_msg_queue_tick(uint64_t now_ms);
extern int rs_msg_queue_get_ready(CQueuedMsgInfo *out, uint32_t max);
extern int rs_msg_queue_get_payload(uint32_t id, uint8_t *out, uint16_t max_len);
extern int rs_msg_queue_mark_sent(uint32_t id);
extern int rs_msg_queue_mark_failed(uint32_t id);
extern int rs_msg_queue_save(void);
