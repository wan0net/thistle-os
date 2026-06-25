idf_component_register(
    SRCS
        "vault/vault_app.c"
        "vault/vault_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal mbedtls
)
