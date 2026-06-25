idf_component_register(
    SRCS
        "launcher/launcher_app.c"
        "launcher/launcher_ui.c"
        "settings/settings_app.c"
        "settings/settings_ui.c"
        "messenger/messenger_app.c"
        "messenger/messenger_transport.c"
        "messenger/messenger_ui.c"
        "assistant/assistant_app.c"
        "assistant/assistant_ui.c"
        "terminal/terminal_app.c"
        "terminal/terminal_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal mbedtls esp_http_client drv_modem_a7682e
)
