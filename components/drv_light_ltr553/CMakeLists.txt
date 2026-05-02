idf_component_register(
    SRCS "src/drv_light_ltr553.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_driver_i2c esp_driver_gpio
)
