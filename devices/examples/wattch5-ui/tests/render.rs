//! Presentation behavior against independent literal fixtures.
use device_core::{Dimensions, Display, Rect, Rgb565 as Packed};
use device_ui::{DirtyRegion, graphics::pixelcolor::Rgb565, repaint};
use wattch5_ui_demo::{Bundle, Event, Model};
#[derive(Debug)]
struct Screen(Vec<Packed>);
impl Display for Screen {
    type Error = ();
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(128, 128)
    }
    fn write(&mut self, rect: Rect, pixels: &[Packed]) -> Result<(), ()> {
        for (row_index, row) in pixels.chunks(usize::from(rect.width)).enumerate() {
            let at = (usize::from(rect.y) + row_index) * 128 + usize::from(rect.x);
            self.0[at..at + row.len()].copy_from_slice(row);
        }
        Ok(())
    }
}
fn bundle() -> Vec<u8> {
    let sizes = [
        (53, 63, 10),
        (36, 36, 5),
        (80, 21, 1),
        (80, 21, 1),
        (80, 21, 1),
        (80, 21, 1),
        (36, 36, 8),
        (80, 21, 1),
        (80, 21, 1),
        (80, 21, 1),
        (80, 21, 1),
        (80, 21, 1),
        (80, 21, 1),
        (80, 21, 1),
        (80, 21, 1),
        (70, 19, 1),
        (10, 13, 10),
        (4, 13, 1),
        (128, 24, 1),
        (10, 11, 5),
        (13, 14, 5),
        (128, 24, 1),
    ];
    let mut bytes = vec![0; 274];
    bytes[..8].copy_from_slice(b"W5UI\x01\0\0\0");
    bytes[8..10].copy_from_slice(&22_u16.to_be_bytes());
    for (index, (width, height, count)) in sizes.into_iter().enumerate() {
        let start = bytes.len();
        for frame in 0..count {
            for _ in 0..width * height {
                bytes.extend_from_slice(
                    &(u16::try_from((index + 1) * 256 + frame).unwrap_or_default()).to_be_bytes(),
                );
            }
        }
        let at = 10 + index * 12;
        bytes[at..at + 2].copy_from_slice(&u16::try_from(width).unwrap_or_default().to_be_bytes());
        bytes[at + 2..at + 4].copy_from_slice(
            &u16::try_from(height * count)
                .unwrap_or_default()
                .to_be_bytes(),
        );
        bytes[at + 4..at + 8]
            .copy_from_slice(&u32::try_from(start).unwrap_or_default().to_be_bytes());
        let len = bytes.len() - start;
        bytes[at + 8..at + 12]
            .copy_from_slice(&u32::try_from(len).unwrap_or_default().to_be_bytes());
    }
    bytes
}
#[test]
fn test_should_place_independent_digit_frames_and_clear_unused_menu_rows() {
    let bytes = bundle();
    let assets = Bundle::new(&bytes).unwrap_or_else(|e| panic!("{e:?}"));
    let mut model = Model::new(10, 50, 3).unwrap_or_else(|e| panic!("{e:?}"));
    let mut screen = Screen(vec![Packed(0xffff); 16384]);
    for stage in 0..3 {
        if stage > 0 {
            assert!(model.handle(Event::Up).is_ok());
        }
        let mut dirty = DirtyRegion::new(screen.dimensions());
        dirty.invalidate_all();
        assert!(
            repaint(
                &mut screen,
                &mut dirty,
                &mut [Packed(0); 32],
                Rgb565::new(0, 0, 0),
                |t| model.draw(&assets, t)
            )
            .is_ok()
        );
        match stage {
            0 => {
                assert_eq!(screen.0[5], Packed(0x101));
                assert_eq!(screen.0[69], Packed(0x100));
                assert_eq!(screen.0[65 * 128 + 5], Packed(0x105));
                assert_eq!(screen.0[64 * 128], Packed(0));
            }
            1 => {
                assert_eq!(screen.0[6 * 128 + 4], Packed(0x201));
                assert_eq!(screen.0[16 * 128 + 46], Packed(0x300));
            }
            _ => {
                assert_eq!(screen.0[6 * 128 + 4], Packed(0x204));
                assert_eq!(screen.0[46 * 128 + 4], Packed(0));
                assert_eq!(screen.0[55 * 128 + 46], Packed(0));
            }
        }
    }
}

struct Count<'a, T> {
    target: &'a mut T,
    consumed: &'a std::cell::Cell<usize>,
}
impl<T: device_ui::graphics::prelude::DrawTarget<Color = Rgb565>>
    device_ui::graphics::prelude::Dimensions for Count<'_, T>
{
    fn bounding_box(&self) -> device_ui::graphics::primitives::Rectangle {
        self.target.bounding_box()
    }
}
impl<T: device_ui::graphics::prelude::DrawTarget<Color = Rgb565>>
    device_ui::graphics::prelude::DrawTarget for Count<'_, T>
{
    type Color = Rgb565;
    type Error = T::Error;
    fn draw_iter<I: IntoIterator<Item = device_ui::graphics::Pixel<Rgb565>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        self.target.draw_iter(pixels)
    }
    fn fill_contiguous<I: IntoIterator<Item = Rgb565>>(
        &mut self,
        area: &device_ui::graphics::primitives::Rectangle,
        colors: I,
    ) -> Result<(), Self::Error> {
        self.target.fill_contiguous(
            area,
            colors
                .into_iter()
                .inspect(|_| self.consumed.set(self.consumed.get() + 1)),
        )
    }
}
#[test]
fn test_should_decode_each_visible_dial_pixel_only_once_across_row_tiles() {
    let bytes = bundle();
    let assets = Bundle::new(&bytes).unwrap_or_else(|e| panic!("{e:?}"));
    let model = Model::new(10, 50, 3).unwrap_or_else(|e| panic!("{e:?}"));
    let mut screen = Screen(vec![Packed(0); 16384]);
    let mut dirty = DirtyRegion::new(screen.dimensions());
    dirty.invalidate_all();
    let consumed = std::cell::Cell::new(0);
    assert!(
        repaint(
            &mut screen,
            &mut dirty,
            &mut [Packed(0); 128],
            Rgb565::new(0, 0, 0),
            |tile| model.draw(
                &assets,
                &mut Count {
                    target: tile,
                    consumed: &consumed
                }
            )
        )
        .is_ok()
    );
    assert_eq!(
        consumed.get(),
        13356,
        "four53x63 images must not be decoded whole again for each row"
    );
}
