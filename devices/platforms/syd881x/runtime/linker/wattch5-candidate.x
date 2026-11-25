/* Integrated candidate only: legal application RAM, both full diagnostics preserved.
   Explicit upper BSS 1600 bytes initialized by candidate reset; old linker unchanged. */
ENTRY(Reset)
MEMORY {
 FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 0x3E000
 RAM (rw) : ORIGIN = 0x200029A0, LENGTH = 0x1560
 DIAG (rw) : ORIGIN = 0x20003F00, LENGTH = 0x100
 UPPER (rw) : ORIGIN = 0x20004000, LENGTH = 0x640
 STACK (rw) : ORIGIN = 0x20004640, LENGTH = 0x38C0
}
__stack_bottom = ORIGIN(STACK);
__stack_top = ORIGIN(STACK) + LENGTH(STACK);
SECTIONS {
 .vector_table ORIGIN(FLASH) : { KEEP(*(.vector_table)) } > FLASH
 /* Pack read-only data by alignment so input order adds less padding. */
 .text : { *(.text*) *(SORT_BY_ALIGNMENT(.rodata*)) *(.ARM.extab*) } > FLASH
 .bringup_profile : ALIGN(4) { KEEP(*(.bringup_profile)) } > FLASH
 .board_configuration : ALIGN(4) { KEEP(*(.board_configuration)) } > FLASH
 .candidate_config : ALIGN(4) { KEEP(*(.candidate_config)) } > FLASH
 .candidate_policy : ALIGN(4) { KEEP(*(.candidate_policy)) } > FLASH
 .candidate_manifest : ALIGN(4) { KEEP(*(.candidate_manifest)) } > FLASH
 .ARM.exidx : { *(.ARM.exidx*) } > FLASH
 .data : ALIGN(4) { __sdata = .; *(.data*) . = ALIGN(4); __edata = .; } > RAM AT> FLASH
 __sidata = LOADADDR(.data);
 .upper_static (NOLOAD) : ALIGN(8) { __supper = .; *(.upper_static) *(.bss.*TIME*) . = ALIGN(8); __eupper = .; } > UPPER
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
ASSERT(__stack_bottom == 0x20004640 && __stack_top == 0x20007F00, "Reserved stack changed")
ASSERT(ADDR(.vector_table) == 0x10000000, "ROM forwarded vectors moved")

ASSERT(__eupper <= __stack_bottom && __supper >= 0x20004000, "Candidate upper BSS overlap")
