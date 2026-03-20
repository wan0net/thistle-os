idf_component_register(
    SRCS "main.c"
    INCLUDE_DIRS "."
    REQUIRES kernel thistle_hal board_tdeck_pro ui unity test_thistle
)
