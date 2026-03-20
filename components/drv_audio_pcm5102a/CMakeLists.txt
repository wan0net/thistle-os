idf_component_register(
    SRCS "src/drv_audio_pcm5102a.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_driver_i2s
)
