idf_component_register(
    SRCS
        "src/manager.c"
        "src/theme.c"
        "src/statusbar.c"
        "src/app_switcher.c"
        "src/epaper_refresh.c"
        "src/toast.c"
        "src/lvgl_wm.c"
        "src/widget_shims.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal kernel esp_timer lvgl__lvgl
)
