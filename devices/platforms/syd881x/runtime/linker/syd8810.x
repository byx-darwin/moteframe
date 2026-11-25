/* Current SDK scatter contract. ROM/shared lower SRAM must never be cleared. */
ENTRY(Reset)
MEMORY {
    FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 0x3E000
    RAM (rw) : ORIGIN = 0x200029A0, LENGTH = 0x5660
}
__stack_top = ORIGIN(RAM) + LENGTH(RAM);
__stack_bottom = __stack_top - 0x1000;
SECTIONS {
    .vector_table ORIGIN(FLASH) : { KEEP(*(.vector_table)) } > FLASH
    .text : { *(.text*) *(.rodata*) *(.ARM.extab*) } > FLASH
    .ARM.exidx : { *(.ARM.exidx*) } > FLASH
    .data : ALIGN(4) {
        __sdata = .; *(.data*) . = ALIGN(4); __edata = .;
    } > RAM AT> FLASH
    __sidata = LOADADDR(.data);
    .bss (NOLOAD) : ALIGN(4) {
        __sbss = .; *(.bss*) *(COMMON) . = ALIGN(4); __ebss = .;
    } > RAM
    .stack __stack_bottom (NOLOAD) : { . = . + 0x1000; } > RAM
    /DISCARD/ : { *(.comment) }
}
ASSERT(SIZEOF(.vector_table) == 0xC0, "Expected 48 SDK vector entries")
ASSERT(__ebss <= __stack_bottom, "Application data collides with stack")
ASSERT(__sdata >= ORIGIN(RAM), "ROM reserved RAM overlap")
ASSERT(LOADADDR(.data) + SIZEOF(.data) <= ORIGIN(FLASH) + LENGTH(FLASH), "Flash overflow")
