idf_component_register(
    SRCS
        "test_hal_registry.c"
        "test_event_bus.c"
        "test_ipc.c"
        "test_app_manager.c"
        "test_syscall_table.c"
        "test_epaper_refresh.c"
        "test_driver_vtable.c"
        "test_kernel_integration.c"
    INCLUDE_DIRS "."
    REQUIRES unity thistle_hal kernel ui esp_timer freertos
    WHOLE_ARCHIVE
)
