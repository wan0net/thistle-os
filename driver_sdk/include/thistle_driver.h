// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors
#pragma once

/*
 * ThistleOS Driver SDK
 *
 * Runtime-loadable drivers include this header and implement a
 * driver_init() function that registers vtables with the HAL.
 *
 * Entry point: int driver_init(const char *config_json)
 *   - config_json: JSON string with driver-specific config from board.json
 *   - Return 0 on success, non-zero on failure
 *
 * Drivers can be written in C or Rust. The ELF loader resolves symbols
 * from the kernel's syscall table at load time.
 *
 * C example:
 *   #include "thistle_driver.h"
 *   int driver_init(const char *config_json) {
 *       void *i2c = hal_bus_get_i2c(0);
 *       // ... init hardware ...
 *       hal_input_register(&my_vtable, NULL);
 *       return 0;
 *   }
 *
 * Rust example:
 *   #[no_mangle]
 *   pub extern "C" fn driver_init(config: *const c_char) -> i32 {
 *       let i2c = unsafe { hal_bus_get_i2c(0) };
 *       // ... init hardware ...
 *       unsafe { hal_input_register(&MY_VTABLE as *const _, std::ptr::null()) };
 *       0
 *   }
 */

/* ── HAL registration (resolved from kernel syscall table) ──────── */
extern int hal_display_register(const void *driver, const void *config);
extern int hal_input_register(const void *driver, const void *config);
extern int hal_radio_register(const void *driver, const void *config);
extern int hal_gps_register(const void *driver, const void *config);
extern int hal_audio_register(const void *driver, const void *config);
extern int hal_power_register(const void *driver, const void *config);
extern int hal_imu_register(const void *driver, const void *config);
extern int hal_storage_register(const void *driver, const void *config);

/* ── Bus handle access (initialized by kernel from board.json) ──── */
extern void *hal_bus_get_spi(int index);
extern void *hal_bus_get_i2c(int index);
extern const void *hal_get_registry(void);

/* ── Kernel utilities ───────────────────────────────────────────── */
extern void thistle_log(const char *tag, const char *fmt, ...);
extern unsigned int thistle_millis(void);
extern void thistle_delay(unsigned int ms);
extern void *thistle_malloc(unsigned int size);
extern void thistle_free(void *ptr);
extern void *thistle_realloc(void *ptr, unsigned int size);

/* ── ESP-IDF peripherals (resolved from kernel syscall table) ───── */
/* GPIO */
extern int gpio_config(const void *config);
extern int gpio_set_level(int gpio_num, unsigned int level);
extern int gpio_get_level(int gpio_num);
extern int gpio_set_direction(int gpio_num, int mode);
extern int gpio_isr_handler_add(int gpio_num, void (*handler)(void*), void *arg);
extern int gpio_isr_handler_remove(int gpio_num);

/* SPI */
extern int spi_bus_add_device(int host, const void *dev_config, void **handle);
extern int spi_device_polling_transmit(void *handle, void *trans);
extern int spi_device_transmit(void *handle, void *trans);

/* I2C */
extern int i2c_master_bus_add_device(void *bus, const void *dev_config, void **handle);
extern int i2c_master_transmit(void *dev, const void *data, unsigned int len, int timeout);
extern int i2c_master_receive(void *dev, void *data, unsigned int len, int timeout);
extern int i2c_master_transmit_receive(void *dev, const void *tx, unsigned int tx_len, void *rx, unsigned int rx_len, int timeout);

/* UART */
extern int uart_driver_install(int uart_num, int rx_buf, int tx_buf, int queue_size, void *queue, int flags);
extern int uart_param_config(int uart_num, const void *config);
extern int uart_set_pin(int uart_num, int tx, int rx, int rts, int cts);
extern int uart_read_bytes(int uart_num, void *buf, unsigned int length, unsigned int timeout);
extern int uart_write_bytes(int uart_num, const void *src, unsigned int size);

/* Timers */
extern int esp_timer_create(const void *args, void **handle);
extern int esp_timer_start_periodic(void *timer, unsigned long long period_us);
extern int esp_timer_start_once(void *timer, unsigned long long timeout_us);
extern int esp_timer_stop(void *timer);
extern int esp_timer_delete(void *timer);

/* FreeRTOS */
extern void vTaskDelay(unsigned int ticks);
extern int xTaskCreate(void (*fn)(void*), const char *name, unsigned int stack, void *param, unsigned int prio, void **handle);
extern void vTaskDelete(void *handle);
