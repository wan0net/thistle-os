// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS HAL — RTC (Real-Time Clock) interface
#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

/**
 * hal_datetime_t — calendar time, matching HalDateTime in hal_registry.rs.
 *
 * Field layout is identical in C and Rust (both #[repr(C)] / packed structs).
 */
typedef struct {
    uint16_t year;    /**< Full year, e.g. 2026 */
    uint8_t  month;   /**< 1–12 */
    uint8_t  day;     /**< 1–31 */
    uint8_t  weekday; /**< 0=Sunday … 6=Saturday */
    uint8_t  hour;    /**< 0–23 */
    uint8_t  minute;  /**< 0–59 */
    uint8_t  second;  /**< 0–59 */
} hal_datetime_t;

/**
 * hal_rtc_driver_t — vtable for a real-time clock driver.
 *
 * The driver is responsible for BCD/binary conversion internally;
 * all values exposed through this interface are plain binary integers.
 */
typedef struct {
    /**
     * Initialise the RTC hardware.
     * @param config  Driver-specific configuration struct (cast from void*).
     * @return ESP_OK on success, ESP error code on failure.
     */
    esp_err_t (*init)(const void *config);

    /** Release resources acquired during init. */
    void (*deinit)(void);

    /**
     * Read the current time from the RTC.
     * @param dt  Output — filled with the current date/time.
     * @return ESP_OK, or an error code if the read failed.
     */
    esp_err_t (*get_time)(hal_datetime_t *dt);

    /**
     * Set the RTC to the given date/time.
     * @param dt  Date/time to program into the chip.
     * @return ESP_OK on success.
     */
    esp_err_t (*set_time)(const hal_datetime_t *dt);

    /**
     * Check whether the stored time is trustworthy.
     *
     * Returns false when the chip has detected a power-loss event (e.g. the
     * PCF8563 VL bit) — callers should treat the time as unreliable and
     * synchronise from an external source before using it.
     */
    bool (*is_valid)(void);

    /** Human-readable driver name, e.g. "PCF8563". */
    const char *name;
} hal_rtc_driver_t;
