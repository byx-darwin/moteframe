// Small C and Arm EABI memory routines for size-constrained GATT builds.
// r12 retains the original destination. No routine calls out or uses the stack.
.syntax unified
.thumb
.section .text.compact_memory, "ax", %progbits
.p2align 1

.macro entry name
    .global \name
    .type \name, %function
    .thumb_func
\name:
.endm

.macro alias name, target
    .global \name
    .thumb_set \name, \target
.endm

// C memcpy(dst, src, count). Use word loads/stores only on aligned addresses.
entry memcpy
    mov r12, r0
.Lforward:
    orr r3, r0, r1
    tst r3, #3
    bne .Lforward_tail
.Lforward_words:
    cmp r2, #4
    blo .Lforward_tail
    ldmia r1!, {r3}
    stmia r0!, {r3}
    subs r2, #4
    b .Lforward_words
.Lforward_tail:
    cbz r2, .Lreturn_dst
.Lforward_bytes:
    ldrb r3, [r1], #1
    strb r3, [r0], #1
    subs r2, #1
    bne .Lforward_bytes
.Lreturn_dst:
    mov r0, r12
    bx lr
.size memcpy, . - memcpy
alias __aeabi_memcpy, memcpy
alias __aeabi_memcpy4, memcpy
alias __aeabi_memcpy8, memcpy

// For a destination inside the source span, copy from the end first.
entry memmove
    mov r12, r0
    subs r3, r0, r1
    beq .Lreturn_dst
    cmp r3, r2
    bhs .Lforward
    add r0, r2
    add r1, r2
    orr r3, r0, r1
    tst r3, #3
    bne .Lbackward_bytes
.Lbackward_words:
    cmp r2, #4
    blo .Lbackward_tail
    ldr r3, [r1, #-4]!
    str r3, [r0, #-4]!
    subs r2, #4
    b .Lbackward_words
.Lbackward_tail:
    cbz r2, .Lreturn_move
.Lbackward_bytes:
    ldrb r3, [r1, #-1]!
    strb r3, [r0, #-1]!
    subs r2, #1
    bne .Lbackward_bytes
.Lreturn_move:
    b .Lreturn_dst
.size memmove, . - memmove
alias __aeabi_memmove, memmove
alias __aeabi_memmove4, memmove
alias __aeabi_memmove8, memmove

// EABI takes (destination, count, [byte]). C memset takes (dst, byte, count).
entry __aeabi_memclr
    mov r2, r1
    movs r1, #0
    b memset
.size __aeabi_memclr, . - __aeabi_memclr
alias __aeabi_memclr4, __aeabi_memclr
alias __aeabi_memclr8, __aeabi_memclr

entry __aeabi_memset
    mov r3, r1
    mov r1, r2
    mov r2, r3
.size __aeabi_memset, . - __aeabi_memset
alias __aeabi_memset4, __aeabi_memset
alias __aeabi_memset8, __aeabi_memset

entry memset
    mov r12, r0
    uxtb r1, r1
    orr r1, r1, r1, lsl #8
    orr r1, r1, r1, lsl #16
.Lalign_fill:
    cbz r2, .Lreturn_fill
    tst r0, #3
    beq .Lfill_words
    strb r1, [r0], #1
    subs r2, #1
    b .Lalign_fill
.Lfill_words:
    cmp r2, #4
    blo .Lfill_tail
    stmia r0!, {r1}
    subs r2, #4
    b .Lfill_words
.Lfill_tail:
    cbz r2, .Lreturn_fill
.Lfill_bytes:
    strb r1, [r0], #1
    subs r2, #1
    bne .Lfill_bytes
.Lreturn_fill:
    b .Lreturn_dst
.size memset, . - memset

.purgem entry
.purgem alias
