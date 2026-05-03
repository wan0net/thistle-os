idf_component_register(
    SRCS "src/drv_epaper_gdeq031t10.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_driver_spi esp_driver_gpio
)
