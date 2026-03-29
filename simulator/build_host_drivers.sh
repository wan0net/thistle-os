#!/bin/bash
# Build Rust drivers as host shared libraries for the simulator.
# These are placed in the simulator/sdcard/drivers/ directory
# alongside the real .drv.elf files.
set -e

KERNEL_RS_DIR="$(dirname "$0")/../components/kernel_rs"
OUT_DIR="$(dirname "$0")/sdcard/drivers"
mkdir -p "$OUT_DIR"

# Detect host architecture
if [ "$(uname -s)" = "Darwin" ]; then
    TARGET="$(uname -m)-apple-darwin"
    EXT="dylib"
else
    TARGET="$(uname -m)-unknown-linux-gnu"
    EXT="so"
fi

# For now, document how to build a standalone driver as a .dylib/.so
cat <<EOF
Host driver build for simulator
================================
Target: $TARGET
Extension: .$EXT
Output: $OUT_DIR

To build a driver as a host shared library:

1. Create a Cargo.toml for the driver with crate-type = ["cdylib"]
2. Export a driver_init function:

   #[no_mangle]
   pub extern "C" fn driver_init(config: *const std::os::raw::c_char) -> i32 {
       // Driver initialization code
       0 // ESP_OK
   }

3. Build: cargo build --release --target $TARGET
4. Copy the .dylib/.so to $OUT_DIR/

Example:
  cp target/$TARGET/release/libmy_driver.$EXT $OUT_DIR/my_driver.drv.$EXT

The simulator will find and dlopen it automatically.
EOF
