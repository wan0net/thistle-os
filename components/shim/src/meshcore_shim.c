#include "shim/meshcore.h"
#include "esp_log.h"

static const char *TAG = "meshcore_shim";

esp_err_t meshcore_shim_init(void)
{
    ESP_LOGI(TAG, "MeshCore shim initialized (Phase 5 -- not yet implemented)");
    return ESP_OK;
}
