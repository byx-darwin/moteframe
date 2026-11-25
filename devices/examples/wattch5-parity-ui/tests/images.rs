#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "Host fixture assertions fail immediately on invalid generated resources."
)]
//! Real image loader over a bounded byte source, with actual V2 pixels as oracle.
#![cfg(feature = "asset-build")]
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
use wattch5_parity_ui_demo::*;
#[derive(Debug)]
struct Frame(Vec<u16>);
impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(128, 128)
    }
}
impl DrawTarget for Frame {
    type Color = Rgb565;
    type Error = ();
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, i: I) -> Result<(), ()> {
        for Pixel(p, c) in i {
            self.0[usize::try_from(p.y).map_err(|_| ())? * 128
                + usize::try_from(p.x).map_err(|_| ())?] = c.into_storage();
        }
        Ok(())
    }
}
#[derive(Debug)]
struct Bytes(Vec<u8>);
impl moteframe_core::ReadAt for Bytes {
    type Error = ();
    fn capacity(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap()
    }
    fn read_exact(&mut self, o: u32, out: &mut [u8]) -> Result<(), ()> {
        assert!(out.len() <= 64);
        out.copy_from_slice(
            self.0
                .get(usize::try_from(o).unwrap()..usize::try_from(o).unwrap() + out.len())
                .ok_or(())?,
        );
        Ok(())
    }
}

#[derive(Debug)]
struct PackedBytes {
    bytes: Vec<u8>,
    reads: std::rc::Rc<core::cell::RefCell<Vec<(u32, usize)>>>,
    fail: std::rc::Rc<core::cell::Cell<bool>>,
}
impl moteframe_core::ReadAt for PackedBytes {
    type Error = ();
    fn capacity(&self) -> u32 {
        u32::try_from(self.bytes.len()).unwrap()
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), ()> {
        self.reads.borrow_mut().push((offset, out.len()));
        if self.fail.replace(false) {
            if let Some(first) = out.first_mut() {
                *first = 255;
            }
            return Err(());
        }
        out.copy_from_slice(
            self.bytes
                .get(offset as usize..offset as usize + out.len())
                .ok_or(())?,
        );
        Ok(())
    }
}
fn packed_fixture() -> PackedBytes {
    let mut bytes = vec![0; 8960 + 4096];
    bytes[..8].copy_from_slice(b"W5UIV202");
    for (i, value) in [2u32, 2232, 8960, 8960, 13056, 1].into_iter().enumerate() {
        bytes[8 + i * 4..12 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    // Default Chinese STACK dial is directory slot36. Every byte stores 0,1,2,3.
    bytes[32 + 36 * 4..36 + 36 * 4].copy_from_slice(&8960u32.to_le_bytes());
    bytes[8960..].fill(0x1b);
    PackedBytes {
        bytes,
        reads: std::rc::Rc::default(),
        fail: std::rc::Rc::default(),
    }
}

#[test]
fn test_should_decode_packed_rows_with_one_data_read_and_reuse_only_valid_scene_identity() {
    let source = packed_fixture();
    let reads = source.reads.clone();
    let assets = ImageAssets::open(source).unwrap();
    reads.borrow_mut().clear();
    let mut model = Model::default();
    let mut row = [Rgb565::new(0, 0, 0); 128];
    assets.row(&model, 0, &mut row).unwrap();
    assert_eq!(&*reads.borrow(), &[(176, 4), (8960, 32)]);
    assert_eq!(
        row[..4]
            .iter()
            .map(|c| c.into_storage())
            .collect::<Vec<_>>(),
        [129, 57177, 48941, 36111]
    );
    reads.borrow_mut().clear();
    assets.row(&model, 127, &mut row).unwrap();
    assert_eq!(&*reads.borrow(), &[(13024, 32)]);
    // Theme changes reuse encoded identity, but must remap every color.
    let mut appearance = model.appearance();
    appearance.theme = Theme::Amber;
    model.set_appearance(appearance).unwrap();
    assets.row(&model, 0, &mut row).unwrap();
    assert_eq!(
        row[..4]
            .iter()
            .map(|c| c.into_storage())
            .collect::<Vec<_>>(),
        [6272, 63190, 65035, 46286]
    );
    // A different screen has no image. It must not display the cached dial.
    model
        .preview(Screen::Heart, &Snapshot::unavailable())
        .unwrap();
    assert!(assets.row(&model, 0, &mut row).is_err());
}

#[test]
fn test_should_preserve_pixels_after_failed_packed_read_and_reject_bad_geometry() {
    let source = packed_fixture();
    let fail = source.fail.clone();
    let assets = ImageAssets::open(source).unwrap();
    let sentinel = Rgb565::new(1, 2, 3);
    let mut row = [sentinel; 128];
    fail.set(true);
    assert!(assets.row(&Model::default(), 0, &mut row).is_err());
    assert!(row.iter().all(|p| *p == sentinel));
    assets.row(&Model::default(), 0, &mut row).unwrap();
    fail.set(true);
    row.fill(sentinel);
    assert!(assets.row(&Model::default(), 1, &mut row).is_err());
    assert!(row.iter().all(|p| *p == sentinel));
    for (offset, value) in [(8, 1u32), (20, 9000), (24, 13055), (28, u32::MAX)] {
        let mut source = packed_fixture();
        source.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(ImageAssets::open(source).is_err());
    }
    for value in [8961u32, 13056, u32::MAX] {
        let mut source = packed_fixture();
        source.bytes[176..180].copy_from_slice(&value.to_le_bytes());
        let assets = ImageAssets::open(source).unwrap();
        row.fill(sentinel);
        assert!(matches!(
            assets.row(&Model::default(), 0, &mut row),
            Err(ImageError::Format)
        ));
        assert!(row.iter().all(|p| *p == sentinel));
    }
    assert!(assets.row(&Model::default(), 128, &mut row).is_err());
}
#[test]
#[ignore = "requires generated full private V2 image pack"]
fn test_should_load_all_bilingual_background_variants_and_remap_all_three_palettes() {
    let path = std::env::var_os("WATTCH5_TEST_IMAGES").expect("generated pack required");
    let pack = std::fs::read(path).unwrap();
    let assets = ImageAssets::open(Bytes(pack)).unwrap();
    for page in Screen::ALL {
        for locale in [Locale::Zh, Locale::En] {
            for v in 0..Model::background_variants(page) {
                for theme in [Theme::Phosphor, Theme::Amber, Theme::Ice] {
                    let mut m = Model::background_model(page, locale, v).unwrap();
                    let mut a = m.appearance();
                    a.theme = theme;
                    m.set_appearance(a).unwrap();
                    let mut reference = Frame(vec![0; 16384]);
                    m.draw_background(&mut reference).unwrap();
                    let mut row = [Rgb565::new(0, 0, 0); 128];
                    let mut got = Vec::new();
                    for y in 0..128 {
                        assets.row(&m, y, &mut row).unwrap();
                        got.extend(row.map(IntoStorage::into_storage));
                    }
                    assert_eq!(got, reference.0, "{page:?} {locale:?} {v} {theme:?}");
                }
            }
        }
    }
}
#[test]
fn test_should_reject_foreign_catalogue_before_any_pixels_are_published() {
    assert!(ImageAssets::open(Bytes(vec![0; 32])).is_err());
}

#[derive(Debug)]
struct Row<'a> {
    y: u16,
    pixels: &'a mut [Rgb565; 128],
}
impl Dimensions for Row<'_> {
    fn bounding_box(&self) -> device_ui::graphics::primitives::Rectangle {
        device_ui::graphics::primitives::Rectangle::new(
            Point::new(0, i32::from(self.y)),
            Size::new(128, 1),
        )
    }
}
impl DrawTarget for Row<'_> {
    type Color = Rgb565;
    type Error = ();
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, p: I) -> Result<(), ()> {
        for Pixel(at, c) in p {
            assert_eq!(at.y, i32::from(self.y));
            self.pixels[usize::try_from(at.x).map_err(|_| ())?] = c;
        }
        Ok(())
    }
}
#[test]
#[ignore = "requires generated full private V2 image pack"]
fn test_should_combine_real_loaded_rows_and_dynamic_overlays_without_missing_or_stale_cells() {
    let path = std::env::var_os("WATTCH5_TEST_IMAGES").expect("generated pack required");
    let assets = ImageAssets::open(Bytes(std::fs::read(path).unwrap())).unwrap();
    for page in Screen::ALL {
        for locale in [Locale::Zh, Locale::En] {
            for theme in [Theme::Phosphor, Theme::Amber, Theme::Ice] {
                for data in [Snapshot::unavailable(), Snapshot::default()] {
                    let mut model = Model::background_model(page, locale, 0).unwrap();
                    let mut appearance = model.appearance();
                    appearance.theme = theme;
                    model.set_appearance(appearance).unwrap();
                    let scene = model.product_scene(&data).unwrap();
                    let mut want = Frame(vec![0; 16384]);
                    scene.draw(&mut want).unwrap();
                    let mut actual = Vec::new();
                    let mut pixels = [Rgb565::new(0, 0, 0); 128];
                    for y in 0..128 {
                        assets
                            .row(scene.background_model(), y, &mut pixels)
                            .unwrap();
                        scene
                            .draw_dynamic_row(&mut Row {
                                y,
                                pixels: &mut pixels,
                            })
                            .unwrap();
                        actual.extend(pixels.map(IntoStorage::into_storage));
                    }
                    assert_eq!(actual, want.0, "{page:?} {locale:?} {theme:?}");
                }
            }
        }
    }
}
