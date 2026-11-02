//! Hand-derived pixels, bounds, first failure and caller scratch behavior.
#![allow(clippy::unwrap_used)]
use embedded_graphics_core::pixelcolor::{BinaryColor, Gray8, GrayColor, Rgb565, Rgb888, RgbColor};
use moteframe_assets::{
    AssetError, DecodeError, GeometryError, Glyph, GlyphMetrics, Gray8Bytes, Mono8, Raster,
    Rgb565Be, Rgb565Le, Rgb888Bytes,
};
use moteframe_core::ReadAt;
use moteframe_storage::SliceSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ink(u8);
struct InkFormat;
impl moteframe_assets::PixelFormat for InkFormat {
    type Color = Ink;
    const BYTES_PER_PIXEL: usize = 1;
    fn decode(bytes: &[u8]) -> Result<Ink, DecodeError> {
        let [byte] = bytes else {
            return Err(DecodeError::InvalidLength);
        };
        Ok(Ink(*byte))
    }
}
struct ZeroFormat;
impl moteframe_assets::PixelFormat for ZeroFormat {
    type Color = Ink;
    const BYTES_PER_PIXEL: usize = 0;
    fn decode(_: &[u8]) -> Result<Ink, DecodeError> {
        Err(DecodeError::InvalidLength)
    }
}

#[test]
fn custom_nondefault_color_and_dyn_reader_work_without_graphics_or_screen_traits() {
    let mut source = SliceSource::new(&[7, 9]).unwrap();
    let dynamic: &mut dyn ReadAt<Error = moteframe_storage::RangeError> = &mut source;
    let mut output = [Ink(0); 2];
    Raster::<InkFormat>::new(0, 2, 1)
        .unwrap()
        .read_span(dynamic, 0, 0, &mut output, &mut [0; 1])
        .unwrap();
    assert_eq!(output, [Ink(7), Ink(9)]);
    assert_eq!(
        Raster::<ZeroFormat>::new(0, 1, 1).unwrap_err(),
        GeometryError::InvalidFormat
    );
}

#[test]
fn built_in_decoders_reject_inexact_pixel_lengths() {
    use moteframe_assets::PixelFormat;
    assert_eq!(Rgb565Be::decode(&[0]), Err(DecodeError::InvalidLength));
    assert_eq!(
        Rgb565Le::decode(&[0, 0, 0]),
        Err(DecodeError::InvalidLength)
    );
    assert_eq!(
        Rgb888Bytes::decode(&[0, 0]),
        Err(DecodeError::InvalidLength)
    );
    assert_eq!(Gray8Bytes::decode(&[]), Err(DecodeError::InvalidLength));
    assert_eq!(Mono8::decode(&[0, 1]), Err(DecodeError::InvalidLength));
}

#[test]
fn colors_decode_identically_across_scratch_sizes_and_byte_orders() {
    for size in [2, 3, 4, 7, 16] {
        let mut scratch = [0; 16];
        let mut pixels = [Rgb565::BLACK; 3];
        let raster = Raster::<Rgb565Be>::new(1, 3, 1).unwrap();
        let mut source = SliceSource::new(&[99, 0xf8, 0, 0x07, 0xe0, 0, 0x1f]).unwrap();
        raster
            .read_span(&mut source, 0, 0, &mut pixels, &mut scratch[..size])
            .unwrap();
        assert_eq!(pixels, [Rgb565::RED, Rgb565::GREEN, Rgb565::BLUE]);
        let mut source = SliceSource::new(&[0, 0xf8, 0xe0, 7, 0x1f, 0]).unwrap();
        Raster::<Rgb565Le>::new(0, 3, 1)
            .unwrap()
            .read_span(&mut source, 0, 0, &mut pixels, &mut scratch[..size])
            .unwrap();
        assert_eq!(pixels, [Rgb565::RED, Rgb565::GREEN, Rgb565::BLUE]);
    }
}

#[test]
fn native_gray_rgb888_and_monochrome_are_not_forced_through_rgb565() {
    let mut source = SliceSource::new(&[10, 20, 30, 90, 80, 70]).unwrap();
    let mut rgb = [Rgb888::BLACK; 2];
    Raster::<Rgb888Bytes>::new(0, 2, 1)
        .unwrap()
        .read_span(&mut source, 0, 0, &mut rgb, &mut [0; 3])
        .unwrap();
    assert_eq!(rgb, [Rgb888::new(10, 20, 30), Rgb888::new(90, 80, 70)]);
    let mut gray = [Gray8::BLACK; 2];
    Raster::<Gray8Bytes>::new(0, 3, 2)
        .unwrap()
        .read_span(&mut source, 1, 1, &mut gray, &mut [0; 1])
        .unwrap();
    assert_eq!([gray[0].luma(), gray[1].luma()], [80, 70]);
    let mut source = SliceSource::new(&[1, 0, 1]).unwrap();
    let mut mono = [BinaryColor::Off; 3];
    Raster::<Mono8>::new(0, 3, 1)
        .unwrap()
        .read_span(&mut source, 0, 0, &mut mono, &mut [0; 2])
        .unwrap();
    assert_eq!(mono, [BinaryColor::On, BinaryColor::Off, BinaryColor::On]);
}

struct Source {
    bytes: [u8; 6],
    capacity: u32,
    reads: usize,
    fail: Option<usize>,
}
impl ReadAt for Source {
    type Error = u8;
    fn capacity(&self) -> u32 {
        self.capacity
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), u8> {
        self.reads += 1;
        if self.fail == Some(self.reads) {
            out[0] = 0xff;
            return Err(47);
        }
        out.copy_from_slice(&self.bytes[offset as usize..offset as usize + out.len()]);
        Ok(())
    }
}
fn source() -> Source {
    Source {
        bytes: [0xf8, 0, 7, 0xe0, 0, 0x1f],
        capacity: 6,
        reads: 0,
        fail: None,
    }
}

#[test]
fn geometry_source_extent_and_scratch_are_checked_before_io() {
    assert_eq!(
        Raster::<Rgb565Be>::new(0, 0, 1).unwrap_err(),
        GeometryError::Empty
    );
    assert_eq!(
        Raster::<Rgb565Be>::new(u32::MAX, 1, 1).unwrap_err(),
        GeometryError::Overflow
    );
    assert_eq!(
        Raster::<Rgb565Be>::new(0, u16::MAX, u16::MAX).unwrap_err(),
        GeometryError::Overflow
    );
    let raster = Raster::<Rgb565Be>::new(0, 3, 1).unwrap();
    let mut source = source();
    let mut out = [Rgb565::WHITE; 2];
    assert_eq!(
        raster.read_span(&mut source, 2, 0, &mut out, &mut [0; 4]),
        Err(AssetError::Bounds)
    );
    assert_eq!(
        raster.read_span(&mut source, 0, 1, &mut out, &mut [0; 4]),
        Err(AssetError::Bounds)
    );
    assert_eq!(
        raster.read_span(&mut source, 0, 0, &mut [], &mut [0; 4]),
        Err(AssetError::Bounds)
    );
    assert_eq!(
        raster.read_span(&mut source, 0, 0, &mut out, &mut [0; 1]),
        Err(AssetError::ScratchTooSmall)
    );
    source.capacity = 5;
    assert_eq!(
        raster.read_span(&mut source, 0, 0, &mut out, &mut [0; 4]),
        Err(AssetError::Bounds)
    );
    assert_eq!(source.reads, 0);
    assert_eq!(out, [Rgb565::WHITE; 2]);
}

#[test]
fn first_backend_or_decode_failure_stops_without_retry() {
    let mut source = source();
    source.fail = Some(2);
    let mut out = [Rgb565::WHITE; 3];
    assert_eq!(
        Raster::<Rgb565Be>::new(0, 3, 1).unwrap().read_span(
            &mut source,
            0,
            0,
            &mut out,
            &mut [0; 2]
        ),
        Err(AssetError::Source(47))
    );
    assert_eq!(source.reads, 2);
    assert_eq!(out[0], Rgb565::RED);
    assert_eq!(out[1], Rgb565::WHITE);
    let mut source = SliceSource::new(&[1, 2, 0]).unwrap();
    assert_eq!(
        Raster::<Mono8>::new(0, 3, 1).unwrap().read_span(
            &mut source,
            0,
            0,
            &mut [BinaryColor::Off; 3],
            &mut [0; 1]
        ),
        Err(AssetError::Decode(DecodeError::InvalidValue))
    );
}

#[test]
fn glyph_reuses_source_backed_raster_without_font_or_pixel_storage() {
    let glyph = Glyph::new(
        Raster::<Mono8>::new(0, 2, 1).unwrap(),
        GlyphMetrics {
            bearing_x: -2,
            bearing_y: 3,
            advance: 4,
        },
    );
    assert_eq!(glyph.metrics().bearing_x, -2);
    assert_eq!(glyph.metrics().advance, 4);
    let mut source = SliceSource::new(&[1, 0]).unwrap();
    let mut out = [BinaryColor::Off; 2];
    glyph
        .raster()
        .read_span(&mut source, 0, 0, &mut out, &mut [0; 1])
        .unwrap();
    assert_eq!(out, [BinaryColor::On, BinaryColor::Off]);
}

#[test]
fn decoder_failure_stops_before_reading_the_next_pixel() {
    let mut source = Source {
        bytes: [1, 2, 0, 0, 0, 0],
        capacity: 6,
        reads: 0,
        fail: None,
    };
    let mut output = [BinaryColor::Off; 3];
    assert_eq!(
        Raster::<Mono8>::new(0, 3, 1).unwrap().read_span(
            &mut source,
            0,
            0,
            &mut output,
            &mut [0; 1]
        ),
        Err(AssetError::Decode(DecodeError::InvalidValue))
    );
    assert_eq!(source.reads, 2);
    assert_eq!(
        output,
        [BinaryColor::On, BinaryColor::Off, BinaryColor::Off]
    );
    // Partial prefix is observable, but the public contract invalidates this whole output.
}
