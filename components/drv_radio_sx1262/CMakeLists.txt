idf_component_register(
    SRCS "src/drv_radio_sx1262.cpp"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver esp_driver_spi esp_driver_gpio esp_timer
)

# RadioLib headers deliberately hide base-class overloads with multi-arg variants.
target_compile_options(${COMPONENT_LIB} PRIVATE -Wno-overloaded-virtual)
