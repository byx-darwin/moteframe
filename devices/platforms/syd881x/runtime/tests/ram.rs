//! Startup must touch only the linker-selected application ranges.
#![allow(
    unsafe_code,
    reason = "Tests supply exclusive live arrays to the unsafe reset boundary"
)]
use syd881x_runtime::initialize_ram;
#[test]
fn test_should_copy_data_and_zero_bss_without_touching_neighbor_bytes() {
    let source = [1_u8, 2, 3, 4];
    let mut data = [0xa5_u8; 8];
    let mut bss = [0x5a_u8; 8];
    // SAFETY: Every selected range belongs to its own live disjoint array, with four source bytes.
    unsafe {
        initialize_ram(
            source.as_ptr(),
            data.as_mut_ptr().add(2),
            data.as_mut_ptr().add(6),
            bss.as_mut_ptr().add(1),
            bss.as_mut_ptr().add(5),
        );
    }
    assert_eq!(data, [0xa5, 0xa5, 1, 2, 3, 4, 0xa5, 0xa5]);
    assert_eq!(bss, [0x5a, 0, 0, 0, 0, 0x5a, 0x5a, 0x5a]);
}
#[test]
fn test_should_allow_empty_regions_without_dereferencing_them() {
    // SAFETY: Both ranges are empty; the function promises not to dereference their pointers.
    unsafe {
        initialize_ram(
            core::ptr::null(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
    }
}
