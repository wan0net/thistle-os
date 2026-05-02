// SPDX-License-Identifier: BSD-3-Clause
// ThistleOS — PCM5102A I2S audio DAC driver
#include "drv_audio_pcm5102a.h"
#include "driver/i2s_std.h"
#include "esp_log.h"
#include "esp_err.h"
#include "freertos/FreeRTOS.h"
#include <string.h>
#include <stdlib.h>

static const char *TAG = "pcm5102a";

static struct {
    audio_pcm5102a_config_t cfg;
    i2s_chan_handle_t        tx_handle;
    bool                     initialized;
    bool                     playing;
    uint8_t                  volume;  // 0–100, applied in software
} s_audio;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/*
 * Scale a buffer of 16-bit PCM samples (interleaved stereo or mono) by the
 * current volume.  The scaling is done in-place on a temporary heap copy so
 * the caller's const buffer is never modified.  Returns the allocated buffer
 * (caller must free) or NULL on allocation failure.
 */
static int16_t *apply_volume(const uint8_t *data, size_t len)
{
    if (len == 0) {
        return NULL;
    }

    int16_t *buf = malloc(len);
    if (!buf) {
        ESP_LOGE(TAG, "apply_volume: malloc failed (%u bytes)", (unsigned)len);
        return NULL;
    }

    memcpy(buf, data, len);

    if (s_audio.volume == 100) {
        return buf;  // nothing to scale
    }

    size_t sample_count = len / sizeof(int16_t);
    for (size_t i = 0; i < sample_count; i++) {
        buf[i] = (int16_t)((int32_t)buf[i] * s_audio.volume / 100);
    }

    return buf;
}

// ---------------------------------------------------------------------------
// vtable implementations
// ---------------------------------------------------------------------------

static esp_err_t pcm5102a_init(const void *config)
{
    if (!config) {
        return ESP_ERR_INVALID_ARG;
    }

    if (s_audio.initialized) {
        ESP_LOGW(TAG, "already initialized — call deinit first");
        return ESP_ERR_INVALID_STATE;
    }

    memcpy(&s_audio.cfg, config, sizeof(audio_pcm5102a_config_t));
    s_audio.volume = 100;
    s_audio.playing = false;

    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(
        s_audio.cfg.i2s_num,
        I2S_ROLE_MASTER
    );
    esp_err_t ret = i2s_new_channel(&chan_cfg, &s_audio.tx_handle, NULL);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "i2s_new_channel failed: %s", esp_err_to_name(ret));
        return ret;
    }

    i2s_std_config_t std_cfg = {
        .clk_cfg  = I2S_STD_CLK_DEFAULT_CONFIG(44100),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(
                        I2S_DATA_BIT_WIDTH_16BIT, I2S_SLOT_MODE_STEREO),
        .gpio_cfg = {
            .mclk = I2S_GPIO_UNUSED,          // PCM5102A generates MCLK from BCK
            .bclk = s_audio.cfg.pin_bck,
            .ws   = s_audio.cfg.pin_ws,
            .dout = s_audio.cfg.pin_data,
            .din  = I2S_GPIO_UNUSED,
        },
    };

    ret = i2s_channel_init_std_mode(s_audio.tx_handle, &std_cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_init_std_mode failed: %s", esp_err_to_name(ret));
        i2s_del_channel(s_audio.tx_handle);
        s_audio.tx_handle = NULL;
        return ret;
    }

    s_audio.initialized = true;
    ESP_LOGI(TAG, "initialized on I2S%d (BCK=%d WS=%d DATA=%d)",
             s_audio.cfg.i2s_num,
             s_audio.cfg.pin_bck,
             s_audio.cfg.pin_ws,
             s_audio.cfg.pin_data);
    return ESP_OK;
}

static void pcm5102a_deinit(void)
{
    if (!s_audio.initialized) {
        return;
    }

    if (s_audio.playing) {
        i2s_channel_disable(s_audio.tx_handle);
        s_audio.playing = false;
    }

    i2s_del_channel(s_audio.tx_handle);
    s_audio.tx_handle  = NULL;
    s_audio.initialized = false;
    ESP_LOGI(TAG, "deinitialized");
}

static esp_err_t pcm5102a_play(const uint8_t *data, size_t len)
{
    if (!s_audio.initialized) {
        return ESP_ERR_INVALID_STATE;
    }
    if (!data || len == 0) {
        return ESP_ERR_INVALID_ARG;
    }

    if (!s_audio.playing) {
        esp_err_t ret = i2s_channel_enable(s_audio.tx_handle);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "i2s_channel_enable failed: %s", esp_err_to_name(ret));
            return ret;
        }
        s_audio.playing = true;
    }

    // Apply software volume — works on a heap copy so caller's buffer is safe.
    int16_t *scaled = apply_volume(data, len);
    const void *write_src = scaled ? (const void *)scaled : (const void *)data;

    size_t bytes_written = 0;
    esp_err_t ret = i2s_channel_write(s_audio.tx_handle,
                                      write_src,
                                      len,
                                      &bytes_written,
                                      portMAX_DELAY);
    free(scaled);

    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_write failed: %s", esp_err_to_name(ret));
        return ret;
    }

    if (bytes_written != len) {
        ESP_LOGW(TAG, "short write: requested %u, wrote %u",
                 (unsigned)len, (unsigned)bytes_written);
    }

    return ESP_OK;
}

static esp_err_t pcm5102a_stop(void)
{
    if (!s_audio.initialized) {
        return ESP_ERR_INVALID_STATE;
    }

    if (s_audio.playing) {
        esp_err_t ret = i2s_channel_disable(s_audio.tx_handle);
        if (ret != ESP_OK) {
            ESP_LOGE(TAG, "i2s_channel_disable failed: %s", esp_err_to_name(ret));
            return ret;
        }
        s_audio.playing = false;
    }

    return ESP_OK;
}

static esp_err_t pcm5102a_set_volume(uint8_t percent)
{
    if (percent > 100) {
        percent = 100;
    }
    s_audio.volume = percent;
    ESP_LOGD(TAG, "volume set to %u%%", percent);
    return ESP_OK;
}

static esp_err_t pcm5102a_configure(const hal_audio_config_t *cfg)
{
    if (!cfg) {
        return ESP_ERR_INVALID_ARG;
    }
    if (!s_audio.initialized) {
        return ESP_ERR_INVALID_STATE;
    }

    // Disable channel before reconfiguring.
    if (s_audio.playing) {
        i2s_channel_disable(s_audio.tx_handle);
        s_audio.playing = false;
    }

    i2s_data_bit_width_t bit_width;
    switch (cfg->bits_per_sample) {
        case 8:  bit_width = I2S_DATA_BIT_WIDTH_8BIT;  break;
        case 16: bit_width = I2S_DATA_BIT_WIDTH_16BIT; break;
        case 24: bit_width = I2S_DATA_BIT_WIDTH_24BIT; break;
        case 32: bit_width = I2S_DATA_BIT_WIDTH_32BIT; break;
        default:
            ESP_LOGE(TAG, "unsupported bits_per_sample: %u", cfg->bits_per_sample);
            return ESP_ERR_INVALID_ARG;
    }

    i2s_slot_mode_t slot_mode = (cfg->channels == 1)
                                ? I2S_SLOT_MODE_MONO
                                : I2S_SLOT_MODE_STEREO;

    i2s_std_config_t std_cfg = {
        .clk_cfg  = I2S_STD_CLK_DEFAULT_CONFIG(cfg->sample_rate),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(bit_width, slot_mode),
        .gpio_cfg = {
            .mclk = I2S_GPIO_UNUSED,
            .bclk = s_audio.cfg.pin_bck,
            .ws   = s_audio.cfg.pin_ws,
            .dout = s_audio.cfg.pin_data,
            .din  = I2S_GPIO_UNUSED,
        },
    };

    esp_err_t ret = i2s_channel_reconfig_std_clock(s_audio.tx_handle,
                                                    &std_cfg.clk_cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_reconfig_std_clock failed: %s",
                 esp_err_to_name(ret));
        return ret;
    }

    ret = i2s_channel_reconfig_std_slot(s_audio.tx_handle, &std_cfg.slot_cfg);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "i2s_channel_reconfig_std_slot failed: %s",
                 esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "reconfigured: %"PRIu32" Hz, %u-bit, %u ch",
             cfg->sample_rate, cfg->bits_per_sample, cfg->channels);
    return ESP_OK;
}

// ---------------------------------------------------------------------------
// vtable + get
// ---------------------------------------------------------------------------

static const hal_audio_driver_t s_vtable = {
    .init       = pcm5102a_init,
    .deinit     = pcm5102a_deinit,
    .play       = pcm5102a_play,
    .stop       = pcm5102a_stop,
    .set_volume = pcm5102a_set_volume,
    .configure  = pcm5102a_configure,
    .name       = "PCM5102A",
};

const hal_audio_driver_t *drv_audio_pcm5102a_get(void)
{
    return &s_vtable;
}
