//! Source8-byte record adapter, independently generated headers and palette values.
use device_assets::Memory;
use device_core::Rgb565;
use wattch5_ui_demo::open_legacy;
#[test]
fn test_should_load_raw_or_local_palette_records_and_reject_unknown_encodings() {
    let raw = [0, 0, 0, 2, 0, 1, 0, 0, 0x12, 0x34, 0xab, 0xcd];
    let mut source = Memory::new(&raw).unwrap_or_else(|e| panic!("{e:?}"));
    let bmp = open_legacy(&mut source, 0).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [Rgb565(0); 2];
    bmp.read_span(&mut source, 0, 0, &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, [Rgb565(0x1234), Rgb565(0xabcd)]);
    let mut data = vec![127, 0, 0, 3, 0, 2, 0, 0];
    for i in 0..16_u16 {
        data.extend_from_slice(&(i * 257).to_be_bytes());
    }
    data.extend_from_slice(&[0x12, 0x34, 0x56]);
    let mut source = Memory::new(&data).unwrap_or_else(|e| panic!("{e:?}"));
    let bmp = open_legacy(&mut source, 0).unwrap_or_else(|e| panic!("{e:?}"));
    bmp.read_span(&mut source, 0, 1, &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, [Rgb565(4 * 257), Rgb565(5 * 257)]);
    for bytes in [
        &[126, 0, 0, 1, 0, 1, 0, 0][..],
        &raw[..9],
        &[0, 0, 0, 0, 0, 1, 0, 0][..],
    ] {
        let mut source = Memory::new(bytes).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(open_legacy(&mut source, 0).is_err());
    }
}
