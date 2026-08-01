set(thistle_main_requires kernel thistle_hal ui apps_builtin)

if(CONFIG_THISTLE_RUN_TESTS)
    list(APPEND thistle_main_requires unity test_thistle)
endif()

# The remaining built-in C applications are a migration bridge for the S3
# images. Other targets boot the small launcher and discover signed ELF apps
# from SPIFFS/SD, keeping generic non-PSRAM firmware within internal DRAM.
if(IDF_TARGET STREQUAL "esp32s3")
    list(APPEND thistle_main_requires
        notes reader vault file_manager wifiscanner navigator flashlight weather appstore
    )
endif()

idf_component_register(
    SRCS "main.c"
    INCLUDE_DIRS "."
    REQUIRES ${thistle_main_requires}
)

if(IDF_TARGET STREQUAL "esp32s3")
    target_compile_definitions(${COMPONENT_LIB} PRIVATE THISTLE_LEGACY_C_APPS=1)
endif()
