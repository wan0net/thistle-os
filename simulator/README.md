# ThistleOS Simulator

SDL2 host-native simulator that runs the real Rust kernel with virtual hardware for development and testing. Not an emulator -- it compiles the actual kernel, display server, window manager, and all 14 built-in apps as a native desktop binary.

## Quick Start

```bash
cd simulator
mkdir -p build && cd build
cmake .. && make -j$(nproc)
./thistle_sim --device tdeck
```

Requires SDL2 (`brew install sdl2` / `apt install libsdl2-dev`) and a prior ESP-IDF build (for LVGL managed component).

## CLI Reference

| Flag | Description |
|------|-------------|
| `--device NAME` | Simulate a specific board (default: `tdeck`) |
| `--headless` | Run without SDL window (framebuffer only, for CI/testing) |
| `--timeout MS` | Exit after MS milliseconds (headless mode) |
| `--assert FILE` | Check log output against assertion file on exit |
| `--scenario FILE` | Load hardware scenario from JSON |
| `-h, --help` | Show help and list all devices |

## Supported Devices

| Device | Resolution | Keyboard | Touch | Radio | GPS | E-Paper |
|--------|-----------|----------|-------|-------|-----|---------|
| `tdeck-pro` | 320x240 | yes | yes | yes | yes | yes |
| `tdeck` | 320x240 | yes | yes | yes | yes | -- |
| `tdeck-plus` | 320x240 | yes | yes | yes | yes | -- |
| `tdisplay` | 320x170 | -- | yes | -- | -- | -- |
| `heltec-v3` | 128x64 | -- | -- | yes | -- | -- |
| `cardputer` | 240x135 | yes | -- | -- | -- | -- |
| `cyd-s022` | 240x320 | -- | yes | -- | -- | -- |
| `cyd-s028` | 320x240 | -- | yes | -- | -- | -- |
| `t3-s3` | 128x64 | -- | -- | yes | -- | -- |
| `c3-mini` | 128x64 | -- | -- | -- | -- | -- |

## Architecture

- **Native host build** -- compiles the real ThistleOS C and Rust code with `cc`/`clang`, not cross-compiled or emulated.
- **Real Rust kernel** -- all 57 kernel modules (app manager, IPC, events, permissions, signing, crypto, HAL registry, driver manager, etc.) linked in as a static library.
- **SDL2 display + input** -- LVGL renders to an SDL2 window; keyboard and mouse events route through the HAL input vtable.
- **Virtual I2C/SPI buses** -- device models respond to real register-level transactions, so drivers run unmodified.
- **Real pthreads** -- background tasks (WiFi, BLE, network) use pthreads, matching FreeRTOS task semantics.
- **dlopen for host drivers** -- standalone `.drv.dylib`/`.drv.so` files load at runtime, same as `.drv.elf` on device.

## Virtual I2C Device Models

| Model | Address | Description |
|-------|---------|-------------|
| PCF8563 | `0x51` | RTC -- returns simulated date/time, supports alarm registers |
| QMI8658C | `0x6A` | 6-axis IMU -- injectable accelerometer and gyroscope values |
| TCA8418 | `0x34` | Keyboard controller -- SDL key events injected as I2C scan codes |
| CST328 | `0x1A` | Touch controller -- SDL mouse events injected as touch coordinates |
| LTR-553 | `0x23` | Light/proximity sensor -- injectable lux and proximity values |

All models registered on bus 0. Drivers read/write registers through the same `i2c_master_transmit`/`i2c_master_transmit_receive` API as on real hardware.

## Scenario Engine

Pre-load hardware state via a JSON file passed with `--scenario`:

```json
{
  "power": {
    "voltage_mv": 3700,
    "percent": 45,
    "state": 1
  },
  "gps": {
    "latitude": 37.7749,
    "longitude": -122.4194,
    "altitude": 10.0,
    "satellites": 8,
    "fix": true
  },
  "imu": {
    "accel": [0.0, 0.0, 9.81],
    "gyro": [0.0, 0.0, 0.0]
  }
}
```

Values are applied to fake HAL drivers at boot. Unspecified fields use defaults.

## Testing

### Unit Tests

```bash
cd simulator/build
cmake .. && make thistle_sim_tests
./thistle_sim_tests
```

### Integration Tests

Headless boot tests across all 10 devices, HAL completeness checks, driver init validation, and scenario engine tests:

```bash
bash tests/run_integration_tests.sh
```

### Assertion File Format

Assertion files contain one pattern per line:

```
+kernel_init: 0          # Pattern MUST appear in log output
-FATAL                   # Pattern must NOT appear in log output
# This is a comment
```

- `+pattern` -- required (test fails if pattern never appears)
- `-pattern` -- forbidden (test fails if pattern appears)
- Lines starting with `#` and blank lines are ignored

### CI

Integration tests run automatically on every PR via GitHub Actions. The headless mode (`--headless --timeout 5000`) enables running the full simulator in CI without a display server.

## Adding a Device Model

Implement `sim_i2c_device_ops_t` callbacks and register on the virtual bus:

```c
#include "sim_i2c_bus.h"

static esp_err_t my_on_read(sim_i2c_device_t *dev,
                            const uint8_t *tx, size_t tx_len,
                            uint8_t *rx, size_t rx_len) {
    // tx[0] is register address, fill rx with register data
    return ESP_OK;
}

static esp_err_t my_on_write(sim_i2c_device_t *dev,
                             const uint8_t *buf, size_t len) {
    // buf[0] is register address, buf[1..] is data
    return ESP_OK;
}

static const sim_i2c_device_ops_t my_ops = { .on_read = my_on_read, .on_write = my_on_write };
static my_model_state_t my_state = { /* ... */ };

// In board_simulator.c:
sim_i2c_bus_add_model(0, 0xNN, &my_ops, &my_state);
```

## Building Host Drivers

Standalone drivers can be built as host shared libraries for the simulator:

1. Create a Cargo.toml with `crate-type = ["cdylib"]`
2. Export a `driver_init` entry point:
   ```rust
   #[no_mangle]
   pub extern "C" fn driver_init(config: *const std::os::raw::c_char) -> i32 {
       // Initialize driver, register HAL vtable
       0 // ESP_OK
   }
   ```
3. Build for host: `cargo build --release`
4. Copy to simulator sdcard: `cp target/release/libmy_driver.dylib sdcard/drivers/my_driver.drv.dylib`

The simulator discovers and `dlopen`s these automatically at boot.

---

License: BSD-3-Clause. See the repository root LICENSE file.
