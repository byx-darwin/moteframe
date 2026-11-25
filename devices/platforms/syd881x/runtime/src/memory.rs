//! Linker-selected application memory initialization, excluding all reserved regions.
#![allow(
    unsafe_code,
    reason = "Reset is the unsafe linker-to-Rust memory initialization boundary"
)]
use core::ptr::{read_volatile, write_volatile};
/// Copies initialized application data and clears only the selected BSS range.
/// Empty ranges are permitted and never dereferenced.
/// # Safety
/// Caller holds exclusive reset execution with interrupts masked. Both destination
/// ranges must be valid writable application memory, nonoverlapping, and ordered
/// start<=end within their allocations. Source must provide the full data length
/// and not overlap destinations. Do not include ROM/shared RAM, NOLOAD diagnostics
/// or stack in either range. Nonempty regions require valid initialized pointers.
#[inline]
pub unsafe fn initialize_ram(
    mut source: *const u8,
    mut data: *mut u8,
    data_end: *mut u8,
    mut bss: *mut u8,
    bss_end: *mut u8,
) {
    while data.addr() < data_end.addr() {
        // SAFETY: The caller guarantees valid disjoint source/data ranges of matching length.
        unsafe {
            write_volatile(data, read_volatile(source));
            source = source.add(1);
            data = data.add(1);
        }
    }
    while bss.addr() < bss_end.addr() {
        // SAFETY: The caller exclusively owns the linker-selected writable BSS range.
        unsafe {
            write_volatile(bss, 0);
            bss = bss.add(1);
        }
    }
}
