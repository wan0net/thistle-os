// Example ThistleOS driver (C)
// This is a template for writing hardware drivers as standalone .drv.elf files.

#include "thistle_driver.h"

#define TAG "example_drv"

// Your HAL vtable implementation would go here.
// For a real driver, you'd implement hal_input_driver_t, hal_display_driver_t, etc.

int driver_init(const char *config_json)
{
    thistle_log(TAG, "Example driver initializing");

    // Parse config_json for pins, addresses, etc.
    // Example: extract I2C bus index and address
    // void *i2c = hal_bus_get_i2c(0);

    // Register your vtable with the HAL
    // hal_input_register(&my_input_driver, NULL);

    thistle_log(TAG, "Example driver ready");
    return 0;
}
