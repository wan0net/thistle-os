idf_component_register(
    SRCS "recovery_wifi.c"
    INCLUDE_DIRS "."
    REQUIRES
        esp_wifi
        esp_event
        nvs_flash
        log
        freertos
)
