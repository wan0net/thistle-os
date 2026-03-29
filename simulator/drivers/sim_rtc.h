// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS Simulator — fake RTC HAL driver (host clock)
#pragma once

#include "hal/rtc.h"

const hal_rtc_driver_t *sim_rtc_get(void);
