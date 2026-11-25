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
 .vector_table : { KEEP(*(.vector_table)) } > FLASH
 .text : { *(.text*) *(.rodata*) *(.ARM.extab*) } > FLASH
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
ASSERT(SIZEOF(.probe_status) <= LENGTH(DIAG), "Diagnostics overflow")

ASSERT(ADDR(.host_error) == 0x20003F80, "Host error capture address changed")
ASSERT(SIZEOF(.host_error) == 128, "Host error capture size changed")
