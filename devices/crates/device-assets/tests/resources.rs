//! Bounds, raw byte order and odd nibble spans independent of vendor resources.
use device_assets::{Bitmap, Format, Memory, ReadAt, Region};
use device_core::Rgb565;
#[test]
fn test_should_read_exact_regions_and_reject_overflow_before_io() {
    let mut source = Memory::new(&[1, 2, 3, 4]).unwrap_or_else(|e| panic!("{e:?}"));
    let r = Region::new(1, 3, source.capacity()).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 2];
    r.read(&mut source, 1, &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, [3, 4]);
    assert!(r.read(&mut source, 3, &mut out).is_err());
    assert!(Region::new(u32::MAX, 2, u32::MAX).is_err());
    assert!(r.read(&mut source, 3, &mut []).is_ok());
}
#[test]
fn test_should_decode_endian_and_odd_indexed_spans_without_row_leakage() {
    let mut source =
        Memory::new(&[0x12, 0x34, 0xab, 0xcd, 0, 0]).unwrap_or_else(|e| panic!("{e:?}"));
    let region = Region::new(0, 4, 6).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [Rgb565(0); 2];
    let bmp = Bitmap::new(2, 1, region, Format::Rgb565Be).unwrap_or_else(|e| panic!("{e:?}"));
    bmp.read_span(&mut source, 0, 0, &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, [Rgb565(0x1234), Rgb565(0xabcd)]);
    let bmp = Bitmap::new(2, 1, region, Format::Rgb565Le).unwrap_or_else(|e| panic!("{e:?}"));
    bmp.read_span(&mut source, 0, 0, &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, [Rgb565(0x3412), Rgb565(0xcdab)]);
    let mut palette = [Rgb565(0); 16];
    for (i, p) in palette.iter_mut().enumerate() {
        *p = Rgb565(u16::try_from(i).unwrap_or(0));
    }
    let indexed = Bitmap::new(
        3,
        2,
        Region::new(0, 3, 6).unwrap_or_else(|e| panic!("{e:?}")),
        Format::Indexed4(palette),
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    indexed
        .read_span(&mut source, 0, 1, &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, [Rgb565(4), Rgb565(10)]);
    assert!(indexed.read_span(&mut source, 2, 1, &mut out).is_err());
    assert!(indexed.read_span(&mut source, 0, 2, &mut out).is_err());
    assert!(Bitmap::new(100, 100, region, Format::Rgb565Be).is_err());
}
