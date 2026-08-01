#include <algorithm>
#include <cassert>

#define SX1262_LIFECYCLE_HOST_TEST
#include "../src/drv_radio_sx1262.cpp"

static size_t event_index(sx1262_test::Event event)
{
    const auto it = std::find(sx1262_test::events.begin(), sx1262_test::events.end(), event);
    assert(it != sx1262_test::events.end());
    return static_cast<size_t>(it - sx1262_test::events.begin());
}

static size_t event_count(sx1262_test::Event event)
{
    return static_cast<size_t>(std::count(
        sx1262_test::events.begin(), sx1262_test::events.end(), event));
}

static bool has_event(sx1262_test::Event event)
{
    std::lock_guard<std::mutex> lock(sx1262_test::events_mutex);
    return std::find(sx1262_test::events.begin(), sx1262_test::events.end(), event)
        != sx1262_test::events.end();
}

static radio_sx1262_config_t config()
{
    return {1, 2, 3, 4, 5, 8000000};
}

int main()
{
    auto cfg = config();

    sx1262_test::reset();
    sx1262_test::fail_task_create = true;
    assert(sx1262_init(&cfg) == ESP_ERR_NO_MEM);
    assert(!sx1262_test::interrupt_attached);
    assert(event_index(sx1262_test::Event::Detach) < event_index(sx1262_test::Event::RadioDelete));
    assert(event_index(sx1262_test::Event::RadioDelete) < event_index(sx1262_test::Event::ModuleDelete));
    assert(event_index(sx1262_test::Event::ModuleDelete) < event_index(sx1262_test::Event::HalDelete));
    assert(event_count(sx1262_test::Event::SemaphoreDelete) == 1);

    sx1262_test::fail_task_create = false;
    assert(sx1262_init(&cfg) == ESP_OK);
    sx1262_deinit();

    sx1262_test::reset();
    sx1262_test::fail_semaphore_create = true;
    assert(sx1262_init(&cfg) == ESP_ERR_NO_MEM);
    assert(!sx1262_test::interrupt_attached);
    assert(event_count(sx1262_test::Event::Attach) == 0);
    assert(event_count(sx1262_test::Event::RadioDelete) == 1);

    sx1262_test::fail_semaphore_create = false;
    assert(sx1262_init(&cfg) == ESP_OK);
    sx1262_deinit();

    sx1262_test::reset();
    assert(sx1262_init(&cfg) == ESP_OK);
    assert(sx1262_start_receive(nullptr, nullptr) == ESP_OK);
    sx1262_test::block_read = true;
    dio1_isr();

    {
        std::unique_lock<std::mutex> lock(sx1262_test::read_mutex);
        sx1262_test::read_cv.wait(lock, [] { return sx1262_test::read_entered; });
    }

    std::thread deinit_thread([] { sx1262_deinit(); });
    while (!has_event(sx1262_test::Event::Detach)) std::this_thread::yield();
    assert(!has_event(sx1262_test::Event::RadioDelete));
    {
        std::lock_guard<std::mutex> lock(sx1262_test::read_mutex);
        sx1262_test::release_read = true;
    }
    sx1262_test::read_cv.notify_all();
    deinit_thread.join();
    sx1262_deinit();

    assert(!sx1262_test::interrupt_attached);
    assert(event_index(sx1262_test::Event::Detach) < event_index(sx1262_test::Event::TaskExit));
    assert(event_index(sx1262_test::Event::TaskExit) < event_index(sx1262_test::Event::Standby));
    assert(event_index(sx1262_test::Event::Standby) < event_index(sx1262_test::Event::RadioDelete));
    assert(sx1262_test::rearm_calls == 1);

    sx1262_test::reset();
    assert(sx1262_init(&cfg) == ESP_OK);
    sx1262_test::block_isr = true;
    std::thread isr_thread([] { dio1_isr(); });
    {
        std::unique_lock<std::mutex> lock(sx1262_test::isr_mutex);
        sx1262_test::isr_cv.wait(lock, [] { return sx1262_test::isr_entered; });
    }
    std::thread isr_deinit_thread([] { sx1262_deinit(); });
    while (!has_event(sx1262_test::Event::Detach)) std::this_thread::yield();
    assert(!has_event(sx1262_test::Event::RadioDelete));
    {
        std::lock_guard<std::mutex> lock(sx1262_test::isr_mutex);
        sx1262_test::release_isr = true;
    }
    sx1262_test::isr_cv.notify_all();
    isr_thread.join();
    isr_deinit_thread.join();
    assert(event_index(sx1262_test::Event::Detach) < event_index(sx1262_test::Event::TaskExit));
    assert(event_index(sx1262_test::Event::TaskExit) < event_index(sx1262_test::Event::RadioDelete));

    for (int i = 0; i < 100; ++i) {
        sx1262_test::reset();
        assert(sx1262_init(&cfg) == ESP_OK);
        dio1_isr();
        sx1262_deinit();
        sx1262_deinit();
        assert(!sx1262_test::interrupt_attached);
    }
    return 0;
}
