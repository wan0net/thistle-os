#pragma once

#include "esp_err.h"
#include <stdint.h>

/* Toast severity levels — affects styling */
typedef enum {
    TOAST_INFO,       /* Black text, white bg, thin border */
    TOAST_SUCCESS,    /* Same style with checkmark prefix */
    TOAST_WARNING,    /* Inverted: white text, black bg */
    TOAST_ERROR,      /* Same as warning with X prefix */
} toast_level_t;

/* Show a toast notification. Auto-dismisses after duration_ms.
 * If another toast is showing, it's replaced. */
esp_err_t toast_show(const char *message, toast_level_t level, uint32_t duration_ms);

/* Show a toast with default INFO level and 3 second duration */
esp_err_t toast_info(const char *message);

/* Show a toast with WARNING level and 4 second duration */
esp_err_t toast_warn(const char *message);

/* Dismiss current toast immediately (if any) */
void toast_dismiss(void);
