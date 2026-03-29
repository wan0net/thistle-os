/*
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026 ThistleOS Contributors
 */
#include "sim_audio.h"
#include <stdio.h>
#include <stddef.h>

static esp_err_t sim_audio_init(const void *config)
{
    (void)config;
    printf("[sim_audio] init\n");
    return ESP_OK;
}

static void sim_audio_deinit(void)
{
    printf("[sim_audio] deinit\n");
}

static esp_err_t sim_audio_play(const uint8_t *data, size_t len)
{
    (void)data;
    printf("[sim_audio] play %zu bytes\n", len);
    return ESP_OK;
}

static esp_err_t sim_audio_stop(void)
{
    printf("[sim_audio] stop\n");
    return ESP_OK;
}

static esp_err_t sim_audio_set_volume(uint8_t percent)
{
    printf("[sim_audio] set_volume %u%%\n", (unsigned)percent);
    return ESP_OK;
}

static esp_err_t sim_audio_configure(const hal_audio_config_t *cfg)
{
    printf("[sim_audio] configure sample_rate=%u bits=%u channels=%u\n",
           (unsigned)cfg->sample_rate,
           (unsigned)cfg->bits_per_sample,
           (unsigned)cfg->channels);
    return ESP_OK;
}

static const hal_audio_driver_t sim_audio_driver = {
    .init      = sim_audio_init,
    .deinit    = sim_audio_deinit,
    .play      = sim_audio_play,
    .stop      = sim_audio_stop,
    .set_volume = sim_audio_set_volume,
    .configure = sim_audio_configure,
    .name      = "Simulator Audio",
};

const hal_audio_driver_t *sim_audio_get(void)
{
    return &sim_audio_driver;
}
