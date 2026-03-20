idf_component_register(
    SRCS
        "src/arduino_shim.c"
        "src/meshcore_shim.c"
    INCLUDE_DIRS "include"
    REQUIRES kernel thistle_hal driver
)
