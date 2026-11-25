#ifndef FR29_FREERTOS_CONFIG_H
#define FR29_FREERTOS_CONFIG_H

#include <stdint.h>

/* The cold bring-up checks that ROM selected the 24 MHz MCU clock. */
#define configCPU_CLOCK_HZ                    24000000UL
#define configTICK_RATE_HZ                    1000U
#define configTICK_TYPE_WIDTH_IN_BITS         TICK_TYPE_WIDTH_32_BITS

#define configUSE_PREEMPTION                  1
#define configMAX_PRIORITIES                  2
#define configMINIMAL_STACK_SIZE              256U
#define configUSE_IDLE_HOOK                   0
#define configUSE_TICK_HOOK                   0
#define configUSE_TIMERS                      0
#ifdef FR29_EVENT_DRIVEN
#define configUSE_TASK_NOTIFICATIONS          1
#define INCLUDE_vTaskSuspend                  1
#else
#define configUSE_TASK_NOTIFICATIONS          0
#endif
#define configSUPPORT_STATIC_ALLOCATION       1
#define configSUPPORT_DYNAMIC_ALLOCATION      0
#define configCHECK_FOR_STACK_OVERFLOW         2

/* Use the Cortex-M33 port's single-security-state exception return. */
#define configRUN_FREERTOS_SECURE_ONLY        1
#define configENABLE_TRUSTZONE                0
#define configENABLE_MPU                      0
#define configENABLE_FPU                      0
#define configMAX_SYSCALL_INTERRUPT_PRIORITY  0x80U
#define configCHECK_HANDLER_INSTALLATION      1

_Noreturn void fr29_freertos_fail(uint32_t code);
#define configASSERT(condition)                 \
    do {                                        \
        if (!(condition)) {                     \
            fr29_freertos_fail(0x10000U | __LINE__); \
        }                                       \
    } while (0)

#endif
