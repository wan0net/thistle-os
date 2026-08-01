cmake_minimum_required(VERSION 3.16)

if(NOT DEFINED IDF_TARGET)
    set(IDF_TARGET "esp32s3")
endif()
set(SDKCONFIG_DEFAULTS
    "sdkconfig.defaults;sdkconfig.defaults.${IDF_TARGET}"
)

set(EXTRA_COMPONENT_DIRS
    "${CMAKE_CURRENT_SOURCE_DIR}/components"
    "${CMAKE_CURRENT_SOURCE_DIR}/apps"
)

include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(thistle_os)
