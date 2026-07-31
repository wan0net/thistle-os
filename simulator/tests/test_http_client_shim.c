/* Simulator fixture tests for the typed HTTP client boundary. */
#include "test_runner.h"
#include "thistle_http_shim.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

typedef struct {
    uint8_t data[128];
    int length;
} response_capture_t;

static int capture_response(const uint8_t *data, int data_len, void *user_data)
{
    response_capture_t *capture = user_data;
    if (!capture || !data || data_len < 0 || data_len > (int)sizeof(capture->data)) {
        return -1;
    }
    memcpy(capture->data, data, (size_t)data_len);
    capture->length = data_len;
    return 0;
}

TEST(http_shim_delivers_fixture_bytes_status_and_cleanup) {
    static const char fixture[] = "{\"apps\":[{\"id\":\"fixture\"}]}";
    static const char path[] = "/tmp/thistle-http-shim-fixture.json";
    static const char url[] = "file:///tmp/thistle-http-shim-fixture.json";

    FILE *file = fopen(path, "wb");
    ASSERT_TRUE(file != NULL);
    ASSERT_EQ((int)fwrite(fixture, 1, sizeof(fixture) - 1, file),
              (int)sizeof(fixture) - 1);
    ASSERT_EQ(fclose(file), 0);

    response_capture_t capture = {0};
    void *client = thistle_http_client_init(url, 2500, capture_response, &capture);
    ASSERT_TRUE(client != NULL);
    ASSERT_EQ(thistle_http_client_perform(client), 0);
    ASSERT_EQ(capture.length, (int)sizeof(fixture) - 1);
    ASSERT_EQ(memcmp(capture.data, fixture, sizeof(fixture) - 1), 0);
    /* libcurl file:// responses have no HTTP status, but the accessor must be safe. */
    ASSERT_EQ(thistle_http_client_get_status_code(client), 0);
    ASSERT_EQ(thistle_http_client_cleanup(client), 0);
    ASSERT_EQ(unlink(path), 0);
}
