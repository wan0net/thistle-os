# Compatibility adapter for Espressif's ELF loader. The upstream component
# currently omits ESP32-C3 from its registry metadata even though the loader's
# generic RISC-V backend is architecture-compatible. Keep this dependency
# pinned and isolated so it can be removed with the planned Rust loader.

set(ESP_ELF_LOADER_COMMIT "a7a883a2c71b2cdb553565550a203efbd24fbdc0")
set(ESP_ELF_LOADER_SHA256 "239801baceefdec38c30dd2dd56ad2817dcdb58d07b92b4cfc41a2e90be564bf")
set(ESP_ELF_LOADER_CACHE "${CMAKE_BINARY_DIR}/_deps/esp-elf-loader-${ESP_ELF_LOADER_COMMIT}.tar.gz")
set(ESP_ELF_LOADER_SOURCE "${CMAKE_BINARY_DIR}/_deps/esp-iot-solution-${ESP_ELF_LOADER_COMMIT}")

if(NOT EXISTS "${ESP_ELF_LOADER_SOURCE}/components/elf_loader/CMakeLists.txt")
    file(MAKE_DIRECTORY "${CMAKE_BINARY_DIR}/_deps")
    file(DOWNLOAD
        "https://github.com/espressif/esp-iot-solution/archive/${ESP_ELF_LOADER_COMMIT}.tar.gz"
        "${ESP_ELF_LOADER_CACHE}"
        EXPECTED_HASH "SHA256=${ESP_ELF_LOADER_SHA256}"
        TLS_VERIFY ON
        STATUS ESP_ELF_LOADER_DOWNLOAD_STATUS
    )
    list(GET ESP_ELF_LOADER_DOWNLOAD_STATUS 0 ESP_ELF_LOADER_DOWNLOAD_CODE)
    if(NOT ESP_ELF_LOADER_DOWNLOAD_CODE EQUAL 0)
        list(GET ESP_ELF_LOADER_DOWNLOAD_STATUS 1 ESP_ELF_LOADER_DOWNLOAD_ERROR)
        message(FATAL_ERROR "Could not download pinned ELF loader: ${ESP_ELF_LOADER_DOWNLOAD_ERROR}")
    endif()
    file(ARCHIVE_EXTRACT
        INPUT "${ESP_ELF_LOADER_CACHE}"
        DESTINATION "${CMAKE_BINARY_DIR}/_deps"
    )
endif()

set(ELF_LOADER_DIR "${ESP_ELF_LOADER_SOURCE}/components/elf_loader")
set(ELF_LOADER_SRCS
    "${ELF_LOADER_DIR}/src/esp_elf_symbol.c"
    "${ELF_LOADER_DIR}/src/esp_elf.c"
    "${ELF_LOADER_DIR}/src/esp_elf_adapter.c"
)

if(CONFIG_ELF_LOADER_CUSTOMER_SYMBOLS)
    list(APPEND ELF_LOADER_SRCS "${ELF_LOADER_DIR}/src/esp_all_symbol.c")
endif()

if(CONFIG_IDF_TARGET_ARCH_XTENSA)
    list(APPEND ELF_LOADER_SRCS "${ELF_LOADER_DIR}/src/arch/esp_elf_xtensa.c")
    if(CONFIG_IDF_TARGET_ESP32S2 AND CONFIG_ELF_LOADER_LOAD_PSRAM)
        list(APPEND ELF_LOADER_SRCS "${ELF_LOADER_DIR}/src/soc/esp_elf_esp32s2.c")
    endif()
elseif(CONFIG_IDF_TARGET_ARCH_RISCV)
    list(APPEND ELF_LOADER_SRCS "${ELF_LOADER_DIR}/src/arch/esp_elf_riscv.c")
else()
    message(FATAL_ERROR "ELF loader has no backend for ${IDF_TARGET}")
endif()

idf_component_register(
    SRCS ${ELF_LOADER_SRCS}
    INCLUDE_DIRS "${ELF_LOADER_DIR}/include"
    PRIV_REQUIRES spi_flash
    LDFRAGMENTS "${ELF_LOADER_DIR}/linker.lf"
)
