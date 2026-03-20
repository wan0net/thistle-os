#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stddef.h>

/* Syscall IDs — used by apps to call kernel functions */
typedef enum {
    /* System */
    SYSCALL_LOG = 0,
    SYSCALL_MILLIS,
    SYSCALL_DELAY,
    SYSCALL_MALLOC,
    SYSCALL_FREE,
    SYSCALL_REALLOC,

    /* Display (LVGL wrappers) */
    SYSCALL_DISPLAY_GET_WIDTH = 100,
    SYSCALL_DISPLAY_GET_HEIGHT,

    /* Input */
    SYSCALL_INPUT_REGISTER_CB = 200,

    /* Radio */
    SYSCALL_RADIO_SEND = 300,
    SYSCALL_RADIO_START_RX,
    SYSCALL_RADIO_SET_FREQ,

    /* GPS */
    SYSCALL_GPS_GET_POSITION = 400,
    SYSCALL_GPS_ENABLE,

    /* Storage */
    SYSCALL_FS_OPEN = 500,
    SYSCALL_FS_READ,
    SYSCALL_FS_WRITE,
    SYSCALL_FS_CLOSE,

    /* IPC */
    SYSCALL_MSG_SEND = 600,
    SYSCALL_MSG_RECV,
    SYSCALL_EVENT_SUBSCRIBE,
    SYSCALL_EVENT_PUBLISH,

    /* Power */
    SYSCALL_POWER_GET_BATTERY_MV = 700,
    SYSCALL_POWER_GET_BATTERY_PCT,

    SYSCALL_MAX
} syscall_id_t;

/* A single entry in the syscall table */
typedef struct {
    const char *name;     // Symbol name for ELF resolver
    void *func_ptr;       // Pointer to implementation
} syscall_entry_t;

/* Initialize syscall table */
esp_err_t syscall_table_init(void);

/* Get the syscall table (for ELF loader symbol resolution) */
const syscall_entry_t *syscall_table_get(void);

/* Get number of entries in the table */
size_t syscall_table_count(void);

/* Look up a symbol by name — returns function pointer or NULL */
void *syscall_resolve(const char *name);
