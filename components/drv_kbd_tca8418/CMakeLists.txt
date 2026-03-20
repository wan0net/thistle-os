idf_component_register(
    SRCS "src/drv_kbd_tca8418.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_timer
)
