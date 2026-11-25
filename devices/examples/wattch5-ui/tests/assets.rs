//! Presentation behavior against independent literal fixtures.
use wattch5_ui_demo::{AssetId, Bundle};
fn fixture() -> Vec<u8> {
    let head = 10 + 22 * 12;
    let mut bytes = vec![0; head + 22 * 4];
    bytes[..8].copy_from_slice(b"W5UI\x01\0\0\0");
    bytes[8..10].copy_from_slice(&22_u16.to_be_bytes());
    for index in 0..22 {
        let count = 10 + index * 12;
        bytes[count..count + 2].copy_from_slice(&1_u16.to_be_bytes());
        bytes[count + 2..count + 4].copy_from_slice(&2_u16.to_be_bytes());
        bytes[count + 4..count + 8].copy_from_slice(
            &u32::try_from(head + index * 4)
                .unwrap_or_else(|e| panic!("{e:?}"))
                .to_be_bytes(),
        );
        bytes[count + 8..count + 12].copy_from_slice(&4_u32.to_be_bytes());
        bytes[head + index * 4..head + index * 4 + 4].copy_from_slice(&[0x11, 0x11, 0x22, 0x22]);
    }
    bytes
}
#[test]
fn test_should_validate_bundle_and_slice_only_complete_strip_frames() {
    let bytes = fixture();
    let a = Bundle::new(&bytes).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        a.frame(AssetId::DialDigits, 1, 1, 1)
            .unwrap_or_else(|e| panic!("{e:?}")),
        &[0x22, 0x22]
    );
    assert!(a.frame(AssetId::DialDigits, 2, 1, 0).is_err());
    assert!(a.frame(AssetId::DialDigits, 1, 0, 0).is_err());
    assert!(a.frame(AssetId::DialDigits, 1, 1, 2).is_err());
    assert!(Bundle::new(&bytes[..bytes.len() - 1]).is_err());
    let mut bad = bytes;
    bad[14..18].copy_from_slice(&0_u32.to_be_bytes());
    assert!(Bundle::new(&bad).is_err());
}
