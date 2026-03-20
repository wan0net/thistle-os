idf_component_register(
    SRCS "src/drv_power_tp4065b.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_adc
)
