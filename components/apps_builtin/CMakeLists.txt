idf_component_register(
    SRCS
        "launcher/launcher_app.c"
        "launcher/launcher_ui.c"
        "settings/settings_app.c"
        "settings/settings_ui.c"
        "file_manager/filemgr_app.c"
        "file_manager/filemgr_ui.c"
        "reader/reader_app.c"
        "reader/reader_ui.c"
        "messenger/messenger_app.c"
        "messenger/messenger_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal
)
