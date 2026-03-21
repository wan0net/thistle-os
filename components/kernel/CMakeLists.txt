# ThistleOS Kernel — C sources that remain (boot, drivers, signing, apps)
# Permissions, IPC, and event bus are provided by the Rust kernel (kernel_rs).

idf_component_register(
    SRCS
        "src/kernel.c"
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
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal crypto_ed25519 kernel_rs esp_timer freertos esp_partition fatfs vfs app_update esp_wifi esp_netif esp_event bt mbedtls esp_http_client driver spiffs
    PRIV_REQUIRES board_tdeck_pro
)
