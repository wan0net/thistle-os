idf_component_register(
    SRCS
        "src/kernel.c"
        "src/app_manager.c"
        "src/driver_manager.c"
        "src/driver_loader.c"
        "src/syscall_table.c"
        "src/ipc.c"
        "src/event.c"
        "src/elf_loader.c"
        "src/ota.c"
        "src/permissions.c"
        "src/signing.c"
        "src/wifi_manager.c"
        "src/ble_manager.c"
        "src/appstore_client.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal esp_timer freertos esp_partition fatfs vfs app_update esp_wifi esp_netif esp_event bt mbedtls esp_http_client driver
    PRIV_REQUIRES board_tdeck_pro
)
