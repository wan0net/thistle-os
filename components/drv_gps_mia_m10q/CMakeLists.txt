idf_component_register(
    SRCS "src/drv_gps_mia_m10q.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_driver_uart esp_driver_gpio
)
