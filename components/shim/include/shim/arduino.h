#pragma once

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include "esp_err.h"

/* ------------------------------------------------------------------ */
/* Timing                                                               */
/* ------------------------------------------------------------------ */
uint32_t millis(void);
uint32_t micros(void);
void delay(uint32_t ms);
void delayMicroseconds(uint32_t us);

/* ------------------------------------------------------------------ */
/* GPIO                                                                 */
/* ------------------------------------------------------------------ */
#define INPUT        0
#define OUTPUT       1
#define INPUT_PULLUP 2
#define HIGH         1
#define LOW          0

void pinMode(uint8_t pin, uint8_t mode);
void digitalWrite(uint8_t pin, uint8_t val);
int digitalRead(uint8_t pin);
int analogRead(uint8_t pin);

/* ------------------------------------------------------------------ */
/* Interrupt support                                                    */
/* ------------------------------------------------------------------ */
#define RISING  1
#define FALLING 2
#define CHANGE  3

void attachInterrupt(uint8_t pin, void (*handler)(void), int mode);
void detachInterrupt(uint8_t pin);

/* ------------------------------------------------------------------ */
/* Serial (maps to ESP_LOG / UART0)                                    */
/* ------------------------------------------------------------------ */
typedef struct {
    void   (*begin)(unsigned long baud);
    size_t (*print)(const char *str);
    size_t (*println)(const char *str);
    size_t (*write)(uint8_t byte);
    size_t (*writeBytes)(const uint8_t *buf, size_t len);
    void   (*flush)(void);
    int    (*available)(void);
    int    (*read)(void);
    int    (*peek)(void);
} arduino_serial_t;

extern arduino_serial_t Serial;

/* ------------------------------------------------------------------ */
/* SPI                                                                  */
/* ------------------------------------------------------------------ */
#define MSBFIRST  1
#define LSBFIRST  0
#define SPI_MODE0 0
#define SPI_MODE1 1
#define SPI_MODE2 2
#define SPI_MODE3 3

typedef struct {
    void    (*begin)(void);
    void    (*end)(void);
    uint8_t (*transfer)(uint8_t data);
    void    (*transferBytes)(const uint8_t *out, uint8_t *in, uint32_t size);
    void    (*beginTransaction)(uint32_t clock, uint8_t bitOrder, uint8_t dataMode);
    void    (*endTransaction)(void);
} arduino_spi_t;

extern arduino_spi_t SPI;

/* ------------------------------------------------------------------ */
/* Wire (I2C)                                                           */
/* ------------------------------------------------------------------ */
typedef struct {
    void    (*begin)(void);
    void    (*beginTransmission)(uint8_t addr);
    uint8_t (*endTransmission)(void);   /* 0 = success */
    size_t  (*write)(uint8_t data);
    size_t  (*writeBytes)(const uint8_t *data, size_t len);
    uint8_t (*requestFrom)(uint8_t addr, uint8_t quantity);
    int     (*read)(void);
    int     (*available)(void);
} arduino_wire_t;

extern arduino_wire_t Wire;

/* ------------------------------------------------------------------ */
/* Math / utility macros                                                */
/* ------------------------------------------------------------------ */
#define constrain(amt, low, high) \
    ((amt) < (low) ? (low) : ((amt) > (high) ? (high) : (amt)))

#define _arduino_min(a, b) ((a) < (b) ? (a) : (b))
#define _arduino_max(a, b) ((a) > (b) ? (a) : (b))
#ifndef min
#define min(a, b) _arduino_min(a, b)
#endif
#ifndef max
#define max(a, b) _arduino_max(a, b)
#endif

/* Bit manipulation */
#define bitRead(value, bit)              (((value) >> (bit)) & 0x01)
#define bitSet(value, bit)               ((value) |= (1UL << (bit)))
#define bitClear(value, bit)             ((value) &= ~(1UL << (bit)))
#define bitWrite(value, bit, bitvalue)   ((bitvalue) ? bitSet(value, bit) : bitClear(value, bit))
#define bit(b)                           (1UL << (b))

/* Byte manipulation */
#define highByte(w) ((uint8_t)((w) >> 8))
#define lowByte(w)  ((uint8_t)((w) & 0xff))

/* ------------------------------------------------------------------ */
/* Math functions                                                       */
/* ------------------------------------------------------------------ */
long map(long x, long in_min, long in_max, long out_min, long out_max);

/* ------------------------------------------------------------------ */
/* Random                                                               */
/* ------------------------------------------------------------------ */
long arduino_random(long maxval);
long arduino_random_range(long minval, long maxval);
#define random(x) arduino_random(x)
void randomSeed(unsigned long seed);

/* ------------------------------------------------------------------ */
/* Lifecycle                                                            */
/* ------------------------------------------------------------------ */
esp_err_t arduino_shim_init(void);

typedef void (*arduino_setup_fn)(void);
typedef void (*arduino_loop_fn)(void);
esp_err_t arduino_shim_run(arduino_setup_fn setup, arduino_loop_fn loop);
