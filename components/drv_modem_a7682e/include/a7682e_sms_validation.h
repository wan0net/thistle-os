// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#pragma once

#include <stdbool.h>
#include <stddef.h>

#define A7682E_SMS_PHONE_MAX_LENGTH 31U
#define A7682E_SMS_MESSAGE_MAX_LENGTH 160U

/* Phone numbers used in AT+CMGS may be international (+<digits>) or local
 * (<digits>). Restricting the input to this grammar prevents AT syntax from
 * entering the command's quoted recipient field. */
static inline bool a7682e_sms_phone_is_valid(const char *phone)
{
    if (!phone) {
        return false;
    }

    size_t length = 0;
    while (length <= A7682E_SMS_PHONE_MAX_LENGTH && phone[length] != '\0') {
        length++;
    }
    if (length == 0 || length > A7682E_SMS_PHONE_MAX_LENGTH) {
        return false;
    }

    size_t index = phone[0] == '+' ? 1U : 0U;
    if (index == length) {
        return false;
    }
    for (; index < length; index++) {
        if (phone[index] < '0' || phone[index] > '9') {
            return false;
        }
    }
    return true;
}

/* Text is sent only after esp_modem has observed the CMGS prompt. Reject C0
 * controls, DEL, and quotes before invoking that API so hostile input cannot
 * terminate, escape, or masquerade as command framing. */
static inline bool a7682e_sms_message_is_valid(const char *message)
{
    if (!message) {
        return false;
    }

    size_t length = 0;
    while (length <= A7682E_SMS_MESSAGE_MAX_LENGTH && message[length] != '\0') {
        unsigned char byte = (unsigned char)message[length];
        if (byte < 0x20 || byte == 0x7f || byte == '"') {
            return false;
        }
        length++;
    }
    return length <= A7682E_SMS_MESSAGE_MAX_LENGTH;
}
