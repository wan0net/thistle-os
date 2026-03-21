# ThistleOS Kernel — all logic is now in Rust (kernel_rs component).
# This component exists only to provide the C headers and link kernel_rs.

# We need at least one C source for idf_component_register — use a stub.
file(WRITE "${CMAKE_CURRENT_BINARY_DIR}/kernel_stub.c" "/* Kernel logic is in Rust */\n")

idf_component_register(
    SRCS "${CMAKE_CURRENT_BINARY_DIR}/kernel_stub.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal crypto_ed25519 kernel_rs esp_timer freertos esp_partition fatfs vfs app_update esp_wifi esp_netif esp_event bt mbedtls esp_http_client driver spiffs
    PRIV_REQUIRES board_tdeck_pro
)
