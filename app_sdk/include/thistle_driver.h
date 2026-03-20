// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors
#pragma once

/*
 * ThistleOS Driver SDK
 *
 * Runtime-loadable drivers include this header and implement a
 * driver_init() function that registers vtables with the HAL.
 *
 * Build with the ThistleOS toolchain, targeting the same Xtensa LX7
 * ABI as the kernel. The output is a position-independent ELF with
 * the .drv.elf extension — place it in /sdcard/drivers/ on the SD card.
 *
 * Example:
 *   #include "thistle_driver.h"
 *
 *   static const hal_display_driver_t my_display = { ... };
 *
 *   int driver_init(void) {
 *       hal_display_register(&my_display, NULL);
 *       return 0;
 *   }
 *
 * The driver_init() function is the ELF entry point.  It is called
 * synchronously during boot (after built-in drivers) from the kernel.
 * Return 0 on success, non-zero on failure.
 */

/* HAL registration functions — resolved at load time from kernel syscall table */
extern int hal_display_register(const void *driver, const void *config);
extern int hal_input_register(const void *driver, const void *config);
extern int hal_radio_register(const void *driver, const void *config);
extern int hal_gps_register(const void *driver, const void *config);
extern int hal_audio_register(const void *driver, const void *config);
extern int hal_power_register(const void *driver, const void *config);
extern int hal_imu_register(const void *driver, const void *config);
extern int hal_storage_register(const void *driver, const void *config);

/* Kernel utility functions available to drivers */
extern void thistle_log(const char *tag, const char *fmt, ...);
extern unsigned int thistle_millis(void);
extern void thistle_delay(unsigned int ms);
extern void *thistle_malloc(unsigned int size);
extern void thistle_free(void *ptr);
