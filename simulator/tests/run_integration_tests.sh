#!/bin/bash
# ThistleOS Simulator Integration Tests
# Runs headless boot tests across multiple devices and scenarios.
set -e

SIM="$(dirname "$0")/../build/thistle_sim"
TESTS_DIR="$(dirname "$0")"
TIMEOUT=5000
PASS=0
FAIL=0
TOTAL=0

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

run_test() {
    local name="$1"
    local device="$2"
    local assert_file="$3"
    local extra_args="${4:-}"

    TOTAL=$((TOTAL + 1))
    printf "  %-50s " "$name"

    local cmd="$SIM --headless --timeout $TIMEOUT --assert $assert_file --device $device $extra_args"
    local output
    output=$($cmd 2>&1)
    local rc=$?

    if [ $rc -eq 0 ]; then
        echo -e "${GREEN}PASS${NC}"
        PASS=$((PASS + 1))
    else
        echo -e "${RED}FAIL${NC}"
        FAIL=$((FAIL + 1))
        echo "    Command: $cmd"
        echo "$output" | grep -E "(FAIL|error)" | head -5 | sed 's/^/    /'
    fi
}

echo "=== ThistleOS Simulator Integration Tests ==="
echo ""

# Check simulator binary exists
if [ ! -x "$SIM" ]; then
    echo "ERROR: Simulator not found at $SIM"
    echo "Build it first: cd simulator/build && cmake .. && make -j\$(nproc)"
    exit 1
fi

echo "[Boot Tests — All Devices]"
for device in tdeck-pro tdeck tdeck-plus tdisplay heltec-v3 cardputer t3-s3 rak3312; do
    run_test "boot/$device" "$device" "$TESTS_DIR/boot_assertions.txt"
done

echo ""
echo "[HAL Completeness]"
run_test "hal/tdeck (full device)" "tdeck" "$TESTS_DIR/hal_complete_assertions.txt"
run_test "hal/heltec-v3 (minimal)" "heltec-v3" "$TESTS_DIR/hal_complete_assertions.txt"

echo ""
echo "[Driver Init — Virtual I2C]"
run_test "drivers/tdeck" "tdeck" "$TESTS_DIR/driver_init_assertions.txt"

echo ""
echo "[Radio + GPS Devices]"
run_test "radio_gps/tdeck" "tdeck" "$TESTS_DIR/radio_gps_assertions.txt"
run_test "radio_gps/tdeck-pro" "tdeck-pro" "$TESTS_DIR/radio_gps_assertions.txt"

echo ""
echo "[Scenario Engine]"
run_test "scenario/tdeck" "tdeck" "$TESTS_DIR/scenario_assertions.txt" "--scenario $TESTS_DIR/test_scenario.json"

echo ""
echo "[Minimal Devices]"
run_test "minimal/heltec-v3" "heltec-v3" "$TESTS_DIR/minimal_device_assertions.txt"
run_test "minimal/tdisplay" "tdisplay" "$TESTS_DIR/minimal_device_assertions.txt"

echo ""
echo "=== Results: $PASS/$TOTAL passed ==="
if [ $FAIL -gt 0 ]; then
    echo -e "${RED}$FAIL test(s) FAILED${NC}"
    exit 1
else
    echo -e "${GREEN}All tests passed${NC}"
    exit 0
fi
