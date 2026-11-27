//! Slot geometry tests use physical offsets, including legal end-of-chip access.
#![cfg(feature = "qspi-config")]
use fr8029d_bringup::qspi_config::Partition;
use moteframe_storage::write::Slot;
#[test]
fn validates_the_whole_erase_footprint() {
    let reserved = [0x44000..0x45000, 0x90000..0xa0000];
    for base in [
        0,
        0x1f000,
        0x20001,
        0x43000,
        0x44000,
        0x8f000,
        0xff000,
        u32::MAX,
    ] {
        assert!(
            Partition::new(base, 0x20000, &reserved).is_err(),
            "{base:#x}"
        );
    }
    let p = Partition::new(0xfe000, 0x20000, &reserved).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(p.address(Slot::A, 0, 4096), Ok(0xfe000));
    assert_eq!(p.address(Slot::B, 4095, 1), Ok(0xfffff));
    assert!(p.address(Slot::A, 4095, 2).is_err());
    assert!(p.address(Slot::B, usize::MAX, 1).is_err());
}

#[test]
fn touching_reserved_ranges_are_allowed_but_one_byte_overlaps_are_rejected() {
    assert!(Partition::new(0x60000, 0x50000, &[0x50000..0x60000, 0x62000..0xe0000]).is_ok());
    for invalid in [0x50000..0x60001, 0x61fff..0xe0000, 0x80000..0x0010_0001] {
        assert!(Partition::new(0x60000, 0x50000, core::slice::from_ref(&invalid)).is_err());
    }
}

#[test]
fn product_layout_uses_the_two_sectors_between_flashenv_and_ota() {
    let region = Partition::layout_v1();
    assert_eq!(region.address(Slot::A, 0, 4096), Ok(0x40000));
    assert_eq!(region.address(Slot::B, 0, 4096), Ok(0x41000));
}
