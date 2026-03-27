// SPDX-License-Identifier: BSD-3-Clause
// base64.hpp stub — only needed if MAX_GROUP_CHANNELS is defined.
#pragma once

#include <stdint.h>
#include <stddef.h>

static inline int decode_base64(const unsigned char* src, size_t src_len, unsigned char* dst) {
    (void)src; (void)src_len; (void)dst;
    return 0;
}

static inline int encode_base64(const unsigned char* src, size_t src_len, unsigned char* dst) {
    (void)src; (void)src_len; (void)dst;
    return 0;
}
