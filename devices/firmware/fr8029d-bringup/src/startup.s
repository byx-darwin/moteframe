.syntax unified
.cpu cortex-m33
.thumb

.ifndef __fr29_use_freertos
.set __fr29_use_freertos, 0
.endif

.section .vectors, "a", %progbits
.balign 512
.global __vector_table
__vector_table:
    .word __stack_top
    .word Reset_Handler
    .rept 5                      // NMI through UsageFault
        .word Default_Handler
    .endr
    .word Default_Handler       // SecureFault (when implemented)
    .rept 3                      // Reserved exceptions 8–10
        .word 0
    .endr
    .if __fr29_use_freertos
    .word SVC_Handler
    .else
    .word Default_Handler       // SVCall
    .endif
    .word Default_Handler       // DebugMonitor
    .word 0                     // Reserved exception 13
    .if __fr29_use_freertos
    .word PendSV_Handler
    .word SysTick_Handler
    .else
    .word Default_Handler       // PendSV
    .word Default_Handler       // SysTick
    .endif
    .rept 64                     // Covers FR802x device IRQs 0–49
        .word Default_Handler
    .endr

.section .text.Reset_Handler, "ax", %progbits
.global Reset_Handler
.type Reset_Handler, %function
.thumb_func
Reset_Handler:
    cpsid i
    ldr r1, =0xe000ed08
    ldr r0, [r1]               // Preserve the ROM-selected VTOR for diagnostics.
    ldr r2, =__vector_table
    str r2, [r1]
    dsb sy
    isb sy

    ldr r1, =__sidata
    ldr r2, =__sdata
    ldr r3, =__edata
.Lcopy_data:
    cmp r2, r3
    bhs .Lstart_bss
    ldr r12, [r1], #4
    str r12, [r2], #4
    b .Lcopy_data

.Lstart_bss:
    ldr r2, =__sbss
    ldr r3, =__ebss
    movs r1, #0
.Lzero_bss:
    cmp r2, r3
    bhs .Lenter_rust
    str r1, [r2], #4
    b .Lzero_bss

.Lenter_rust:
    b fr8029d_probe_main
.size Reset_Handler, . - Reset_Handler
.ltorg

.section .text.Default_Handler, "ax", %progbits
.global Default_Handler
.type Default_Handler, %function
.thumb_func
Default_Handler:
    cpsid i
.Ldefault_wait:
    wfi
    b .Ldefault_wait
.size Default_Handler, . - Default_Handler
