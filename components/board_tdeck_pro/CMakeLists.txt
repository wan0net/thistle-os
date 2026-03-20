idf_component_register(
    SRCS "src/board_tdeck_pro.c"
    INCLUDE_DIRS "include"
    REQUIRES thistle_hal drv_epaper_gdeq031t10 drv_kbd_tca8418 drv_touch_cst328 drv_radio_sx1262 drv_gps_mia_m10q drv_audio_pcm5102a drv_imu_bhi260ap drv_light_ltr553 drv_power_tp4065b drv_sdcard driver
)
