idf_component_register(
    SRCS
        "reader/reader_app.c"
        "reader/reader_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal
)
