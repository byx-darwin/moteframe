//! Actual factory source through the product scheduler; old V2 overlays must stay absent.
#![allow(clippy::unwrap_used)]
use core::cell::{Cell, RefCell};
use device_core::{Dimensions, Display, Rect, Rgb565};
use device_runtime::{PortError, Work};
use wattch5_bringup::{Event, InputReport, InputSource, Readiness, ResetGate, TouchStatus};
use wattch5_candidate::v2_ui::V2Ui;
use wattch5_parity_ui_demo::{FactoryAssets, Screen};
#[derive(Debug)]
struct Source<'a> {
    bytes: Vec<u8>,
    fail: &'a Cell<Option<usize>>,
    reads: &'a Cell<usize>,
}
impl moteframe_core::ReadAt for Source<'_> {
    type Error = PortError;
    fn capacity(&self) -> u32 {
        self.bytes.len().try_into().unwrap()
    }
    fn read_exact(&mut self, at: u32, out: &mut [u8]) -> Result<(), PortError> {
        assert!(out.len() <= 256);
        let n = self.reads.get() + 1;
        self.reads.set(n);
        if self.fail.get() == Some(n) {
            out[0] = 255;
            return Err(PortError::Unavailable);
        }
        out.copy_from_slice(
            self.bytes
                .get(at as usize..at as usize + out.len())
                .ok_or(PortError::Fault)?,
        );
        Ok(())
    }
}
struct Panel<'a>(&'a RefCell<Vec<Rgb565>>, &'a Cell<usize>);
impl Display for Panel<'_> {
    type Error = ();
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(128, 128)
    }
    fn write(&mut self, r: Rect, p: &[Rgb565]) -> Result<(), ()> {
        let start = usize::from(r.y) * 128 + usize::from(r.x);
        self.0.borrow_mut()[start..start + p.len()].copy_from_slice(p);
        self.1.set(self.1.get() + 1);
        Ok(())
    }
}
struct Input<'a>(&'a Cell<Option<Event>>);
impl InputSource for Input<'_> {
    fn sample(&mut self, _: u64) -> InputReport {
        InputReport {
            event: self.0.take(),
            touch: TouchStatus::Ready,
            fault: None,
        }
    }
}
fn image() -> Vec<u8> {
    let mut bytes = vec![0; 9888];
    bytes[..8].copy_from_slice(b"W5UIF001");
    for (i, v) in [1u32, 2336, 9376, 9632, 9888, 1].into_iter().enumerate() {
        bytes[8 + i * 4..12 + i * 4].copy_from_slice(&v.to_le_bytes());
    }
    for b in bytes[32..9376].chunks_exact_mut(4) {
        b.copy_from_slice(&9376u32.to_le_bytes());
    }
    for b in bytes[9632..].chunks_exact_mut(2) {
        b.copy_from_slice(&0xf800u16.to_be_bytes());
    }
    bytes
}
#[test]
fn test_should_retry_factory_resource_contention_without_panel_write_or_losing_navigation() {
    let fail = Cell::new(None);
    let reads = Cell::new(0);
    let frames = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let input = Cell::new(None);
    let assets = FactoryAssets::open(Source {
        bytes: image(),
        fail: &fail,
        reads: &reads,
    })
    .unwrap();
    let gate = ResetGate::new();
    gate.completed();
    gate.arm_ui().unwrap();
    let status = Cell::new(Readiness::new());
    let mut ui = V2Ui::new(
        Panel(&frames, &writes),
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind_background(&assets);
    // Fail in the second data chunk, after the scratch row already changed.
    fail.set(Some(5));
    ui.step(0).unwrap();
    assert_eq!(writes.get(), 0);
    assert_eq!(ui.next_paint_pixel(), Some(0));
    for n in 1..=128 {
        ui.step(n).unwrap();
    }
    assert_eq!(ui.frames(), 1);
    assert!(frames.borrow().iter().all(|p| p.0 == 0xf800));
    input.set(Some(Event::Left));
    ui.step(129).unwrap();
    assert_eq!(ui.model().screen(), Screen::Activity);
    for n in 130..258 {
        ui.step(n).unwrap();
    }
    assert!(
        frames.borrow().iter().all(|p| p.0 == 0xf800),
        "retro heading/dynamic bands must never overwrite factory pixels"
    );
}
