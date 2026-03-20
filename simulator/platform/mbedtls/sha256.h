#pragma once

/*
 * Simulator shim for mbedtls/sha256.h — uses macOS CommonCrypto.
 * Provides the exact mbedtls_sha256_* API that appstore_client.c calls.
 * SPDX-License-Identifier: BSD-3-Clause
 */

#include <CommonCrypto/CommonDigest.h>
#include <stdint.h>
#include <stddef.h>

typedef struct {
    CC_SHA256_CTX cc_ctx;
    int           is224;   /* 0 = SHA-256, 1 = SHA-224 (unused in ThistleOS) */
} mbedtls_sha256_context;

static inline void mbedtls_sha256_init(mbedtls_sha256_context *ctx) {
    __builtin_memset(ctx, 0, sizeof(*ctx));
}

static inline void mbedtls_sha256_free(mbedtls_sha256_context *ctx) {
    (void)ctx;
}

/* is224: 0 = SHA-256, 1 = SHA-224 */
static inline int mbedtls_sha256_starts(mbedtls_sha256_context *ctx, int is224) {
    ctx->is224 = is224;
    CC_SHA256_Init(&ctx->cc_ctx);
    return 0;
}

static inline int mbedtls_sha256_update(mbedtls_sha256_context *ctx,
                                         const uint8_t *input, size_t ilen) {
    CC_SHA256_Update(&ctx->cc_ctx, input, (CC_LONG)ilen);
    return 0;
}

static inline int mbedtls_sha256_finish(mbedtls_sha256_context *ctx,
                                         uint8_t output[32]) {
    CC_SHA256_Final(output, &ctx->cc_ctx);
    return 0;
}
