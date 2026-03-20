#pragma once

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include "esp_err.h"

/* Arduino timing */
uint32_t millis(void);
uint32_t micros(void);
void delay(uint32_t ms);
void delayMicroseconds(uint32_t us);

/* Arduino GPIO */
#define INPUT        0
#define OUTPUT       1
#define INPUT_PULLUP 2
#define HIGH         1
#define LOW          0

void pinMode(uint8_t pin, uint8_t mode);
void digitalWrite(uint8_t pin, uint8_t val);
int digitalRead(uint8_t pin);
int analogRead(uint8_t pin);

/* Arduino Serial (maps to ESP_LOG) */
typedef struct {
    void   (*begin)(unsigned long baud);
    size_t (*print)(const char *str);
    size_t (*println)(const char *str);
    int    (*available)(void);
    int    (*read)(void);
} arduino_serial_t;

extern arduino_serial_t Serial;

/* Initialize the Arduino shim layer */
esp_err_t arduino_shim_init(void);

/* Run an Arduino-style app (calls setup() once, then loop() repeatedly) */
typedef void (*arduino_setup_fn)(void);
typedef void (*arduino_loop_fn)(void);
esp_err_t arduino_shim_run(arduino_setup_fn setup, arduino_loop_fn loop);
