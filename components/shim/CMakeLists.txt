idf_component_register(
    SRCS
        "src/arduino_shim.c"
        "src/meshcore_shim.cpp"
    INCLUDE_DIRS "include"
    REQUIRES kernel thistle_hal driver esp_driver_i2c esp_driver_spi esp_driver_gpio esp_driver_uart
)
