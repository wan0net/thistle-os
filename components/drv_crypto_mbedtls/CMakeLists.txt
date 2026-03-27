# SPDX-License-Identifier: BSD-3-Clause
# ThistleOS — mbedtls hardware crypto driver
#
# Wraps ESP-IDF's mbedtls (with Espressif hardware acceleration patches)
# as a HAL crypto driver. Provides SHA-256, AES-256-CBC, HMAC-SHA256,
# and RNG through the standard hal_crypto_driver_t vtable.

idf_component_register(
    SRCS "src/drv_crypto_mbedtls.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal mbedtls esp_hw_support
)
