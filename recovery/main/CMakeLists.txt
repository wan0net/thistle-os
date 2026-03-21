idf_component_register(
    SRCS "main.c"
    INCLUDE_DIRS "."
    REQUIRES
        recovery_wifi
        recovery_ota
        recovery_ui
        esp_wifi
        esp_event
        nvs_flash
        app_update
        esp_partition
        freertos
        log
)
