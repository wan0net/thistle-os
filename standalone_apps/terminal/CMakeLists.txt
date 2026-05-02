cmake_minimum_required(VERSION 3.16)
project(terminal C)

set(THISTLE_APP_SDK "${CMAKE_CURRENT_SOURCE_DIR}/../../app_sdk")
include(${THISTLE_APP_SDK}/CMakeLists.txt)

thistle_app(terminal SRCS main.c)

# libghostty-vt headers
target_include_directories(terminal PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/../../external/ghostty/include
)

# libghostty-vt library (built separately via Zig)
# For now, link against a pre-built .a if available
set(GHOSTTY_LIB "${CMAKE_CURRENT_SOURCE_DIR}/../../external/ghostty/zig-out/lib/libghostty-vt.a")
if(EXISTS ${GHOSTTY_LIB})
    target_link_libraries(terminal PRIVATE ${GHOSTTY_LIB})
else()
    message(WARNING "libghostty-vt.a not found — Terminal will not link until built")
    target_compile_definitions(terminal PRIVATE THISTLE_TERMINAL_NO_GHOSTTY=1)
endif()
