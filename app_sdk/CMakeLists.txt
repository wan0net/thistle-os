# ThistleOS App SDK -- CMake helpers for building .app.elf
#
# Usage in app CMakeLists.txt:
#   set(THISTLE_SDK_PATH "/path/to/app_sdk")
#   include(${THISTLE_SDK_PATH}/CMakeLists.txt)
#   thistle_app(my_app SRCS main.c)

cmake_minimum_required(VERSION 3.16)

function(thistle_app APP_NAME)
    cmake_parse_arguments(ARG "" "" "SRCS" ${ARGN})

    if(THISTLE_HOST_APP)
        add_library(${APP_NAME} MODULE ${ARG_SRCS})
    else()
        add_executable(${APP_NAME} ${ARG_SRCS})
    endif()

    target_include_directories(${APP_NAME} PRIVATE
        ${CMAKE_CURRENT_FUNCTION_LIST_DIR}/include
    )

    if(THISTLE_HOST_APP)
        target_compile_definitions(${APP_NAME} PRIVATE THISTLE_HOST_APP=1)
        target_compile_options(${APP_NAME} PRIVATE -fPIC)
        if(APPLE)
            target_link_options(${APP_NAME} PRIVATE -Wl,-undefined,dynamic_lookup)
        endif()
        set_target_properties(${APP_NAME} PROPERTIES PREFIX "")
    else()
        # Build as relocatable ELF (no standard libs, position independent)
        target_compile_options(${APP_NAME} PRIVATE
            -fPIC
            -nostdlib
            -fno-exceptions
        )

        target_link_options(${APP_NAME} PRIVATE
            -nostdlib
            -Wl,-relocatable
        )
    endif()

    set_target_properties(${APP_NAME} PROPERTIES
        SUFFIX ".app.elf"
    )
endfunction()
