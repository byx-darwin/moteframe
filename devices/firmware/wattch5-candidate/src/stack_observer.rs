//! Opt-in laboratory observation of real SysTick entry; no artificial interrupts.
use core::arch::naked_asm;

/// Observe only the reserved diagnostic words, preserving exception LR and SP.
/// # Safety
/// Enter only through the pinned zero-stack ROM SysTick forwarder, after reset
/// initialized STATUS. Thread mode uses MSP; r0-r3 are hardware-stacked. The
/// tail target is the unchanged runtime handler, never another observer.
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn SysTick_Handler() {
    // SAFETY: This exact naked sequence uses only hardware-stacked r0-r3, never
    // pushes or changes SP/LR, and touches only STATUS[26..31]. Hardware frame
    // PC/xPSR offsets are fixed even when xPSR bit9 records alignment padding.
    naked_asm!(
        "mrs r0, msp",
        "ldr r1, ={status}",
        "ldr r2, [r1, #104]",
        "adds r2, #1",
        "str r2, [r1, #104]",
        "ldr r2, [r1, #108]",
        "cmp r2, #0",
        "beq 2f",
        "cmp r0, r2",
        "bhs 3f",
        "2:",
        "str r0, [r1, #108]",
        "ldr r2, [r0, #24]",
        "str r2, [r1, #112]",
        "ldr r2, [r0, #28]",
        "str r2, [r1, #116]",
        "3:",
        "mrs r2, ipsr",
        "str r2, [r1, #120]",
        "ldr r3, ={clock}",
        "bx r3",
        status = sym super::STATUS,
        clock = sym syd881x_runtime::SysTick_Handler,
    );
}
