#include "thistle/ota.h"
#include "thistle/kernel.h"
#include "esp_ota_ops.h"
#include "esp_partition.h"
#include "esp_app_format.h"
#include "esp_log.h"
#include "esp_system.h"
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>

static const char *TAG = "ota";

#define OTA_BUF_SIZE       4096
#define OTA_SD_UPDATE_PATH "/sdcard/update/thistle_os.bin"

esp_err_t ota_init(void)
{
    /* Mark current partition as valid if boot was successful.
     * On first boot after OTA, ESP-IDF keeps the partition in
     * "pending verify" state — we confirm it here. */
    const esp_partition_t *running = esp_ota_get_running_partition();
    if (running) {
        ESP_LOGI(TAG, "Running from partition: %s (addr=0x%lx)",
                 running->label, (unsigned long)running->address);
    }

    esp_ota_img_states_t state;
    if (esp_ota_get_state_partition(running, &state) == ESP_OK) {
        if (state == ESP_OTA_IMG_PENDING_VERIFY) {
            ESP_LOGI(TAG, "Confirming OTA update (marking valid)");
            esp_ota_mark_app_valid_cancel_rollback();
        }
    }

    ESP_LOGI(TAG, "OTA subsystem initialized");
    return ESP_OK;
}

bool ota_sd_update_available(void)
{
    struct stat st;
    return (stat(OTA_SD_UPDATE_PATH, &st) == 0 && st.st_size > 0);
}

esp_err_t ota_apply_from_sd(ota_progress_cb_t progress_cb, void *user_data)
{
    FILE *f = fopen(OTA_SD_UPDATE_PATH, "rb");
    if (!f) {
        ESP_LOGE(TAG, "Cannot open update file: %s", OTA_SD_UPDATE_PATH);
        return ESP_ERR_NOT_FOUND;
    }

    /* Get file size */
    fseek(f, 0, SEEK_END);
    long file_size = ftell(f);
    fseek(f, 0, SEEK_SET);

    if (file_size <= 0) {
        ESP_LOGE(TAG, "Invalid update file size: %ld", file_size);
        fclose(f);
        return ESP_ERR_INVALID_SIZE;
    }

    ESP_LOGI(TAG, "Applying OTA update from SD (%ld bytes)", file_size);

    /* Find the next OTA partition */
    const esp_partition_t *update_partition = esp_ota_get_next_update_partition(NULL);
    if (!update_partition) {
        ESP_LOGE(TAG, "No OTA partition available");
        fclose(f);
        return ESP_ERR_NOT_FOUND;
    }

    ESP_LOGI(TAG, "Writing to partition: %s (0x%lx, %lu bytes)",
             update_partition->label,
             (unsigned long)update_partition->address,
             (unsigned long)update_partition->size);

    /* Begin OTA */
    esp_ota_handle_t ota_handle;
    esp_err_t ret = esp_ota_begin(update_partition, file_size, &ota_handle);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_ota_begin failed: %s", esp_err_to_name(ret));
        fclose(f);
        return ret;
    }

    /* Write in chunks */
    uint8_t *buf = malloc(OTA_BUF_SIZE);
    if (!buf) {
        esp_ota_abort(ota_handle);
        fclose(f);
        return ESP_ERR_NO_MEM;
    }

    uint32_t written = 0;
    while (written < (uint32_t)file_size) {
        size_t to_read = OTA_BUF_SIZE;
        if (written + to_read > (uint32_t)file_size) {
            to_read = (uint32_t)file_size - written;
        }

        size_t nread = fread(buf, 1, to_read, f);
        if (nread != to_read) {
            ESP_LOGE(TAG, "Short read at offset %lu", (unsigned long)written);
            free(buf);
            esp_ota_abort(ota_handle);
            fclose(f);
            return ESP_ERR_INVALID_SIZE;
        }

        ret = esp_ota_write(ota_handle, buf, nread);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "esp_ota_write failed at offset %lu: %s",
                     (unsigned long)written, esp_err_to_name(ret));
            free(buf);
            esp_ota_abort(ota_handle);
            fclose(f);
            return ret;
        }

        written += nread;

        if (progress_cb) {
            progress_cb(written, (uint32_t)file_size, user_data);
        }
    }

    free(buf);
    fclose(f);

    /* Finalize OTA */
    ret = esp_ota_end(ota_handle);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_ota_end failed: %s", esp_err_to_name(ret));
        return ret;
    }

    /* Set boot partition */
    ret = esp_ota_set_boot_partition(update_partition);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_ota_set_boot_partition failed: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "OTA update successful (%lu bytes). Rebooting...", (unsigned long)written);
    esp_restart();

    return ESP_OK;  /* unreachable */
}

esp_err_t ota_apply_from_http(const char *url, ota_progress_cb_t progress_cb, void *user_data)
{
    /* TODO: Implement HTTP OTA using esp_https_ota component */
    (void)url; (void)progress_cb; (void)user_data;
    ESP_LOGW(TAG, "HTTP OTA not yet implemented");
    return ESP_ERR_NOT_SUPPORTED;
}

const char *ota_get_current_version(void)
{
    return THISTLE_VERSION_STRING;
}

const char *ota_get_running_partition(void)
{
    const esp_partition_t *p = esp_ota_get_running_partition();
    return p ? p->label : "unknown";
}

esp_err_t ota_mark_valid(void)
{
    return esp_ota_mark_app_valid_cancel_rollback();
}

esp_err_t ota_rollback(void)
{
    esp_err_t ret = esp_ota_mark_app_invalid_rollback_and_reboot();
    return ret;  /* unreachable on success */
}
