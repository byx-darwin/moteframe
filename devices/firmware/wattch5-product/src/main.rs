//! Offline-only Cortex-M0 candidate: execute protocol checks, then fail-stop.
#![no_std]
#![no_main]

use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
use wattch5_product::{HELLO_VECTOR, self_check, service_self_check};

global_asm!(include_str!(
    "../../../platforms/syd881x/runtime/startup/wattch5-product.s"
));
#[used]
#[unsafe(link_section = ".candidate_manifest")]
static CAPABILITIES: [u8; include_bytes!(concat!(env!("OUT_DIR"), "/embedded_capabilities.json"))
    .len()] = *include_bytes!(concat!(env!("OUT_DIR"), "/embedded_capabilities.json"));
#[used]
#[unsafe(link_section = ".probe_status")]
static mut STATUS: [u32; 32] = [0; 32];
// Keep the existing native layout's second diagnostic half reserved; no Host exists.
#[used]
#[unsafe(link_section = ".host_error")]
static mut RESERVED: [u8; 128] = [0; 128];

unsafe extern "C" {
    static __sidata: u8;
    static mut __sdata: u8;
    static mut __edata: u8;
    static mut __sbss: u8;
    static mut __ebss: u8;
}

fn publish(index: usize, value: u32) {
    if index < 32 {
        // SAFETY: Only this foreground owns the bounded, aligned status area.
        unsafe {
            write_volatile((&raw mut STATUS).cast::<u32>().add(index), value);
        }
    }
}
fn stop() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Runtime entry deliberately reads the input through volatile memory so the
/// linked self-check exercises codecs rather than embedding a constant result.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn wattch5_protocol_self_check() -> u32 {
    let mut input = [0; 18];
    for (index, byte) in input.iter_mut().enumerate() {
        // SAFETY: Literal vector is immutable valid Flash and index is bounded.
        *byte = unsafe { read_volatile(HELLO_VECTOR.as_ptr().add(index)) };
    }
    match self_check(&input) {
        Ok(report) => {
            let Ok(service_checks) = service_self_check() else {
                return 2;
            };
            publish(2, report.checks_passed);
            publish(3, report.fragments);
            publish(4, report.crc32c_check);
            publish(5, service_checks);
            #[cfg(feature = "wattch5-source")]
            {
                let Ok(source_checks) = wattch5_product::product::source_self_check() else {
                    return 3;
                };
                publish(6, source_checks);
            }
            0
        }
        Err(_) => 1,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn reset_rust() -> ! {
    // SAFETY: Reset exclusively owns the same linker-bounded application ranges;
    // ROM/shared RAM, NOLOAD diagnostics and stack remain outside these bounds.
    unsafe {
        syd881x_runtime::initialize_ram(
            &raw const __sidata,
            &raw mut __sdata,
            &raw mut __edata,
            &raw mut __sbss,
            &raw mut __ebss,
        );
    }
    for index in 0..32 {
        publish(index, 0);
    }
    let result = wattch5_protocol_self_check();
    publish(1, result);
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
    publish(0, 0x5741_5031); // WAP1: checks complete; not a hardware-success tag.
    stop()
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    // SAFETY: Fail-stop only: mask interrupts, no reset/ROM/peripheral operation.
    unsafe {
        asm!("cpsid i", options(nomem, nostack, preserves_flags));
    }
    publish(0, 0);
    publish(1, u32::MAX);
    stop()
}
