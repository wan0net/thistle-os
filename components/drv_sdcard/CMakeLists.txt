idf_component_register(
    SRCS "src/drv_sdcard.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_driver_spi esp_driver_gpio fatfs vfs sdmmc
)
