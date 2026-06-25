idf_component_register(
    SRCS
        "file_manager/filemgr_app.c"
        "file_manager/filemgr_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal
)
