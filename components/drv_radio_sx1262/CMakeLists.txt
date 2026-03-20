idf_component_register(
    SRCS "src/drv_radio_sx1262.cpp"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_timer
)
