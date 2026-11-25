use crate::{Event, ResetGate};
use core::cell::Cell;
use device_core::{Dimensions, Display, Rgb565 as Packed};
use device_runtime::{PortError, Work};
use device_ui::{
    DirtyRegion,
    graphics::{
        mono_font::{MonoTextStyle, ascii::FONT_6X10},
        pixelcolor::Rgb565,
        prelude::*,
        primitives::Rectangle,
        text::Text,
    },
    repaint,
};
/// Live stage observed by actual advertising/accept/GATT operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStage {
    /// Host reset/setup in progress.
    Initializing,
    /// Advertisement command is pending.
    AdvertisingCommand,
    /// Advertising command completed successfully.
    Advertising,
    /// An actual connection was accepted.
    Connected,
    /// Actual clean disconnection.
    Disconnected,
    /// Actual Host/GATT/runtime failure.
    Fault,
}
/// Physical whole-read status, never inferred from bus construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchStatus {
    /// No successful physical sample yet.
    Pending,
    /// A whole register read succeeded.
    Ready,
    /// Read/normalization/queue operation failed.
    Fault,
}
/// Only declared absence and live link/input diagnostics; no product snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Readiness {
    link: LinkStage,
    touch: TouchStatus,
    last: Option<Event>,
}
impl Default for Readiness {
    fn default() -> Self {
        Self::new()
    }
}
impl Readiness {
    /// Diagnostic initial state with all product providers unbound.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            link: LinkStage::Initializing,
            touch: TouchStatus::Pending,
            last: None,
        }
    }
    /// Set only from an observed actual source operation.
    pub fn set_link(&mut self, link: LinkStage) {
        self.link = link;
    }
    /// Actual observed connection stage.
    #[must_use]
    pub const fn link(&self) -> LinkStage {
        self.link
    }
    /// Literal diagnostic text; no fabricated observations.
    #[must_use]
    pub fn line(&self, n: usize) -> &'static str {
        match n {
            0 => "WATTCH5 BRINGUP",
            1 => "CONTROL ONLY / TEST",
            2 => match self.link {
                LinkStage::Initializing => "BLE INITIALIZING",
                LinkStage::AdvertisingCommand => "BLE ADV PENDING",
                LinkStage::Advertising => "BLE ADVERTISING",
                LinkStage::Connected => "BLE CONNECTED",
                LinkStage::Disconnected => "BLE DISCONNECTED",
                LinkStage::Fault => "BLE FAULT",
            },
            3 => "STORAGE UNBOUND",
            4 => "UTC UNSYNC",
            5 => "SENSOR UNBOUND",
            6 => match self.touch {
                TouchStatus::Pending => "TOUCH PENDING",
                TouchStatus::Ready => "TOUCH READ OK",
                TouchStatus::Fault => "TOUCH FAULT",
            },
            7 => match self.last {
                None => "INPUT NONE",
                Some(Event::Tap { .. }) => "INPUT TAP",
                Some(Event::Down) => "INPUT DOWN",
                Some(Event::Up) => "INPUT UP",
                Some(Event::Left) => "INPUT LEFT",
                Some(Event::Right) => "INPUT RIGHT",
                Some(Event::LongPress) => "INPUT TOUCH HOLD",
                Some(Event::KeyRelease) => "INPUT KEY RELEASE",
                Some(Event::KeyLongPress) => "INPUT KEY HOLD",
            },
            _ => "",
        }
    }
}
/// One bounded physical-input slice, retaining event and fault independently.
#[derive(Debug, Clone, Copy)]
pub struct InputReport {
    /// At most one consumed queued interaction.
    pub event: Option<Event>,
    /// Actual touch read status.
    pub touch: TouchStatus,
    /// Actual failure, retained alongside any valid event.
    pub fault: Option<PortError>,
}
/// Physical input boundary; implementations must bound every operation.
pub trait InputSource {
    /// Poll actual sources using boot-local nominal microseconds.
    fn sample(&mut self, now_us: u64) -> InputReport;
}
/// One physical row per foreground slice, with stable snapshot and coalesced follow-up damage.
#[derive(Debug)]
pub struct ReadinessUi<'a, D, I> {
    display: D,
    input: I,
    status: &'a Cell<Readiness>,
    gate: &'a ResetGate,
    shown: Readiness,
    snapshot: Readiness,
    row: Option<u16>,
    again: bool,
    scratch: [Packed; 128],
}
impl<'a, D: Display, I: InputSource> ReadinessUi<'a, D, I> {
    /// Bind initialized128x128 panel and fallible physical input after actual Host reset.
    /// # Errors
    /// Rejects mismatched geometry or missing exclusive UI ownership.
    pub fn new(
        display: D,
        input: I,
        status: &'a Cell<Readiness>,
        gate: &'a ResetGate,
    ) -> Result<Self, PortError> {
        if display.dimensions() != Dimensions::new(128, 128) || !gate.ui_allowed() {
            return Err(PortError::InvalidInput);
        }
        let s = status.get();
        Ok(Self {
            display,
            input,
            status,
            gate,
            shown: s,
            snapshot: s,
            row: Some(0),
            again: false,
            scratch: [Packed(0); 128],
        })
    }
    /// Current unfinished row, retained after display failure.
    #[must_use]
    pub const fn pending_row(&self) -> Option<u16> {
        self.row
    }
}
impl<D: Display, I: InputSource> Work for ReadinessUi<'_, D, I> {
    fn step(&mut self, now_ms: u64) -> Result<(), PortError> {
        // Late reset retires all bus access, including input, before touching pins.
        if !self.gate.ui_allowed() {
            return Err(PortError::Fault);
        }
        let input = self.input.sample(now_ms.saturating_mul(1000));
        let mut live = self.status.get();
        live.touch = input.touch;
        if let Some(e) = input.event {
            live.last = Some(e)
        }
        self.status.set(live);
        if live != self.snapshot {
            if self.row.is_some() {
                self.again = true
            } else {
                self.snapshot = live;
                self.row = Some(0)
            }
        }
        let mut render = Ok(());
        if let Some(row) = self.row {
            let mut dirty = DirtyRegion::new(Dimensions::new(128, 128));
            dirty.invalidate(Rectangle::new(
                Point::new(0, i32::from(row)),
                Size::new(128, 1),
            ));
            let snapshot = self.snapshot;
            render = repaint(
                &mut self.display,
                &mut dirty,
                &mut self.scratch,
                Rgb565::BLACK,
                |tile| {
                    let style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
                    for n in 0..8 {
                        Text::new(snapshot.line(n), Point::new(1, 12 + n as i32 * 14), style)
                            .draw(tile)?;
                    }
                    Ok::<(), core::convert::Infallible>(())
                },
            )
            .map_err(|_| PortError::Fault);
            if render.is_ok() {
                if row == 127 {
                    self.shown = self.snapshot;
                    if self.again {
                        self.snapshot = live;
                        self.row = Some(0);
                        self.again = false
                    } else {
                        self.row = None
                    }
                } else {
                    self.row = Some(row + 1)
                }
            }
        }
        render.and(input.fault.map_or(Ok(()), Err))
    }
}
