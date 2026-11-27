// FR8029D QSPI0/P25Q80. All instructions, literals and stack accesses while
// the bus is busy are independent of XIP. r3 is a board-owned SRAM fault exit.
.syntax unified
.thumb
.section .ramfunc.qspi_slots,"ax",%progbits
.p2align 2
.global fr29_slot_command
.type fr29_slot_command,%function
.thumb_func
fr29_slot_command:
    push {r3-r11,lr}
    mov r5,r0
    mov r6,r1
    mov r7,r2
    mov r8,r3
    mrs r9,primask
    cpsid i
    ldr r4,=0xe00a4000
    ldr r10,=0xe00a0000
    movs r0,#0
    mov r11,r0
    mov.w r12,#0x100000
    cmp r5,#256
    bls .Lcache
    lsrs r0,r5,#24
    cmp r0,#0x20
    bne .Ldispatch
.Lcache:
    ldr r0,[r10,#4]
    ands r0,r0,#3
    cmp r0,#0
    beq .Ldispatch
    cmp r0,#2
    beq .Ldisable
    bl .Lbudget
    b .Lcache
.Ldisable:
    mov r11,r0
    movs r0,#0x3c
    str r0,[r10]
    movs r0,#0x38
    str r0,[r10]
.Ldisabled:
    bl .Lbudget
    ldr r0,[r10,#4]
    tst r0,#3
    bne .Ldisabled
.Ldispatch:
    bl .Lidle
    cmp r5,#256
    bls .Lpage
    str r6,[r4,#0x94]
    str r7,[r4,#0xa8]
    mov r0,r5
    bl .Lissue
    ldr r2,[r4,#0xa0]
    b .Lwip
.Lpage:
    cmp r5,#0
    it eq
    moveq.w r5,#256
    orr r0,r6,#0x10000000
    movs r1,#0
.Lbyte:
    ldrb r3,[r7,r1]
    strb r3,[r0,r1]
    adds r1,#1
    cmp r1,r5
    bne .Lbyte
    dsb sy
    bl .Lidle
    movs r2,#0
.Lwip:
    ldr r0,=0x05800000
    bl .Lissue
    ldr r0,[r4,#0xa0]
    tst r0,#1
    bne .Lwip
    cmp r11,#0
    beq .Lreturn
    movs r0,#0x38
    str r0,[r10]
    movs r0,#0x3c
    str r0,[r10]
.Lclocked:
    bl .Lbudget
    ldr r0,[r10,#4]
    tst r0,#0x10
    beq .Lclocked
    movs r0,#0x3e
    str r0,[r10]
.Linvalidate:
    bl .Lbudget
    ldr r0,[r10]
    tst r0,#2
    bne .Linvalidate
    movs r0,#0x3d
    str r0,[r10]
.Lenabled:
    bl .Lbudget
    ldr r0,[r10,#4]
    ands r0,r0,#3
    cmp r0,#2
    bne .Lenabled
.Lreturn:
    dsb sy
    isb sy
    msr primask,r9
    mov r0,r2
    pop {r3-r11,pc}
.Lissue:
    str r0,[r4,#0x90]
    orr r0,r0,#1
    str r0,[r4,#0x90]
.Lidle:
    subs.w r12,r12,#1
    beq .Lstall
    ldr r1,[r4,#0x90]
    tst r1,#3
    bne .Lidle
    ldr r1,[r4]
    cmp r1,#0
    bpl .Lidle
    bx lr
.Lbudget:
    subs.w r12,r12,#1
    beq .Lstall
    bx lr
.Lstall:
    bx r8
.ltorg
.size fr29_slot_command,.-fr29_slot_command
