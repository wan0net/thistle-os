/* SPDX-License-Identifier: BSD-3-Clause */

#include "unity.h"
#include "a7682e_sms_validation.h"

TEST_CASE("A7682E SMS phone validation accepts digit-only recipients", "[a7682e][sms]")
{
    TEST_ASSERT_TRUE(a7682e_sms_phone_is_valid("61412345678"));
    TEST_ASSERT_TRUE(a7682e_sms_phone_is_valid("+61412345678"));
}

TEST_CASE("A7682E SMS phone validation rejects AT-command syntax", "[a7682e][sms]")
{
    TEST_ASSERT_FALSE(a7682e_sms_phone_is_valid(""));
    TEST_ASSERT_FALSE(a7682e_sms_phone_is_valid("+"));
    TEST_ASSERT_FALSE(a7682e_sms_phone_is_valid("+61\rAT"));
    TEST_ASSERT_FALSE(a7682e_sms_phone_is_valid("+61\";AT"));
    TEST_ASSERT_FALSE(a7682e_sms_phone_is_valid("+61 412345678"));
}

TEST_CASE("A7682E SMS message validation preserves printable SMS text", "[a7682e][sms]")
{
    TEST_ASSERT_TRUE(a7682e_sms_message_is_valid("Meet me at 6pm!"));
    TEST_ASSERT_TRUE(a7682e_sms_message_is_valid("Price: $12.50"));
}

TEST_CASE("A7682E SMS message validation rejects command control bytes", "[a7682e][sms]")
{
    TEST_ASSERT_FALSE(a7682e_sms_message_is_valid("hello\rAT"));
    TEST_ASSERT_FALSE(a7682e_sms_message_is_valid("hello\nAT"));
    TEST_ASSERT_FALSE(a7682e_sms_message_is_valid("hello\x1b" "AT"));
    TEST_ASSERT_FALSE(a7682e_sms_message_is_valid("hello\x1a" "AT"));
    TEST_ASSERT_FALSE(a7682e_sms_message_is_valid("hello\tthere"));
}
