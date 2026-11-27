#include "FreeRTOS.h"
#include "task.h"

#if defined(FR29_OTA_XIP)
#define FR29_BLE_STACK_BYTES 19456U
#elif defined(FR29_DLE_SRAM)
#define FR29_BLE_STACK_BYTES 13312U
#elif defined(FR29_EVENT_SRAM)
#define FR29_BLE_STACK_BYTES 13824U
#else
#define FR29_BLE_STACK_BYTES 16384U
#endif
#define FR29_BLE_STACK_WORDS (FR29_BLE_STACK_BYTES / sizeof(StackType_t))

StackType_t fr29_ble_task_stack[FR29_BLE_STACK_WORDS]
    __attribute__((section(".freertos_stacks.ble"), aligned(portBYTE_ALIGNMENT)));
StackType_t fr29_idle_task_stack[configMINIMAL_STACK_SIZE]
    __attribute__((section(".freertos_stacks.idle"), aligned(portBYTE_ALIGNMENT)));

_Static_assert(sizeof(fr29_ble_task_stack) == FR29_BLE_STACK_BYTES, "BLE task stack size");
_Static_assert(sizeof(fr29_idle_task_stack) == 1024U, "idle task stack size");

static StaticTask_t fr29_ble_task;
static StaticTask_t fr29_idle_task;

#ifdef FR29_EVENT_DRIVEN
static TaskHandle_t fr29_ble_handle;

void fr29_notify_from_isr(void)
{
    BaseType_t woken = pdFALSE;
    vTaskNotifyGiveFromISR(fr29_ble_handle, &woken);
    portYIELD_FROM_ISR(woken);
}

void fr29_wait_notification(void)
{
    (void)ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
}
#endif

/* Read via the debugger or by the Rust caller if scheduler startup returns. */
enum {
    FR29_FREERTOS_STARTING = 1,
    FR29_FREERTOS_TASK_CREATE_FAILED,
    FR29_FREERTOS_SCHEDULER_RETURNED,
    FR29_FREERTOS_STACK_OVERFLOW,
};
volatile uint32_t fr29_freertos_status;

_Noreturn void fr29_freertos_fail(uint32_t code)
{
    __asm volatile ("cpsid i" ::: "memory");
    fr29_freertos_status = code;
    for (;;) {
        __asm volatile ("wfi");
    }
}

void vApplicationStackOverflowHook(TaskHandle_t task, char *name)
{
    (void)task;
    (void)name;
    fr29_freertos_fail(FR29_FREERTOS_STACK_OVERFLOW);
}

void vApplicationGetIdleTaskMemory(StaticTask_t **task, StackType_t **stack,
                                   configSTACK_DEPTH_TYPE *depth)
{
    *task = &fr29_idle_task;
    *stack = fr29_idle_task_stack;
    *depth = configMINIMAL_STACK_SIZE;
}

void fr29_freertos_start(void (*entry)(void *))
{
    TaskHandle_t task = xTaskCreateStatic(entry, "BLE", FR29_BLE_STACK_WORDS, NULL,
                                         tskIDLE_PRIORITY + 1U, fr29_ble_task_stack,
                                         &fr29_ble_task);
    if (task == NULL) {
        fr29_freertos_status = FR29_FREERTOS_TASK_CREATE_FAILED;
        return;
    }
#ifdef FR29_EVENT_DRIVEN
    fr29_ble_handle = task;
#endif
    fr29_freertos_status = FR29_FREERTOS_STARTING;
    vTaskStartScheduler();
    fr29_freertos_status = FR29_FREERTOS_SCHEDULER_RETURNED;
}
