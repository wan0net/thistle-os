#pragma once

#include "esp_err.h"

/*
 * MeshCore Shim Layer (Phase 5)
 *
 * Provides ThistleMeshBoard, ThistleMeshRadio, ThistleMeshDisplay
 * that route MeshCore's hardware calls through ThistleOS HAL.
 *
 * Build MeshCore with:
 *   -DBOARD_CLASS=ThistleMeshBoard
 *   -DRADIO_CLASS=ThistleMeshRadio
 *   -DDISPLAY_CLASS=ThistleMeshDisplay
 */

/* Initialize MeshCore shim (sets up HAL bridges) */
esp_err_t meshcore_shim_init(void);
