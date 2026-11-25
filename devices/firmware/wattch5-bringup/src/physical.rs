//! Wattch5 physical input; foreground is the only GPIO pending acknowledgement owner.
use board_syd8810::{
    drivers::{
        Board, Input, PinId, Pull,
        i2c::{BitBangI2c, OpenDrain},
    },
    hardware::Mmio,
};
use device_core::{DelayMs, OutputPin};
use device_runtime::PortError;
use wattch5_bringup::{InputReport, InputSource, InputState, TouchStatus};
#[derive(Debug)]
pub(super) struct Inputs<'a> {
    board: &'a Board<Mmio>,
    irq: Input<'a, Mmio>,
    key: Input<'a, Mmio>,
    bus: BitBangI2c<OpenDrain<'a, Mmio>, OpenDrain<'a, Mmio>, fn()>,
    state: InputState,
    touch: TouchStatus,
    reads: u32,
    errors: u32,
    pending: bool,
}
fn pin(n: u8) -> Result<PinId, PortError> {
    PinId::new(n).map_err(|_| PortError::Fault)
}
fn pause() {
    super::settle(2);
}
impl<'a> Inputs<'a> {
    pub(super) fn new(board: &'a Board<Mmio>, delay: &mut impl DelayMs) -> Result<Self, PortError> {
        let mut reset = board
            .output(pin(board_syd8810::TOUCH_PINS.reset)?, false)
            .map_err(|_| PortError::Fault)?;
        delay.delay_ms(10);
        reset.set_level(true).map_err(|_| PortError::Fault)?;
        delay.delay_ms(10);
        let mut irq = board
            .input(pin(board_syd8810::TOUCH_PINS.interrupt)?, Pull::Up)
            .map_err(|_| PortError::Fault)?;
        // Peripheral source may latch pending, but GPIO NVIC is never enabled. Only sample() acknowledges it.
        irq.configure_polled_event(true);
        let key = board
            .input(pin(board_syd8810::TOUCH_PINS.key)?, Pull::Up)
            .map_err(|_| PortError::Fault)?;
        let clock = board
            .open_drain(pin(board_syd8810::TOUCH_PINS.clock)?)
            .map_err(|_| PortError::Fault)?;
        let data = board
            .open_drain(pin(board_syd8810::TOUCH_PINS.data)?)
            .map_err(|_| PortError::Fault)?;
        let bus = BitBangI2c::new(clock, data, pause as fn(), 32).map_err(|_| PortError::Fault)?;
        Ok(Self {
            board,
            irq,
            key,
            bus,
            state: InputState::new(),
            touch: TouchStatus::Pending,
            reads: 0,
            errors: 0,
            pending: false,
        })
    }
}
impl InputSource for Inputs<'_> {
    fn sample(&mut self, now: u64) -> InputReport {
        let mut fault = self.state.key(self.key.is_high(), now).err();
        self.pending |=
            self.board.gpio_interrupt() & (1 << board_syd8810::TOUCH_PINS.interrupt) != 0;
        if self.state.due(self.pending || self.irq.is_high(), now) {
            self.pending = false;
            let frame = device_touch::read_frame(
                board_syd8810::TOUCH_ADDRESS,
                board_syd8810::TOUCH_REGISTERS,
                |address, register, out| self.bus.read(address, register, out),
            )
            .map_err(|e| {
                super::record(18, e as u32 + 1);
                PortError::Fault
            });
            if let Ok(bytes) = frame {
                self.reads = self.reads.saturating_add(1);
                super::record(17, u32::from_be_bytes(bytes));
            }
            let result = self.state.touch(frame, now);
            self.touch = if result.is_ok() {
                TouchStatus::Ready
            } else {
                TouchStatus::Fault
            };
            if let Err(error) = result {
                fault = Some(error);
            }
        }
        if fault.is_some() {
            self.errors = self.errors.saturating_add(1);
            self.touch = TouchStatus::Fault;
        }
        super::record(15, self.reads);
        super::record(16, self.errors);
        InputReport {
            event: self.state.pop(),
            touch: self.touch,
            fault,
        }
    }
}
