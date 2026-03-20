/*
 * test_epaper_refresh.c — Unit tests for the e-paper refresh tracker
 *
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Each test calls epaper_refresh_init(320, 240) to reset all state.
 * The tracker uses static globals, so re-init is the standard reset path.
 */

#include "unity.h"
#include "ui/epaper_refresh.h"

#define DISP_W 320
#define DISP_H 240

/* --------------------------------------------------------------------------
 * Tests
 * -------------------------------------------------------------------------- */

TEST_CASE("epaper_refresh_init returns ESP_OK", "[epaper]")
{
    esp_err_t ret = epaper_refresh_init(DISP_W, DISP_H);
    TEST_ASSERT_EQUAL(ESP_OK, ret);
}

TEST_CASE("epaper_refresh_is_dirty is false immediately after init", "[epaper]")
{
    TEST_ASSERT_EQUAL(ESP_OK, epaper_refresh_init(DISP_W, DISP_H));
    TEST_ASSERT_FALSE(epaper_refresh_is_dirty());
}

TEST_CASE("epaper_refresh_mark_dirty sets dirty flag and correct bounds", "[epaper]")
{
    TEST_ASSERT_EQUAL(ESP_OK, epaper_refresh_init(DISP_W, DISP_H));

    epaper_refresh_mark_dirty(10, 10, 100, 50);

    TEST_ASSERT_TRUE(epaper_refresh_is_dirty());

    uint16_t x1, y1, x2, y2;
    epaper_refresh_get_bounds(&x1, &y1, &x2, &y2);

    TEST_ASSERT_EQUAL_UINT16(10,  x1);
    TEST_ASSERT_EQUAL_UINT16(10,  y1);
    TEST_ASSERT_EQUAL_UINT16(100, x2);
    TEST_ASSERT_EQUAL_UINT16(50,  y2);
}

TEST_CASE("epaper_refresh_mark_dirty with two non-overlapping areas produces union bounds", "[epaper]")
{
    TEST_ASSERT_EQUAL(ESP_OK, epaper_refresh_init(DISP_W, DISP_H));

    /* Area 1: top-left region */
    epaper_refresh_mark_dirty(0, 0, 50, 50);
    /* Area 2: bottom-right region — does not overlap area 1 */
    epaper_refresh_mark_dirty(200, 150, 319, 239);

    TEST_ASSERT_TRUE(epaper_refresh_is_dirty());

    uint16_t x1, y1, x2, y2;
    epaper_refresh_get_bounds(&x1, &y1, &x2, &y2);

    /* Union bounding box must span both areas */
    TEST_ASSERT_EQUAL_UINT16(0,   x1);
    TEST_ASSERT_EQUAL_UINT16(0,   y1);
    TEST_ASSERT_EQUAL_UINT16(319, x2);
    TEST_ASSERT_EQUAL_UINT16(239, y2);
}

TEST_CASE("epaper_refresh_mark_full sets bounds to full display", "[epaper]")
{
    TEST_ASSERT_EQUAL(ESP_OK, epaper_refresh_init(DISP_W, DISP_H));

    epaper_refresh_mark_full();

    TEST_ASSERT_TRUE(epaper_refresh_is_dirty());

    uint16_t x1, y1, x2, y2;
    epaper_refresh_get_bounds(&x1, &y1, &x2, &y2);

    TEST_ASSERT_EQUAL_UINT16(0,               x1);
    TEST_ASSERT_EQUAL_UINT16(0,               y1);
    TEST_ASSERT_EQUAL_UINT16(DISP_W - 1, x2);
    TEST_ASSERT_EQUAL_UINT16(DISP_H - 1, y2);
}

TEST_CASE("epaper_refresh_clear resets dirty flag and increments refresh counter", "[epaper]")
{
    TEST_ASSERT_EQUAL(ESP_OK, epaper_refresh_init(DISP_W, DISP_H));

    epaper_refresh_mark_dirty(0, 0, 100, 100);
    TEST_ASSERT_TRUE(epaper_refresh_is_dirty());

    uint32_t count_before = epaper_refresh_get_count();
    epaper_refresh_clear();

    TEST_ASSERT_FALSE(epaper_refresh_is_dirty());
    TEST_ASSERT_EQUAL_UINT32(count_before + 1, epaper_refresh_get_count());
}

/* --------------------------------------------------------------------------
 * Additional edge-case tests
 * -------------------------------------------------------------------------- */

TEST_CASE("test_epaper_dirty_bounds_union: two non-adjacent marks produce correct union", "[epaper]")
{
    TEST_ASSERT_EQUAL(ESP_OK, epaper_refresh_init(DISP_W, DISP_H));

    epaper_refresh_mark_dirty(10, 10, 50, 50);
    epaper_refresh_mark_dirty(100, 100, 200, 150);

    TEST_ASSERT_TRUE(epaper_refresh_is_dirty());

    uint16_t x1, y1, x2, y2;
    epaper_refresh_get_bounds(&x1, &y1, &x2, &y2);

    TEST_ASSERT_EQUAL_UINT16(10,  x1);
    TEST_ASSERT_EQUAL_UINT16(10,  y1);
    TEST_ASSERT_EQUAL_UINT16(200, x2);
    TEST_ASSERT_EQUAL_UINT16(150, y2);
}

TEST_CASE("test_epaper_clear_increments_counter: counter increases by exactly 1 per clear", "[epaper]")
{
    TEST_ASSERT_EQUAL(ESP_OK, epaper_refresh_init(DISP_W, DISP_H));

    uint32_t before = epaper_refresh_get_count();

    epaper_refresh_mark_dirty(0, 0, 10, 10);
    epaper_refresh_clear();

    TEST_ASSERT_EQUAL_UINT32(before + 1, epaper_refresh_get_count());
}

TEST_CASE("test_epaper_init_invalid_dimensions: init with 0x0 returns error", "[epaper]")
{
    esp_err_t ret = epaper_refresh_init(0, 0);
    TEST_ASSERT_NOT_EQUAL(ESP_OK, ret);
}
