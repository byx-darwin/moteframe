ENTRY(Reset)
MEMORY { FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 0x3E000 }
__stack_top = 0x20007F00;
SECTIONS {
 .vector_table : { KEEP(*(.vector_table)) } > FLASH
 .text : { *(.text*) *(.rodata*) } > FLASH
 .ARM.exidx : { *(.ARM.exidx*) } > FLASH
 /DISCARD/ : { *(.comment) }
}
ASSERT(SIZEOF(.vector_table) == 192, "48 vectors required")
ASSERT(SIZEOF(.data) == 0, "Loader cannot use initialized SRAM data")
ASSERT(SIZEOF(.bss) == 0, "Loader cannot use BSS")
