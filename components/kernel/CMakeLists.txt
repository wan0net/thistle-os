# ThistleOS Kernel — logic is in Rust (kernel_rs), C shims provide FFI bridges.

set(KERNEL_SRCS
    "src/kernel_shims.c" "src/tk_wm_shims.c" "src/sdcard_shim.c"
    "src/link_deps.c" "src/board_fallback.c" "src/board_fingerprint.c"
    "src/board_builtin_drivers.c"
)
set(KERNEL_REQUIRES
    thistle_hal kernel_rs esp_timer freertos esp_partition fatfs vfs app_update
    esp_wifi esp_netif esp_event mbedtls esp_http_client driver spiffs nvs_flash
    sdmmc esp_adc bt
)
if(CONFIG_BT_ENABLED)
    list(APPEND KERNEL_SRCS "src/ble_security_shim.c")
endif()

idf_component_register(
    SRCS ${KERNEL_SRCS}
    INCLUDE_DIRS "include"
    REQUIRES ${KERNEL_REQUIRES}
    PRIV_REQUIRES drv_crypto_mbedtls esp_driver_i2c esp_driver_uart esp_driver_spi esp_driver_gpio
                  drv_epaper_gdeq031t10 drv_kbd_tca8418 drv_touch_cst328
)
