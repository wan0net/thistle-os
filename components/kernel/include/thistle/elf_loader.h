// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) ThistleOS contributors
#pragma once

#include "esp_err.h"
#include "thistle/app_manager.h"
#include <stdint.h>
#include <stddef.h>

/* Opaque handle for a loaded ELF app */
typedef struct elf_app_handle *elf_app_handle_t;

/* Load an ELF app from the filesystem path (e.g., "/sdcard/apps/hello.app.elf")
 * Returns ESP_OK on success and populates *handle.
 * The ELF is loaded into PSRAM, symbols resolved against the syscall table. */
esp_err_t elf_app_load(const char *path, elf_app_handle_t *handle);

/* Start executing a loaded ELF app.
 * Calls the app's entry point in a new FreeRTOS task. */
esp_err_t elf_app_start(elf_app_handle_t handle);

/* Stop and unload an ELF app.
 * Deletes the FreeRTOS task, frees PSRAM. */
esp_err_t elf_app_unload(elf_app_handle_t handle);

/* Get the app manifest from a loaded ELF (NULL if not found) */
const app_manifest_t *elf_app_get_manifest(elf_app_handle_t handle);

/* Initialize the ELF loader subsystem */
esp_err_t elf_loader_init(void);
