// SPDX-License-Identifier: BSD-3-Clause
// FS.h stub — Arduino filesystem not used on ESP-IDF.
// SimpleMeshTables.h includes this under #ifdef ESP32.
#pragma once

#include <stdint.h>
#include <stddef.h>

// Minimal File stub for compilation only
class File {
public:
    size_t read(uint8_t* buf, size_t size) { (void)buf; (void)size; return 0; }
    size_t write(const uint8_t* buf, size_t size) { (void)buf; (void)size; return 0; }
    operator bool() const { return false; }
};

#define FILESYSTEM void
