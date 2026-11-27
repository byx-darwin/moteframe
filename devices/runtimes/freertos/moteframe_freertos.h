#ifndef MOTEFRAME_FREERTOS_H
#define MOTEFRAME_FREERTOS_H

#include "FreeRTOS.h"
#include "task.h"

#if configSUPPORT_STATIC_ALLOCATION == 1

/* The firmware owns this task's stack and control block for its entire life. */
typedef struct {
    const char *name;
    UBaseType_t priority;
    StackType_t *stack;
    configSTACK_DEPTH_TYPE stack_words;
    StaticTask_t *storage;
} MoteframeFreeRtosTask;

static inline TaskHandle_t moteframe_freertos_create_task(
    const MoteframeFreeRtosTask *task, TaskFunction_t entry, void *argument)
{
    return xTaskCreateStatic(entry, task->name, task->stack_words, argument,
                             task->priority, task->stack, task->storage);
}

#endif

#if configUSE_TASK_NOTIFICATIONS

/* The caller must provide a live task and an IRQ priority allowed by the port. */
static inline void moteframe_freertos_notify_from_isr(TaskHandle_t task)
{
    BaseType_t woken = pdFALSE;
    vTaskNotifyGiveFromISR(task, &woken);
    portYIELD_FROM_ISR(woken);
}

/* Uses the calling task's default notification slot. */
static inline void moteframe_freertos_wait_notification(void)
{
    (void)ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
}

#endif
#endif
