# Core C sources (always compiled)
set(KERNEL_SRCS
    "src/kernel.c"
    "src/app_manager.c"
    "src/driver_manager.c"
    "src/driver_loader.c"
    "src/syscall_table.c"
    "src/elf_loader.c"
    "src/ota.c"
    "src/signing.c"
    "src/wifi_manager.c"
    "src/ble_manager.c"
    "src/appstore_client.c"
    "src/net_manager.c"
    "src/manifest.c"
    "src/board_config.c"
    "src/display_server.c"
)

# C fallback sources — only compiled when Rust kernel is not available.
# When Rust kernel_rs is linked, these symbols come from Rust instead.
find_program(CARGO_BIN cargo HINTS "$ENV{HOME}/.cargo/bin")
if(CARGO_BIN)
    execute_process(
        COMMAND ${CARGO_BIN} +esp --version
        RESULT_VARIABLE CARGO_ESP_RESULT
        OUTPUT_QUIET ERROR_QUIET
    )
endif()

if(NOT CARGO_BIN OR NOT CARGO_ESP_RESULT EQUAL 0)
    message(STATUS "Rust kernel unavailable — compiling C fallbacks (permissions, ipc, event)")
    list(APPEND KERNEL_SRCS
        "src/permissions.c"
        "src/ipc.c"
        "src/event.c"
    )
else()
    message(STATUS "Rust kernel available — permissions, ipc, event provided by kernel_rs")
endif()

idf_component_register(
    SRCS ${KERNEL_SRCS}
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal crypto_ed25519 kernel_rs esp_timer freertos esp_partition fatfs vfs app_update esp_wifi esp_netif esp_event bt mbedtls esp_http_client driver spiffs
    PRIV_REQUIRES board_tdeck_pro
)
