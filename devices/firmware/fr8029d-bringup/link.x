ENTRY(Reset_Handler)

__sram_image_size = DEFINED(__wide_sram_probe) ? 8K : 4K;
__gatt_sram = DEFINED(__gatt_board_probe) && DEFINED(__probe_in_sram);
__rtos = DEFINED(__gatt_freertos);
__image_origin = __gatt_sram ? (__rtos ? 0x20008000 : 0x20009800) :
    (DEFINED(__probe_in_sram) ? 0x20003000 : 0x10002000);
__image_length = __rtos ? (__gatt_sram ? 64K : 72K) :
    (DEFINED(__gatt_board_probe) ? (__gatt_sram ? 58K : 60K) :
    (DEFINED(__probe_in_sram) ? __sram_image_size : 1016K));
__ram_origin = DEFINED(__probe_in_sram) && !DEFINED(__gatt_board_probe)
    ? 0x20003000 + __sram_image_size : 0x20003000;
__ram_limit = __rtos ? 0x20003400 :
    (DEFINED(__gatt_board_probe) ? 0x20004000 : 0x20006f00);
__diag_origin = __rtos ? 0x20007f00 :
    (DEFINED(__gatt_board_probe) ? 0x20009700 : 0x20006f00);
__stack_bottom = __rtos ? 0x20007800 :
    (DEFINED(__gatt_board_probe) ? 0x20004000 : 0x20007000);
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
