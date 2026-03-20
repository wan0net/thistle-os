idf_component_register(
    SRCS "main.c"
    INCLUDE_DIRS "."
    REQUIRES kernel thistle_hal board_tdeck_pro ui apps_builtin unity test_thistle
)
