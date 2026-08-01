// SPDX-License-Identifier: BSD-3-Clause
#include "a7682e_lifecycle.h"

#include <assert.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <unistd.h>

static atomic_bool s_waiter_started;
static atomic_bool s_waiter_acquired;

static void *blocked_waiter(void *unused)
{
    (void)unused;
    atomic_store(&s_waiter_started, true);
    bool acquired = a7682e_lifecycle_begin_operation();
    atomic_store(&s_waiter_acquired, acquired);
    if (acquired) {
        a7682e_lifecycle_finish_operation();
    }
    return NULL;
}

static void test_waiter_rejects_after_teardown_and_reinit_works(void)
{
    assert(a7682e_lifecycle_begin_init() == A7682E_INIT_ACQUIRED);
    a7682e_lifecycle_finish_init(true);
    assert(a7682e_lifecycle_begin_init() == A7682E_INIT_ALREADY_RUNNING);

    assert(a7682e_lifecycle_begin_stop());

    pthread_t waiter;
    atomic_store(&s_waiter_started, false);
    atomic_store(&s_waiter_acquired, true);
    assert(pthread_create(&waiter, NULL, blocked_waiter, NULL) == 0);
    while (!atomic_load(&s_waiter_started)) {
        usleep(100);
    }
    usleep(10000);

    a7682e_lifecycle_finish_stop();
    assert(pthread_join(waiter, NULL) == 0);
    assert(!atomic_load(&s_waiter_acquired));

    assert(a7682e_lifecycle_begin_init() == A7682E_INIT_ACQUIRED);
    a7682e_lifecycle_finish_init(true);
    assert(a7682e_lifecycle_begin_operation());
    a7682e_lifecycle_finish_operation();
    assert(a7682e_lifecycle_begin_stop());
    a7682e_lifecycle_finish_stop();
}

static atomic_int s_inside;
static atomic_int s_max_inside;
static int s_simulated_mode;
static int s_response_owner;

enum {
    SIMULATED_COMMAND_MODE = 0,
    SIMULATED_DATA_MODE = 1,
};

static void *serialized_operation(void *unused)
{
    int operation_id = (int)(intptr_t)unused;
    assert(a7682e_lifecycle_begin_operation());
    int inside = atomic_fetch_add(&s_inside, 1) + 1;
    int observed = atomic_load(&s_max_inside);
    while (inside > observed
           && !atomic_compare_exchange_weak(&s_max_inside, &observed, inside)) {
    }
    assert(s_simulated_mode == SIMULATED_DATA_MODE);
    s_simulated_mode = SIMULATED_COMMAND_MODE;
    s_response_owner = operation_id;
    usleep(1000);
    assert(s_response_owner == operation_id);
    s_simulated_mode = SIMULATED_DATA_MODE;
    atomic_fetch_sub(&s_inside, 1);
    a7682e_lifecycle_finish_operation();
    return NULL;
}

static void test_operations_are_serialized(void)
{
    assert(a7682e_lifecycle_begin_init() == A7682E_INIT_ACQUIRED);
    a7682e_lifecycle_finish_init(true);
    atomic_store(&s_inside, 0);
    atomic_store(&s_max_inside, 0);
    s_simulated_mode = SIMULATED_DATA_MODE;
    s_response_owner = -1;

    pthread_t threads[16];
    for (size_t i = 0; i < 16; ++i) {
        assert(pthread_create(&threads[i], NULL, serialized_operation,
                              (void *)(intptr_t)(i + 1)) == 0);
    }
    for (size_t i = 0; i < 16; ++i) {
        assert(pthread_join(threads[i], NULL) == 0);
    }
    assert(atomic_load(&s_max_inside) == 1);
    assert(s_simulated_mode == SIMULATED_DATA_MODE);
    assert(a7682e_lifecycle_begin_stop());
    a7682e_lifecycle_finish_stop();
}

int main(void)
{
    test_waiter_rejects_after_teardown_and_reinit_works();
    test_operations_are_serialized();
    puts("A7682E lifecycle tests passed");
    return 0;
}
