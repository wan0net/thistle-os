// SPDX-License-Identifier: BSD-3-Clause
// Stream.h stub for ESP-IDF builds of MeshCore.
// Minimal Arduino Stream abstraction — enough for MeshCore's
// readFrom/writeTo/printTo serialization.
#pragma once

#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdio.h>

class Stream {
public:
    virtual ~Stream() {}

    // Read interface
    virtual int available() { return 0; }
    virtual int read() { return -1; }
    virtual size_t readBytes(uint8_t* buffer, size_t length) {
        size_t count = 0;
        while (count < length) {
            int c = read();
            if (c < 0) break;
            buffer[count++] = (uint8_t)c;
        }
        return count;
    }

    // Write interface
    virtual size_t write(uint8_t b) { (void)b; return 0; }
    virtual size_t write(const uint8_t* buffer, size_t size) {
        size_t n = 0;
        while (n < size) {
            if (write(buffer[n]) == 0) break;
            n++;
        }
        return n;
    }

    // Print helpers
    void print(const char* s) {
        if (s) write((const uint8_t*)s, strlen(s));
    }
    void print(int n) {
        char buf[12];
        snprintf(buf, sizeof(buf), "%d", n);
        print(buf);
    }
    void println(const char* s = "") {
        print(s);
        write((uint8_t)'\n');
    }
    void println(int n) {
        print(n);
        write((uint8_t)'\n');
    }
};
