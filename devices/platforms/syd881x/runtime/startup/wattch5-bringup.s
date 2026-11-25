.syntax unified
.cpu cortex-m0
.thumb
/* Dedicated future IRQ bring-up template, not the polling lab/self-check table.
   Exact ROM SHA256 1b32ead7d8689857af6986ed7cd790ae425209da1c3989300c424d70749797e9
   forwards SysTick via 0x1000003c and GPIO via 0x10000070. Do not write VTOR
   or overwrite ROM/shared RAM. Other ROM IRQ routes are NOT established here.

   reset_rust must initialize only application RAM, disable/clear all NVIC
   routes, initialize time while masked, and only then unmask interrupts. Leave
   GPIO disabled when polling it; native LL/Timer3/LLC2 stay disabled. Never
   enable interrupts from this Reset stub. Fault_Handler and Default_Handler
   must be supplied by the entry, latch bounded diagnostics, and fail-stop.
   SysTick_Handler must come from the exclusive runtime IRQ clock. Reserved
   slots deliberately target Default_Handler rather than address zero. */
.section .vector_table,"a",%progbits
.global __Vectors
__Vectors:
.word __stack_top
.word Reset
.word Fault_Handler /* NMI */
.word Fault_Handler /* HardFault */
.rept 7
.word Default_Handler
.endr
.word Default_Handler /* SVC */
.rept 2
.word Default_Handler
.endr
.word Default_Handler /* PendSV */
.word SysTick_Handler
.rept 12 /* IRQ0..IRQ11: unsupported/ROM-owned sources remain disabled */
.word Default_Handler
.endr
.word GPIO_IRQHandler /* IRQ12; enable only with the runtime mailbox owner */
.rept 19 /* IRQ13..IRQ31 */
.word Default_Handler
.endr
.global __Vectors_End
__Vectors_End:

.section .text.Reset,"ax",%progbits
.global Reset
.type Reset,%function
.thumb_func
Reset:
    cpsid i
.ifdef WATTCH5_NORMALIZED_RESET
    /* Reject unsupported ROM entry contexts without touching an inherited stack.
       Only privileged Thread entry may normalize MSP/CONTROL and core MMIO. */
    mrs r0, ipsr
    cmp r0, #0
    bne .Lunsupported_entry
    mrs r0, control
    movs r1, #1
    tst r0, r1
    bne .Lunsupported_entry
    ldr r0, =__stack_top
    msr msp, r0
    movs r0, #0
    msr control, r0
    isb
    bl reset_rust
.Lunsupported_entry:
    b .Lunsupported_entry
.else
    ldr r0, =__stack_top
    mov sp, r0
    bl reset_rust
    b .
.endif
.size Reset, .-Reset
