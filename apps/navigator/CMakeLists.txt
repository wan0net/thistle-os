idf_component_register(
    SRCS
        "navigator/navigator_app.c"
        "navigator/navigator_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal
)
