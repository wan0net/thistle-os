idf_component_register(
    SRCS "recovery_ota.c"
    INCLUDE_DIRS "."
    REQUIRES
        app_update
        esp_partition
        esp_http_client
        esp_tls
        fatfs
        vfs
        sdmmc
        driver
        log
        freertos
        json
)
