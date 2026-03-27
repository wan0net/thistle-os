// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — mbedtls hardware crypto driver
#pragma once

#include "hal/crypto.h"

/// Get the mbedtls hardware crypto driver vtable.
const hal_crypto_driver_t *drv_crypto_mbedtls_get(void);
