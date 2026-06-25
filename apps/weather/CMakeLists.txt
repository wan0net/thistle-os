idf_component_register(
    SRCS
        "weather/weather_app.c"
        "weather/weather_ui.c"
    INCLUDE_DIRS "."
    REQUIRES kernel ui thistle_hal
)
