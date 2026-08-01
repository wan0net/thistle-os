#pragma once

#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <mutex>
#include <thread>
#include <vector>

using esp_err_t = int;
using spi_host_device_t = int;
using gpio_num_t = int;
using BaseType_t = int;
using UBaseType_t = unsigned int;
using TickType_t = unsigned int;
using hal_radio_rx_cb_t = void (*)(const uint8_t *, size_t, int, void *);

constexpr esp_err_t ESP_OK = 0;
constexpr esp_err_t ESP_FAIL = -1;
constexpr esp_err_t ESP_ERR_NO_MEM = 0x101;
constexpr esp_err_t ESP_ERR_INVALID_ARG = 0x102;
constexpr esp_err_t ESP_ERR_INVALID_STATE = 0x103;
constexpr BaseType_t pdFALSE = 0;
constexpr BaseType_t pdTRUE = 1;
constexpr BaseType_t pdPASS = 1;
constexpr TickType_t portMAX_DELAY = UINT32_MAX;
constexpr UBaseType_t configMAX_PRIORITIES = 25;
constexpr int RADIOLIB_ERR_NONE = 0;
constexpr int RADIOLIB_SX126X_SYNC_WORD_PRIVATE = 0x12;

#define IRAM_ATTR
#define ESP_LOGD(...) ((void)0)
#define ESP_LOGE(...) ((void)0)
#define ESP_LOGI(...) ((void)0)
#define portYIELD_FROM_ISR(...) ((void)0)

struct radio_sx1262_config_t {
    spi_host_device_t spi_host;
    gpio_num_t pin_cs;
    gpio_num_t pin_reset;
    gpio_num_t pin_busy;
    gpio_num_t pin_dio1;
    int spi_clock_hz;
};

struct hal_radio_driver_t {
    esp_err_t (*init)(const void *);
    void (*deinit)(void);
    esp_err_t (*set_frequency)(uint32_t);
    esp_err_t (*set_tx_power)(int8_t);
    esp_err_t (*set_bandwidth)(uint32_t);
    esp_err_t (*set_spreading_factor)(uint8_t);
    esp_err_t (*send)(const uint8_t *, size_t);
    esp_err_t (*start_receive)(hal_radio_rx_cb_t, void *);
    esp_err_t (*stop_receive)(void);
    int (*get_rssi)(void);
    esp_err_t (*sleep)(bool);
    const char *name;
};

namespace sx1262_test {

enum class Event {
    Attach,
    Detach,
    TaskCreate,
    TaskExit,
    Standby,
    RadioDelete,
    ModuleDelete,
    HalDelete,
    SemaphoreDelete,
};

inline std::mutex events_mutex;
inline std::vector<Event> events;
inline bool fail_task_create;
inline bool fail_semaphore_create;
inline bool interrupt_attached;
inline int begin_result = RADIOLIB_ERR_NONE;
inline int read_calls;
inline int rearm_calls;
inline std::mutex read_mutex;
inline std::condition_variable read_cv;
inline bool block_read;
inline bool read_entered;
inline bool release_read;
inline std::mutex isr_mutex;
inline std::condition_variable isr_cv;
inline bool block_isr;
inline bool isr_entered;
inline bool release_isr;

inline void record(Event event)
{
    std::lock_guard<std::mutex> lock(events_mutex);
    events.push_back(event);
}

inline void reset()
{
    std::lock_guard<std::mutex> lock(events_mutex);
    events.clear();
    fail_task_create = false;
    fail_semaphore_create = false;
    interrupt_attached = false;
    begin_result = RADIOLIB_ERR_NONE;
    read_calls = 0;
    rearm_calls = 0;
    block_read = false;
    read_entered = false;
    release_read = false;
    block_isr = false;
    isr_entered = false;
    release_isr = false;
}

struct TestTask {
    std::mutex mutex;
    std::condition_variable cv;
    unsigned notifications = 0;
    std::thread thread;
};

struct TestSemaphore {
    std::mutex mutex;
    std::condition_variable cv;
    bool available = false;
};

inline thread_local TestTask *current_task;

} // namespace sx1262_test

using TaskHandle_t = sx1262_test::TestTask *;
using SemaphoreHandle_t = sx1262_test::TestSemaphore *;

inline BaseType_t xTaskCreate(void (*entry)(void *), const char *, uint32_t, void *arg,
                              UBaseType_t, TaskHandle_t *out)
{
    sx1262_test::record(sx1262_test::Event::TaskCreate);
    if (sx1262_test::fail_task_create) {
        *out = nullptr;
        return pdFALSE;
    }
    auto *task = new sx1262_test::TestTask;
    *out = task;
    task->thread = std::thread([task, entry, arg] {
        sx1262_test::current_task = task;
        entry(arg);
        sx1262_test::record(sx1262_test::Event::TaskExit);
    });
    return pdPASS;
}

inline uint32_t ulTaskNotifyTake(BaseType_t, TickType_t)
{
    auto *task = sx1262_test::current_task;
    std::unique_lock<std::mutex> lock(task->mutex);
    task->cv.wait(lock, [task] { return task->notifications > 0; });
    task->notifications--;
    return 1;
}

inline BaseType_t xTaskNotifyGive(TaskHandle_t task)
{
    if (!task) return pdFALSE;
    {
        std::lock_guard<std::mutex> lock(task->mutex);
        task->notifications++;
    }
    task->cv.notify_one();
    return pdTRUE;
}

inline void vTaskNotifyGiveFromISR(TaskHandle_t task, BaseType_t *wake)
{
    *wake = xTaskNotifyGive(task);
}

inline void vTaskDelete(TaskHandle_t task)
{
    if (!task) return;
    xTaskNotifyGive(task);
    if (task->thread.joinable()) task->thread.join();
    delete task;
}

inline void pause_isr()
{
    std::unique_lock<std::mutex> lock(sx1262_test::isr_mutex);
    sx1262_test::isr_entered = true;
    sx1262_test::isr_cv.notify_all();
    if (sx1262_test::block_isr) {
        sx1262_test::isr_cv.wait(lock, [] { return sx1262_test::release_isr; });
    }
}

#define SX1262_TEST_ISR_PAUSE() pause_isr()

inline void vTaskDelay(TickType_t)
{
    std::this_thread::yield();
}

inline SemaphoreHandle_t xSemaphoreCreateBinary()
{
    if (sx1262_test::fail_semaphore_create) return nullptr;
    return new sx1262_test::TestSemaphore;
}

inline BaseType_t xSemaphoreGive(SemaphoreHandle_t semaphore)
{
    {
        std::lock_guard<std::mutex> lock(semaphore->mutex);
        semaphore->available = true;
    }
    semaphore->cv.notify_one();
    return pdTRUE;
}

inline BaseType_t xSemaphoreTake(SemaphoreHandle_t semaphore, TickType_t)
{
    std::unique_lock<std::mutex> lock(semaphore->mutex);
    semaphore->cv.wait(lock, [semaphore] { return semaphore->available; });
    semaphore->available = false;
    return pdTRUE;
}

inline void vSemaphoreDelete(SemaphoreHandle_t semaphore)
{
    sx1262_test::record(sx1262_test::Event::SemaphoreDelete);
    delete semaphore;
}

class ThistleHal {
public:
    ThistleHal(spi_host_device_t, int) {}
    ~ThistleHal() { sx1262_test::record(sx1262_test::Event::HalDelete); }
};

class Module {
public:
    Module(ThistleHal *, int, int, int, int) {}
    ~Module() { sx1262_test::record(sx1262_test::Event::ModuleDelete); }
};

class SX1262 {
public:
    explicit SX1262(Module *) {}
    ~SX1262() { sx1262_test::record(sx1262_test::Event::RadioDelete); }

    int begin(float, float, int, int, int, int) { return sx1262_test::begin_result; }
    void setDio1Action(void (*)(void)) {
        sx1262_test::interrupt_attached = true;
        sx1262_test::record(sx1262_test::Event::Attach);
    }
    void clearDio1Action() {
        sx1262_test::interrupt_attached = false;
        sx1262_test::record(sx1262_test::Event::Detach);
    }
    size_t getPacketLength() { return 1; }
    int readData(uint8_t *data, size_t) {
        data[0] = 0x42;
        std::unique_lock<std::mutex> lock(sx1262_test::read_mutex);
        sx1262_test::read_calls++;
        sx1262_test::read_entered = true;
        sx1262_test::read_cv.notify_all();
        if (sx1262_test::block_read) {
            sx1262_test::read_cv.wait(lock, [] { return sx1262_test::release_read; });
        }
        return RADIOLIB_ERR_NONE;
    }
    float getRSSI() { return -42.0f; }
    int startReceive() {
        sx1262_test::rearm_calls++;
        return RADIOLIB_ERR_NONE;
    }
    int standby() {
        sx1262_test::record(sx1262_test::Event::Standby);
        return RADIOLIB_ERR_NONE;
    }
    int setFrequency(float) { return RADIOLIB_ERR_NONE; }
    int setOutputPower(int8_t) { return RADIOLIB_ERR_NONE; }
    int setBandwidth(float) { return RADIOLIB_ERR_NONE; }
    int setSpreadingFactor(uint8_t) { return RADIOLIB_ERR_NONE; }
    int transmit(uint8_t *, size_t) { return RADIOLIB_ERR_NONE; }
    int sleep() { return RADIOLIB_ERR_NONE; }
};
