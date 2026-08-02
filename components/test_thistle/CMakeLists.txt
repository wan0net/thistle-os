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
        "test_permissions.c"
        "test_signing.c"
        "test_wifi_manager.c"
        "test_ota.c"
        "test_net_manager.c"
        "test_driver_loader.c"
        "test_appstore_client.c"
        "test_a7682e_sms_validation.c"
        "test_board_bus_registration.c"
        "test_appstore_http_shim.c"
    INCLUDE_DIRS "."
        "../drv_modem_a7682e/include"
    REQUIRES unity thistle_hal kernel ui esp_timer freertos
    WHOLE_ARCHIVE
)
