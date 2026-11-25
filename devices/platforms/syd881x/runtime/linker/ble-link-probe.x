ENTRY(Reset)
MEMORY {
    CODE (rwx) : ORIGIN = 0x20003000, LENGTH = 0x3B00
    LINK (rw) : ORIGIN = 0x20006D00, LENGTH = 0x80
    CAPTURE (rw) : ORIGIN = 0x20006B00, LENGTH = 0x200
    DIAG (rw) : ORIGIN = 0x20006F00, LENGTH = 0x100
    STACK (rw) : ORIGIN = 0x20007000, LENGTH = 0xF00
}
__stack_top = 0x20007F00;
PHDRS {
    vectors PT_LOAD FLAGS(4);
    code PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
    capture PT_LOAD FLAGS(6);
    diagnostics PT_LOAD FLAGS(6);
    stack PT_LOAD FLAGS(6);
}
SECTIONS {
    .vector_table : { KEEP(*(.vector_table)) } > CODE :vectors
    .text : { *(.text*) *(.rodata*) *(.ARM.extab*) } > CODE :code
    .ARM.exidx : { *(.ARM.exidx*) } > CODE :code
    .data : ALIGN(4) { *(.data*) } > CODE :data
    .bss (NOLOAD) : ALIGN(4) {
        __sbss = .; *(.bss*) *(COMMON) . = ALIGN(4); __ebss = .;
    } > CODE :data
    .rx_capture : { KEEP(*(.rx_capture)) } > CAPTURE :capture
    .link_metrics (NOLOAD) : { KEEP(*(.link_metrics)) } > LINK :diagnostics
    .probe_status : { KEEP(*(.probe_status)) } > DIAG :diagnostics
    .stack (NOLOAD) : { . = . + 0xF00; } > STACK :stack
    /DISCARD/ : { *(.comment) }
}
ASSERT(__ebss <= 0x20006B00, "Probe overlaps diagnostics")
ASSERT(SIZEOF(.probe_status) <= 0x100, "Diagnostics exceed reserved area")
