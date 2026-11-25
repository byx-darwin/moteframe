ENTRY(Reset)
MEMORY {
 FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 0x3E000
 RAM (rw) : ORIGIN = 0x200029A0, LENGTH = 0x4560
 CAPTURE (rw) : ORIGIN = 0x20006000, LENGTH = 0x200
 DIAG (rw) : ORIGIN = 0x20006F00, LENGTH = 0x100
 STACK (rw) : ORIGIN = 0x20007000, LENGTH = 0xF00
}
__stack_top = 0x20007F00;
SECTIONS {
 .vector_table : { KEEP(*(.vector_table)) } > FLASH
 .text : { *(.text*) *(.rodata*) *(.ARM.extab*) } > FLASH
 .ARM.exidx : { *(.ARM.exidx*) } > FLASH
 .data : ALIGN(4) { __sdata = .; *(.data*) . = ALIGN(4); __edata = .; } > RAM AT> FLASH
 __sidata = LOADADDR(.data);
 .bss (NOLOAD) : ALIGN(4) { __sbss = .; *(.bss*) *(COMMON) . = ALIGN(4); __ebss = .; } > RAM
 .rx_capture (NOLOAD) : { KEEP(*(.rx_capture)) } > CAPTURE
 .link_metrics 0x20006200 (NOLOAD) : { KEEP(*(.link_metrics)) } > RAM
 .cpu_metrics 0x20006E00 (NOLOAD) : { KEEP(*(.cpu_metrics)) } > RAM
 .probe_status (NOLOAD) : { KEEP(*(.probe_status)) } > DIAG
 .stack (NOLOAD) : { . = . + 0xF00; } > STACK
 /DISCARD/ : { *(.comment) }
}
ASSERT(SIZEOF(.vector_table) == 192, "48 vectors required")
ASSERT(__ebss <= 0x20006000, "Application data exceeds scratch area")
