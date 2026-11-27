ENTRY(Reset_Handler)

__sram_image_size = DEFINED(__wide_sram_probe) ? 8K : 4K;
__gatt_sram = DEFINED(__gatt_board_probe) && DEFINED(__probe_in_sram);
__gatt_event = DEFINED(__gatt_event_wake);
__rtos = DEFINED(__gatt_freertos);
/* Shift stack and executable together, keeping MSP capacity and the tested SRAM ceiling. */
__bare_gatt_sram = !__rtos && __gatt_sram;
__event_rtos_sram = __gatt_sram && __gatt_event && __rtos;
/* SDK v0.3.6 starts application SRAM at 0x20001818. Keep the ROM area
   and align our SRAM-only layout to the next vector boundary. */
__sram_shift = __gatt_sram && DEFINED(__gatt_dle) ? 0x1600 : 0;
__pool_space = DEFINED(__gatt_dle) && !__bare_gatt_sram ? 0x600 : 0;
__rtos_event_image = 0x20006e00;
__image_origin = __gatt_sram ? (__rtos ? (__gatt_event ? __rtos_event_image : 0x20008000)
                                    : (__gatt_event ? 0x20007a00 : 0x20008e00)) - __sram_shift + __pool_space :
    (DEFINED(__probe_in_sram) ? 0x20003000 : 0x10002000);
__gatt_flash_budget = (DEFINED(__gatt_ota) ? 2K : 0) + (__rtos ? 72K : (__gatt_event ? 68K : 64K)) + (DEFINED(__gatt_dle) ? 8K : 0);
__image_length = __gatt_sram ? 0x20018000 - __image_origin :
    (DEFINED(__gatt_board_probe) ? __gatt_flash_budget :
    (DEFINED(__probe_in_sram) ? __sram_image_size : 232K));
__ram_origin = DEFINED(__probe_in_sram) && !DEFINED(__gatt_board_probe)
    ? 0x20003000 + __sram_image_size : 0x20003000 - __sram_shift;
/* The event-driven SRAM trial uses 768 bytes for static RAM and 14 KiB for task stacks. */
__ram_limit = (__rtos ? (__event_rtos_sram ? 0x20003300 : 0x20003400) :
    (__bare_gatt_sram ? 0x20003a00 : (DEFINED(__gatt_board_probe) ? 0x20004000 : 0x20006f00))) - __sram_shift + __pool_space;
/* The XIP OTA receiver needs additional BLE task stack for image verification. */
__ota_task_space = __rtos && DEFINED(__gatt_ota) ? 3K : 0;
__diag_origin = __gatt_sram ? __image_origin - 0x100 : (__rtos ? 0x20007f00 + __ota_task_space :
    (DEFINED(__gatt_board_probe) ? 0x20009700 : 0x20006f00)) + __pool_space;
__stack_bottom = (__rtos ? (__event_rtos_sram ? 0x20006b00 : 0x20007800 + __ota_task_space) :
    (__bare_gatt_sram ? 0x20003a00 : (DEFINED(__gatt_board_probe) ? 0x20004000 : 0x20007000))) - __sram_shift + __pool_space;
__stack_limit = DEFINED(__gatt_board_probe) ? __diag_origin : 0x20008000;

MEMORY {
    IMAGE (rx) : ORIGIN = __image_origin, LENGTH = __image_length
    RAM   (rw) : ORIGIN = __ram_origin, LENGTH = __ram_limit - __ram_origin
    TASK_STACKS (rw) : ORIGIN = __ram_limit, LENGTH = __rtos ? __stack_bottom - __ram_limit : 0
    DIAG  (rw) : ORIGIN = __diag_origin, LENGTH = 0x100
    STACK (rw) : ORIGIN = __stack_bottom, LENGTH = __stack_limit - __stack_bottom
}

PHDRS {
    image PT_LOAD FLAGS(5);
    ram PT_LOAD FLAGS(6);
}

__stack_top = ORIGIN(STACK) + LENGTH(STACK);

SECTIONS {
    .vectors ORIGIN(IMAGE) : {
        KEEP(*(.vectors))
    } > IMAGE :image

    .text : ALIGN(4) {
        *(.text .text.*)
        *(.rodata .rodata.*)
    } > IMAGE :image

    .ARM.exidx : ALIGN(4) {
        *(.ARM.exidx .ARM.exidx.*)
    } > IMAGE :image

    .data : ALIGN(4) {
        __sdata = .;
        *(.ramfunc .ramfunc.*)
        *(.data .data.*)
        . = ALIGN(4);
        __edata = .;
    } > RAM AT> IMAGE :ram
    __sidata = LOADADDR(.data);
    __fr29_image_end = LOADADDR(.data) + SIZEOF(.data);

    .bss (NOLOAD) : ALIGN(4) {
        __sbss = .;
        *(.bss .bss.* COMMON)
        . = ALIGN(4);
        __ebss = .;
    } > RAM :ram

    .freertos_stacks ORIGIN(TASK_STACKS) (NOLOAD) : {
        KEEP(*(.freertos_stacks .freertos_stacks.*))
    } > TASK_STACKS :NONE

    .diagnostics ORIGIN(DIAG) (NOLOAD) : {
        __diagnostics_start = .;
        . += LENGTH(DIAG);
    } > DIAG :NONE
}

ASSERT(ADDR(.vectors) % 512 == 0, "vector table must be 512-byte aligned")
ASSERT(SIZEOF(.vectors) == 320, "unexpected vector table size")
ASSERT(LOADADDR(.data) % 4 == 0, "data source must be word-aligned")
ASSERT(__stack_top % 8 == 0, "stack must be 8-byte aligned")
ASSERT(!__gatt_sram || ORIGIN(DIAG) + LENGTH(DIAG) == ORIGIN(IMAGE),
       "unexpected GATT diagnostic boundary")
ASSERT(__ebss <= ORIGIN(DIAG), "application data overlaps diagnostics")
ASSERT(DEFINED(__gatt_board_probe) || ORIGIN(DIAG) + LENGTH(DIAG) == ORIGIN(STACK),
       "unexpected stack boundary")
ASSERT(!DEFINED(__gatt_board_probe) || __ebss <= __stack_bottom, "application data overlaps GATT stack")
ASSERT(!__rtos || SIZEOF(.freertos_stacks) == LENGTH(TASK_STACKS), "unexpected FreeRTOS stack reservations")

/* Product layout v1: FlashEnv starts at physical offset 0x3c000. */
ASSERT(DEFINED(__probe_in_sram) || __fr29_image_end <= 0x1003c000, "XIP payload overlaps SDK FlashEnv")
