idf_component_register(
    SRCS
        "flashlight/flashlight_app.c"
        "flashlight/flashlight_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal
)
