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

// ltoa stub (used by TxtDataHelpers)
static inline char* ltoa(long val, char* buf, int radix) {
    if (radix == 10) { snprintf(buf, 12, "%ld", val); }
    else if (radix == 16) { snprintf(buf, 12, "%lx", val); }
    else { buf[0] = '\0'; }
    return buf;
}

#include <stdio.h>

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
