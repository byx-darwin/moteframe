ENTRY(compare_entry)
SECTIONS {
 .text 0x20003100 : { *(.text.compare_entry) *(.text .text.*) *(.rodata .rodata.*) }
 .data 0x20003900 : { *(.data .data.*) }
 .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) }
 .compare_status 0x20003a00 (NOLOAD) : { KEEP(*(.compare_status)) }
 /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) }
}
ASSERT(ADDR(.text)+SIZEOF(.text)<=0x20003900,"RAM probe text overflow")
ASSERT(ADDR(.bss)+SIZEOF(.bss)<=0x20003a00,"RAM probe data overflow")
ASSERT(SIZEOF(.compare_status)<=0x600,"RAM probe status overflow")
