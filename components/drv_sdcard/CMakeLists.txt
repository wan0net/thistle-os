idf_component_register(
    SRCS "src/drv_sdcard.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver fatfs vfs sdmmc
)
