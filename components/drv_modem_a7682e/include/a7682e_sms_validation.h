// SPDX-License-Identifier: BSD-3-Clause
// Input validation for A7682E SMS AT command construction.

#pragma once

#include <stdbool.h>

/* Phone numbers used in AT+CMGS may be international (+<digits>) or local
 * (<digits>).  Restricting the input to this grammar prevents AT syntax from
 * being introduced into the command's quoted recipient field. */
static inline bool a7682e_sms_phone_is_valid(const char *phone)
{
    if (!phone || !*phone) {
        return false;
    }

    if (*phone == '+') {
        phone++;
    }
    if (!*phone) {
        return false;
    }

    for (; *phone; phone++) {
        if (*phone < '0' || *phone > '9') {
            return false;
        }
    }
    return true;
}

/* The message is appended to an AT command after the CMGS prompt.  Reject
 * all C0 controls and DEL so no input byte can terminate or escape that flow
 * (in particular CR, LF, ESC, and Ctrl-Z). */
static inline bool a7682e_sms_message_is_valid(const char *message)
{
    if (!message) {
        return false;
    }

    for (; *message; message++) {
        unsigned char byte = (unsigned char)*message;
        if (byte < 0x20 || byte == 0x7f) {
            return false;
        }
    }
    return true;
}
