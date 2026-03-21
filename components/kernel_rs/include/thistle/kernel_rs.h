// SPDX-License-Identifier: BSD-3-Clause
// C header for Rust kernel FFI functions.
//
// These functions are implemented in Rust (components/kernel_rs/src/ffi.rs)
// and exported as C-compatible symbols. They can be called directly from C
// as drop-in replacements for the corresponding C implementations.

#pragma once

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#include "thistle/manifest.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Parse a manifest.json file (Rust implementation).
 * Same semantics as manifest_parse_file() in manifest.c. */
int rs_manifest_parse_file(const char *json_path, thistle_manifest_t *out);

/* Check manifest compatibility (Rust implementation).
 * current_arch: e.g., "esp32s3" */
bool rs_manifest_is_compatible(const thistle_manifest_t *manifest, const char *current_arch);

/* Derive manifest path from ELF path (Rust implementation). */
void rs_manifest_path_from_elf(const char *elf_path, char *out_path, size_t out_size);

/* Get kernel version string from Rust kernel. */
const char *rs_kernel_version(void);

#ifdef __cplusplus
}
#endif
