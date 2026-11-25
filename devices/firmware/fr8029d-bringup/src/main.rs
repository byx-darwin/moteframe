//! Standalone, headless FR8029D startup probe.
#![no_std]
#![no_main]

use core::{
    arch::{asm, global_asm},
    panic::PanicInfo,
    ptr::{read_volatile, write_volatile},
    sync::atomic::{Ordering, fence},
};

use fr8029d_bringup::{Snapshot, XIP_ENTRY};

global_asm!(include_str!("startup.s"));

const CPUID: usize = 0xe000_ed00;
const QSPI0_CONFIG: usize = 0xe00a_4000;
const QSPI0_SIZE: usize = 0xe00a_4014;
const QSPI0_REMAP: usize = 0xe00a_4024;
const DIAGNOSTIC_MAGIC: u32 = if cfg!(feature = "ram-probe") {
    u32::from_le_bytes(*b"FR2R")
} else {
    u32::from_le_bytes(*b"FR29")
};
const DIAGNOSTIC_VERSION: u32 = 1;

unsafe extern "C" {
    static mut __diagnostics_start: [u32; 8];
}

#[unsafe(no_mangle)]
extern "C" fn fr8029d_probe_main(boot_vtor: u32) -> ! {
    let diagnostics = (&raw mut __diagnostics_start).cast::<u32>();
    // SAFETY: The linker reserves this aligned SRAM region for the probe, and startup masks IRQs.
    unsafe { write_volatile(diagnostics, 0) };

    // SAFETY: Startup runs privileged on Cortex-M. CPUID is a word-aligned, read-only SCB register.
    let cpuid = unsafe { read_volatile(CPUID as *const u32) };
    let mut snapshot = Snapshot {
        cpuid,
        vtor: boot_vtor,
        qspi_config: 0,
        qspi_size: 0,
        qspi_remap: 0,
    };
    if Snapshot::is_cortex_m33(cpuid) && boot_vtor == XIP_ENTRY {
        // SAFETY: This board-specific image executes after FR8029D established QSPI XIP.
        // These three aligned registers hold configuration and have side-effect-free reads.
        unsafe {
            snapshot.qspi_config = read_volatile(QSPI0_CONFIG as *const u32);
            snapshot.qspi_size = read_volatile(QSPI0_SIZE as *const u32);
            snapshot.qspi_remap = read_volatile(QSPI0_REMAP as *const u32);
        }
    }

    let fields = [
        DIAGNOSTIC_VERSION,
        snapshot.classify() as u32,
        snapshot.cpuid,
        snapshot.vtor,
        snapshot.qspi_config,
        snapshot.qspi_size,
        snapshot.qspi_remap,
    ];
    for (index, field) in fields.into_iter().enumerate() {
        // SAFETY: Indices 1–7 belong to the same aligned, probe-owned diagnostic block.
        unsafe { write_volatile(diagnostics.add(index + 1), field) };
    }
    fence(Ordering::Release);
    // SAFETY: The first word is reserved for the completion marker and has one writer.
    unsafe { write_volatile(diagnostics, DIAGNOSTIC_MAGIC) };
    park()
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    park()
}

fn park() -> ! {
    loop {
        // SAFETY: WFI has no memory operands and leaves the stack and registers unchanged.
        unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) };
    }
}
