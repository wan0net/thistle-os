// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors

#include "a7682e_sms_validation.h"

#include <assert.h>
#include <string.h>

static void test_phone_grammar(void)
{
    assert(a7682e_sms_phone_is_valid("+61412345678"));
    assert(a7682e_sms_phone_is_valid("0412345678"));

    const char *invalid[] = {
        "",
        "+",
        "+61\";AT+CFUN=0",
        "123\rAT+CFUN=0",
        "123\nAT+CFUN=0",
        "123\x1b" "AT",
        "123\x1a" "AT",
        "12 34",
        "12-34",
    };
    for (size_t i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        assert(!a7682e_sms_phone_is_valid(invalid[i]));
    }

    char overlong[A7682E_SMS_PHONE_MAX_LENGTH + 2U];
    memset(overlong, '1', sizeof(overlong));
    overlong[sizeof(overlong) - 1U] = '\0';
    assert(!a7682e_sms_phone_is_valid(overlong));
}

static void test_message_controls(void)
{
    assert(a7682e_sms_message_is_valid("ordinary GSM text"));
    assert(a7682e_sms_message_is_valid(""));

    const char *invalid[] = {
        "quoted \"payload\"",
        "line\rAT+CFUN=0",
        "line\nAT+CFUN=0",
        "escape\x1b" "AT",
        "terminate\x1a" "AT",
        "delete\x7f" "AT",
    };
    for (size_t i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        assert(!a7682e_sms_message_is_valid(invalid[i]));
    }

    char maximum[A7682E_SMS_MESSAGE_MAX_LENGTH + 1U];
    memset(maximum, 'A', A7682E_SMS_MESSAGE_MAX_LENGTH);
    maximum[A7682E_SMS_MESSAGE_MAX_LENGTH] = '\0';
    assert(a7682e_sms_message_is_valid(maximum));

    char overlong[A7682E_SMS_MESSAGE_MAX_LENGTH + 2U];
    memset(overlong, 'A', sizeof(overlong));
    overlong[sizeof(overlong) - 1U] = '\0';
    assert(!a7682e_sms_message_is_valid(overlong));
}

int main(void)
{
    test_phone_grammar();
    test_message_controls();
    return 0;
}
