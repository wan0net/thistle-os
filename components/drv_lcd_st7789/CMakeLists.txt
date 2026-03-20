idf_component_register(
    SRCS "src/drv_lcd_st7789.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_driver_ledc
)
