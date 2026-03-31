idf_component_register(
    SRCS "main.c"
    INCLUDE_DIRS "."
    REQUIRES kernel thistle_hal ui apps_builtin unity test_thistle
)
