# Compatibility adapter for Espressif's ELF loader. The upstream component
# currently omits ESP32-C3 from its registry metadata even though the loader's
# generic RISC-V backend is architecture-compatible. Keep this dependency
# pinned and isolated so it can be removed with the planned Rust loader.

set(ELF_LOADER_DIR "${CMAKE_CURRENT_LIST_DIR}/vendor/elf_loader")
set(ELF_LOADER_SRCS
    "${ELF_LOADER_DIR}/src/esp_elf_symbol.c"
    "${ELF_LOADER_DIR}/src/esp_elf.c"
    "${ELF_LOADER_DIR}/src/esp_elf_adapter.c"
)

if(CONFIG_ELF_LOADER_CUSTOMER_SYMBOLS)
    list(APPEND ELF_LOADER_SRCS "${ELF_LOADER_DIR}/src/esp_all_symbol.c")
endif()

if(IDF_TARGET STREQUAL "esp32" OR IDF_TARGET STREQUAL "esp32s2" OR IDF_TARGET STREQUAL "esp32s3")
    list(APPEND ELF_LOADER_SRCS "${ELF_LOADER_DIR}/src/arch/esp_elf_xtensa.c")
    if(IDF_TARGET STREQUAL "esp32s2" AND CONFIG_ELF_LOADER_LOAD_PSRAM)
        list(APPEND ELF_LOADER_SRCS "${ELF_LOADER_DIR}/src/soc/esp_elf_esp32s2.c")
    endif()
elseif(IDF_TARGET STREQUAL "esp32c3" OR IDF_TARGET STREQUAL "esp32c6")
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

# Upstream normally injects these through its package-manager helper.
target_compile_definitions(${COMPONENT_LIB} PRIVATE
    ELF_LOADER_VER_MAJOR=1
    ELF_LOADER_VER_MINOR=3
    ELF_LOADER_VER_PATCH=1
)
