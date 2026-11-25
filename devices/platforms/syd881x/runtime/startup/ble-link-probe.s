.syntax unified
.cpu cortex-m0
.thumb
.section .vector_table,"a",%progbits
.word __stack_top
.word Reset
.rept 46
.word 0
.endr
.section .text.Reset,"ax",%progbits
.global Reset
.type Reset,%function
.thumb_func
Reset:
    cpsid i
    ldr r0, =__stack_top
    mov sp, r0
    /* No active frame yet: initialize only the reserved probe stack. */
    ldr r0, =0x20007000
    ldr r1, =__stack_top
    ldr r2, =0xa5a5a5a5
1:
    str r2, [r0]
    adds r0, r0, #4
    cmp r0, r1
    blo 1b
    bl reset_rust
    b .
