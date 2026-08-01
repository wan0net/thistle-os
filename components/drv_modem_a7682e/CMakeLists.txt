idf_component_register(
    SRCS "src/drv_modem_a7682e.c" "src/a7682e_lifecycle.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_driver_gpio esp_driver_uart esp_netif esp_event
)
