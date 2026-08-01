// SPDX-License-Identifier: BSD-3-Clause
#include "a7682e_lifecycle.h"

#ifdef A7682E_LIFECYCLE_HOST_TEST
#include <pthread.h>

static pthread_mutex_t s_mutex = PTHREAD_MUTEX_INITIALIZER;
#define LIFECYCLE_LOCK()   (pthread_mutex_lock(&s_mutex) == 0)
#define LIFECYCLE_UNLOCK() ((void)pthread_mutex_unlock(&s_mutex))

#else
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"

static StaticSemaphore_t s_mutex_storage;
static SemaphoreHandle_t s_mutex;
static portMUX_TYPE s_mutex_creation_lock = portMUX_INITIALIZER_UNLOCKED;

static bool lifecycle_lock(void)
{
    if (!s_mutex) {
        portENTER_CRITICAL(&s_mutex_creation_lock);
        if (!s_mutex) {
            s_mutex = xSemaphoreCreateMutexStatic(&s_mutex_storage);
        }
        portEXIT_CRITICAL(&s_mutex_creation_lock);
    }
    return s_mutex && xSemaphoreTake(s_mutex, portMAX_DELAY) == pdTRUE;
}

#define LIFECYCLE_LOCK()   lifecycle_lock()
#define LIFECYCLE_UNLOCK() ((void)xSemaphoreGive(s_mutex))
#endif

/* Access is always protected by s_mutex. The mutex has process lifetime and
 * is deliberately never deleted: queued callers can therefore safely wake,
 * observe STOPPED, and reject the operation after teardown. */
static a7682e_lifecycle_state_t s_state = A7682E_LIFECYCLE_STOPPED;

a7682e_init_result_t a7682e_lifecycle_begin_init(void)
{
    if (!LIFECYCLE_LOCK()) {
        return A7682E_INIT_FAILED;
    }
    if (s_state == A7682E_LIFECYCLE_RUNNING) {
        LIFECYCLE_UNLOCK();
        return A7682E_INIT_ALREADY_RUNNING;
    }
    if (s_state != A7682E_LIFECYCLE_STOPPED) {
        LIFECYCLE_UNLOCK();
        return A7682E_INIT_FAILED;
    }
    s_state = A7682E_LIFECYCLE_STARTING;
    return A7682E_INIT_ACQUIRED;
}

void a7682e_lifecycle_finish_init(bool success)
{
    s_state = success ? A7682E_LIFECYCLE_RUNNING : A7682E_LIFECYCLE_STOPPED;
    LIFECYCLE_UNLOCK();
}

bool a7682e_lifecycle_begin_operation(void)
{
    if (!LIFECYCLE_LOCK()) {
        return false;
    }
    if (s_state != A7682E_LIFECYCLE_RUNNING) {
        LIFECYCLE_UNLOCK();
        return false;
    }
    return true;
}

void a7682e_lifecycle_finish_operation(void)
{
    LIFECYCLE_UNLOCK();
}

bool a7682e_lifecycle_begin_stop(void)
{
    if (!LIFECYCLE_LOCK()) {
        return false;
    }
    if (s_state != A7682E_LIFECYCLE_RUNNING) {
        LIFECYCLE_UNLOCK();
        return false;
    }
    s_state = A7682E_LIFECYCLE_STOPPING;
    return true;
}

void a7682e_lifecycle_finish_stop(void)
{
    s_state = A7682E_LIFECYCLE_STOPPED;
    LIFECYCLE_UNLOCK();
}
