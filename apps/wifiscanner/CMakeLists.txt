idf_component_register(
    SRCS
        "wifiscanner/wifiscanner_app.c"
        "wifiscanner/wifiscanner_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal
)
