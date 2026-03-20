idf_component_register(
    SRCS "src/drv_imu_bhi260ap.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal driver
)
