/* Dedicated native CONTROL bring-up; preserve all existing ROM/shared RAM, diagnostic and stack boundaries. */
ENTRY(Reset)
MEMORY {
 FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 0x3E000
 RAM (rw) : ORIGIN = 0x200029A0, LENGTH = 0x1560
 DIAG (rw) : ORIGIN = 0x20003F00, LENGTH = 0x100
 STACK (rw) : ORIGIN = 0x20004000, LENGTH = 0x3F00
}
__stack_bottom = ORIGIN(STACK);
__stack_top = ORIGIN(STACK) + LENGTH(STACK);
SECTIONS {
 .vector_table ORIGIN(FLASH) : { KEEP(*(.vector_table)) } > FLASH
 .text : { *(.text*) *(.rodata*) *(.ARM.extab*) } > FLASH
 .bringup_profile : ALIGN(4) { KEEP(*(.bringup_profile)) } > FLASH
 .board_configuration : ALIGN(4) { KEEP(*(.board_configuration)) } > FLASH
 .candidate_manifest : ALIGN(4) { KEEP(*(.candidate_manifest)) } > FLASH
 .ARM.exidx : { *(.ARM.exidx*) } > FLASH
 .data : ALIGN(4) { __sdata = .; *(.data*) . = ALIGN(4); __edata = .; } > RAM AT> FLASH
 __sidata = LOADADDR(.data);
 .bss (NOLOAD) : ALIGN(4) { __sbss = .; *(.bss*) *(COMMON) . = ALIGN(4); __ebss = .; } > RAM
 .probe_status (NOLOAD) : { KEEP(*(.probe_status)) } > DIAG
 .host_error (NOLOAD) : { KEEP(*(.host_error)) } > DIAG
 .stack (NOLOAD) : { . = . + LENGTH(STACK); } > STACK
 /DISCARD/ : { *(.comment) }
}
ASSERT(SIZEOF(.vector_table) == 192, "48 vectors required")
ASSERT(__ebss <= ORIGIN(DIAG), "Application data exceeds RAM")
ASSERT(ADDR(.probe_status) == 0x20003F00 && SIZEOF(.probe_status) == 128, "Candidate status contract")
ASSERT(ADDR(.host_error) == 0x20003F80 && SIZEOF(.host_error) == 128, "Reserved diagnostic contract")
ASSERT(LOADADDR(.data) + SIZEOF(.data) <= ORIGIN(FLASH) + LENGTH(FLASH), "Flash overflow")

ASSERT(__sdata >= 0x200029A0 && __ebss <= 0x20003F00, "Reserved ROM/shared RAM")
ASSERT(__stack_bottom == 0x20004000 && __stack_top == 0x20007F00, "Reserved stack changed")
ASSERT(ADDR(.vector_table) == 0x10000000, "ROM forwarded vectors moved")
