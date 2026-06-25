idf_component_register(
    SRCS
        "notes/notes_app.c"
        "notes/notes_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal
)
