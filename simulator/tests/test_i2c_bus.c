/*
 * Unit tests for virtual I2C bus and device models.
 * SPDX-License-Identifier: BSD-3-Clause
 */
#include "test_runner.h"
#include "sim_i2c_bus.h"
#include "sim_devices.h"
#include "driver/i2c_master.h"
#include <string.h>
#include <stdint.h>

TEST(i2c_bus_init_succeeds) {
    sim_i2c_bus_init();
    void *bus = sim_i2c_bus_get(0);
    ASSERT_TRUE(bus != NULL);
}

TEST(i2c_bus_get_out_of_range_returns_null) {
    ASSERT_TRUE(sim_i2c_bus_get(99) == NULL);
    ASSERT_TRUE(sim_i2c_bus_get(-1) == NULL);
}

TEST(i2c_add_device_returns_handle) {
    sim_i2c_bus_init();
    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);

    /* Register a PCF8563 model */
    dev_pcf8563_register(0, 0x51);

    /* Add device -- should find the model */
    i2c_device_config_t cfg = { .device_address = 0x51 };
    i2c_master_dev_handle_t dev = NULL;
    esp_err_t ret = i2c_master_bus_add_device(bus, &cfg, &dev);
    ASSERT_EQ(ret, 0);
    ASSERT_TRUE(dev != NULL);
    ASSERT_TRUE(dev != (void*)1);  /* Not the sentinel */
}

TEST(i2c_unregistered_address_returns_sentinel) {
    sim_i2c_bus_init();
    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);

    i2c_device_config_t cfg = { .device_address = 0xFF };
    i2c_master_dev_handle_t dev = NULL;
    esp_err_t ret = i2c_master_bus_add_device(bus, &cfg, &dev);
    ASSERT_EQ(ret, 0);
    /* Should get sentinel (backward compat) */
    ASSERT_TRUE(dev == (void*)(uintptr_t)1);
}

TEST(pcf8563_who_am_i) {
    sim_i2c_bus_init();
    dev_pcf8563_register(0, 0x51);

    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);
    i2c_device_config_t cfg = { .device_address = 0x51 };
    i2c_master_dev_handle_t dev = NULL;
    i2c_master_bus_add_device(bus, &cfg, &dev);

    /* Read CTRL1 register (0x00) */
    uint8_t reg = 0x00;
    uint8_t val = 0xFF;
    i2c_master_transmit_receive(dev, &reg, 1, &val, 1, 50);
    ASSERT_EQ(val, 0x00);  /* CTRL1 default is 0x00 */
}

TEST(pcf8563_reads_time) {
    sim_i2c_bus_init();
    dev_pcf8563_register(0, 0x51);

    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);
    i2c_device_config_t cfg = { .device_address = 0x51 };
    i2c_master_dev_handle_t dev = NULL;
    i2c_master_bus_add_device(bus, &cfg, &dev);

    /* Read 7 time registers starting at 0x02 */
    uint8_t reg = 0x02;
    uint8_t time_regs[7] = {0};
    i2c_master_transmit_receive(dev, &reg, 1, time_regs, 7, 50);

    /* Seconds should be valid BCD (0x00-0x59 masked to 0x7F) */
    uint8_t seconds = time_regs[0] & 0x7F;
    ASSERT_TRUE(seconds <= 0x59);

    /* Month register (index 5) should have century bit set for year >= 2000 */
    uint8_t month_raw = time_regs[5];
    ASSERT_TRUE((month_raw & 0x80) != 0);  /* Century bit set */
}

TEST(qmi8658c_who_am_i) {
    sim_i2c_bus_init();
    dev_qmi8658c_register(0, 0x6A);

    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);
    i2c_device_config_t cfg = { .device_address = 0x6A };
    i2c_master_dev_handle_t dev = NULL;
    i2c_master_bus_add_device(bus, &cfg, &dev);

    /* WHO_AM_I register 0x00 should return 0x05 */
    uint8_t reg = 0x00;
    uint8_t id = 0;
    i2c_master_transmit_receive(dev, &reg, 1, &id, 1, 50);
    ASSERT_EQ(id, 0x05);
}

TEST(qmi8658c_reads_accel_data) {
    sim_i2c_bus_init();
    dev_qmi8658c_register(0, 0x6A);

    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);
    i2c_device_config_t cfg = { .device_address = 0x6A };
    i2c_master_dev_handle_t dev = NULL;
    i2c_master_bus_add_device(bus, &cfg, &dev);

    /* Read 6 bytes from accel registers (0x35) */
    uint8_t reg = 0x35;
    uint8_t data[6] = {0};
    i2c_master_transmit_receive(dev, &reg, 1, data, 6, 50);

    /* Z-axis should be non-zero (default accel_z = 9.81 m/s^2) */
    int16_t az = (int16_t)((data[4]) | (data[5] << 8));
    ASSERT_TRUE(az != 0);  /* Should have non-zero Z accel */
}

TEST(tca8418_empty_fifo) {
    sim_i2c_bus_init();
    dev_tca8418_register(0, 0x34);

    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);
    i2c_device_config_t cfg = { .device_address = 0x34 };
    i2c_master_dev_handle_t dev = NULL;
    i2c_master_bus_add_device(bus, &cfg, &dev);

    /* KEY_LCK_EC register (0x02) should show 0 events */
    uint8_t reg = 0x02;
    uint8_t val = 0xFF;
    i2c_master_transmit_receive(dev, &reg, 1, &val, 1, 50);
    ASSERT_EQ(val & 0x0F, 0);  /* Event count = 0 */
}

TEST(tca8418_key_injection) {
    sim_i2c_bus_init();
    dev_tca8418_register(0, 0x34);

    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);
    i2c_device_config_t cfg = { .device_address = 0x34 };
    i2c_master_dev_handle_t dev = NULL;
    i2c_master_bus_add_device(bus, &cfg, &dev);

    /* Inject a key press */
    dev_tca8418_inject_key(0x41, true);  /* Key 'A' press */

    /* KEY_LCK_EC should show 1 event */
    uint8_t reg = 0x02;
    uint8_t val = 0;
    i2c_master_transmit_receive(dev, &reg, 1, &val, 1, 50);
    ASSERT_EQ(val & 0x0F, 1);

    /* KEY_EVENT_A (0x04) should return the event */
    reg = 0x04;
    val = 0;
    i2c_master_transmit_receive(dev, &reg, 1, &val, 1, 50);
    ASSERT_TRUE((val & 0x80) != 0);  /* Press bit set */
    ASSERT_EQ(val & 0x7F, 0x41);     /* Key code */
}

TEST(cst328_no_touch) {
    sim_i2c_bus_init();
    dev_cst328_register(0, 0x1A);

    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);
    i2c_device_config_t cfg = { .device_address = 0x1A };
    i2c_master_dev_handle_t dev = NULL;
    i2c_master_bus_add_device(bus, &cfg, &dev);

    /* Read touch count (register 0xD000 = write [0xD0, 0x00]) */
    uint8_t reg[2] = {0xD0, 0x00};
    uint8_t count = 0xFF;
    i2c_master_transmit_receive(dev, reg, 2, &count, 1, 50);
    ASSERT_EQ(count, 0);  /* No touch */
}

TEST(cst328_touch_injection) {
    sim_i2c_bus_init();
    dev_cst328_register(0, 0x1A);

    i2c_master_bus_handle_t bus = sim_i2c_bus_get(0);
    i2c_device_config_t cfg = { .device_address = 0x1A };
    i2c_master_dev_handle_t dev = NULL;
    i2c_master_bus_add_device(bus, &cfg, &dev);

    /* Inject touch at (100, 200) */
    dev_cst328_inject_touch(100, 200, true);

    /* Read touch count */
    uint8_t reg[2] = {0xD0, 0x00};
    uint8_t count = 0;
    i2c_master_transmit_receive(dev, reg, 2, &count, 1, 50);
    ASSERT_EQ(count, 1);  /* One touch */
}
