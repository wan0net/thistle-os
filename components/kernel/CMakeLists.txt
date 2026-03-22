# ThistleOS Kernel — logic is in Rust (kernel_rs), C shims provide FFI bridges.

idf_component_register(
    SRCS "src/kernel_shims.c" "src/tk_wm_shims.c" "src/sdcard_shim.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal kernel_rs esp_timer freertos esp_partition fatfs vfs app_update esp_wifi esp_netif esp_event bt mbedtls esp_http_client driver spiffs nvs_flash sdmmc esp_adc
    PRIV_REQUIRES board_tdeck_pro
)
