#pragma once

#include "esp_err.h"
#include <stdint.h>
#include <stdbool.h>

typedef enum {
    APP_STATE_UNLOADED,
    APP_STATE_LOADING,
    APP_STATE_RUNNING,
    APP_STATE_BACKGROUNDED,
    APP_STATE_SUSPENDED,
} app_state_t;

typedef struct {
    const char *id;          // e.g., "com.thistle.launcher"
    const char *name;        // Display name
    const char *version;     // Semver string
    bool allow_background;   // Can run in background?
    uint32_t min_memory_kb;  // Minimum PSRAM needed
} app_manifest_t;

/* App callbacks — implemented by each app (built-in or loaded) */
typedef struct {
    esp_err_t (*on_create)(void);        // Initialize app
    void (*on_start)(void);              // App becomes foreground
    void (*on_pause)(void);              // App goes to background/suspended
    void (*on_resume)(void);             // App returns to foreground
    void (*on_destroy)(void);            // Cleanup before unload
    const app_manifest_t *manifest;
} app_entry_t;

typedef int app_handle_t;
#define APP_HANDLE_INVALID (-1)

/* Register a built-in app */
esp_err_t app_manager_register(const app_entry_t *app);

/* Launch an app by ID (brings to foreground) */
esp_err_t app_manager_launch(const char *app_id);

/* Switch to another running app */
esp_err_t app_manager_switch_to(app_handle_t handle);

/* Get foreground app handle */
app_handle_t app_manager_get_foreground(void);

/* Get app state */
app_state_t app_manager_get_state(app_handle_t handle);

/* Suspend the given app */
esp_err_t app_manager_suspend(app_handle_t handle);

/* Kill/unload an app */
esp_err_t app_manager_kill(app_handle_t handle);

/* Initialize app manager subsystem */
esp_err_t app_manager_init(void);
