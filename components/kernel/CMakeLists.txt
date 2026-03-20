idf_component_register(
    SRCS
        "src/kernel.c"
        "src/app_manager.c"
        "src/driver_manager.c"
        "src/syscall_table.c"
        "src/ipc.c"
        "src/event.c"
        "src/elf_loader.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal esp_timer freertos esp_partition fatfs vfs
    PRIV_REQUIRES board_tdeck_pro
)
