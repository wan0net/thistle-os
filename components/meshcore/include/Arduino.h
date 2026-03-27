// SPDX-License-Identifier: BSD-3-Clause
// Arduino.h stub for ESP-IDF builds of MeshCore.
// MeshCore includes <Arduino.h> for millis(), random(), etc.
// We provide minimal stubs backed by ESP-IDF APIs.
#pragma once

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <esp_timer.h>
#include <esp_random.h>
#include <freertos/FreeRTOS.h>
#include <freertos/task.h>

static inline unsigned long millis(void) {
    return (unsigned long)(esp_timer_get_time() / 1000ULL);
}

static inline void delay(unsigned long ms) {
    vTaskDelay(ms / portTICK_PERIOD_MS);
}

// Arduino random() — can't be named "random" in C++ as it conflicts
// with libc. MeshCore's RNG class uses its own virtual nextInt(), so
// these are only needed for stray Arduino-isms.
static inline long arduino_random(long max) {
    if (max <= 0) return 0;
    return (long)(esp_random() % (uint32_t)max);
}
#define random(x) arduino_random(x)

// Serial stub — MeshCore uses Serial.print for debug logging
class FakeSerial {
public:
    void print(const char*) {}
    void print(int) {}
    void print(unsigned int) {}
    void print(long) {}
    void print(unsigned long) {}
    void print(float, int = 2) {}
    void println(const char* s = "") { (void)s; }
    void println(int) {}
    void println(unsigned int) {}
    void printf(const char*, ...) {}
    int available() { return 0; }
    int read() { return -1; }
};

extern FakeSerial Serial;

// Arduino pin mode / digital IO stubs
#define INPUT  0
#define OUTPUT 1
#define HIGH   1
#define LOW    0
static inline void pinMode(int, int) {}
static inline void digitalWrite(int, int) {}
static inline int digitalRead(int) { return 0; }
static inline int analogRead(int) { return 0; }
