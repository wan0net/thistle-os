idf_component_register(
    SRCS
        "appstore/appstore_app.c"
        "appstore/appstore_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal mbedtls esp_http_client
)
