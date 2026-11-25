.syntax unified
.cpu cortex-m33
.thumb

.text
.balign 4
.global fr8029d_program_page
.type fr8029d_program_page, %function
.thumb_func
fr8029d_program_page:
    cpsid i
    tst r0, #0xff
    bne .Lreject
    lsrs r2, r0, #12
    beq .Lprogram
.ifdef FR8029D_NOTIFY_XIP_PAGES
    // Deliberately exclude OTA sector 1 and all sectors after the 60 KiB app.
    subs r2, #2
    cmp r2, #14
    bhi .Lreject
.else
    cmp r2, #2
    bne .Lreject
.endif

.Lprogram:
    orr r0, r0, #0x10000000
    movw r1, #0x4000
    movt r1, #0x2000
    movw r2, #256
.Lbyte:
    ldrb r3, [r1], #1
    strb r3, [r0], #1
    subs r2, r2, #1
    bne .Lbyte
    dsb sy
    movs r0, #0
    bkpt #0x29
    b .

.Lreject:
    movs r0, #1
    bkpt #0x2a
    b .
.size fr8029d_program_page, . - fr8029d_program_page
.balign 4
