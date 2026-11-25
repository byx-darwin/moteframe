
.syntax unified
.cpu cortex-m0
.thumb
.section .vector_table,"a",%progbits
.word __stack_top
.word Reset
.rept 46
.word Fault_Handler
.endr
.section .text.Reset,"ax",%progbits
.global Reset
.type Reset,%function
.thumb_func
Reset:
 cpsid i
 ldr r0, =__stack_top
 mov sp, r0
 bl copy_payload
 b .
