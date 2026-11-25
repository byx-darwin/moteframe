//! Record physical GPIO configuration writes to distinguish polling from IRQ ownership.
#![allow(clippy::unwrap_used)]
use std::{cell::RefCell, collections::BTreeMap};
use syd881x_hal::{
    drivers::{Board, Input, PinId, Pull},
    registers::{Register8, Register32, Registers},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Write {
    Byte(Register8, u8),
    Word(Register32, u32),
}
#[derive(Debug, Default)]
struct Io {
    values: RefCell<BTreeMap<usize, u32>>,
    writes: RefCell<Vec<Write>>,
}
impl Io {
    fn value(&self, a: usize) -> u32 {
        self.values.borrow().get(&a).copied().unwrap_or(0)
    }
    fn seed32(&self, r: Register32, v: u32) {
        self.values.borrow_mut().insert(r.address(), v);
    }
    fn seed8(&self, r: Register8, v: u8) {
        self.values.borrow_mut().insert(r.address(), u32::from(v));
    }
}
impl Registers for Io {
    fn read8(&self, r: Register8) -> u8 {
        self.value(r.address()).to_le_bytes()[0]
    }
    fn read32(&self, r: Register32) -> u32 {
        self.value(r.address())
    }
    fn write8(&self, r: Register8, v: u8) {
        self.writes.borrow_mut().push(Write::Byte(r, v));
        self.seed8(r, v);
    }
    fn write32(&self, r: Register32, v: u32) {
        self.writes.borrow_mut().push(Write::Word(r, v));
        self.seed32(r, v);
    }
    fn update8(&self, r: Register8, c: u8, s: u8) {
        self.write8(r, (self.read8(r) & !c) | s);
    }
    fn update32(&self, r: Register32, c: u32, s: u32) {
        self.write32(r, (self.read32(r) & !c) | s);
    }
}
fn configure_for_polling(input: &mut Input<'_, Io>, inverted: bool) {
    input.configure_polled_event(inverted);
}
#[test]
fn test_should_configure_polled_source_without_any_nvic_write() {
    for inverted in [false, true] {
        let board = Board::new(Io::default());
        let mut input = board.input(PinId::new(22).unwrap(), Pull::Up).unwrap();
        let io = board.registers();
        io.seed32(Register32::Invert, 0xa580_0000);
        io.seed32(Register32::GpioMask, 0xffff_ffff);
        io.seed8(Register8::GpioTopMask, 0xad);
        io.writes.borrow_mut().clear();
        configure_for_polling(&mut input, inverted);
        let writes = io.writes.borrow();
        assert!(
            !writes.iter().any(|w| matches!(
                w,
                Write::Word(
                    Register32::NvicEnable | Register32::NvicDisable | Register32::NvicClear,
                    _
                )
            )),
            "polling must not change NVIC: {writes:?}"
        );
        let selected = 1 << 22;
        assert_eq!(
            *writes,
            [
                Write::Word(
                    Register32::Invert,
                    (0xa580_0000 & !selected) | if inverted { selected } else { 0 }
                ),
                Write::Word(Register32::GpioClear, selected),
                Write::Word(Register32::GpioMask, !selected),
                Write::Byte(Register8::GpioTopMask, 0xac)
            ]
        );
        drop(writes);
        assert_eq!(io.read32(Register32::GpioMask) & !(1 << 22), !(1 << 22));
    }
}
#[test]
fn test_should_preserve_existing_irq_api_nvic_enable_semantics() {
    let board = Board::new(Io::default());
    let mut input = board.input(PinId::new(22).unwrap(), Pull::Up).unwrap();
    let io = board.registers();
    io.writes.borrow_mut().clear();
    input.enable_interrupt(true);
    assert_eq!(
        io.writes.borrow().last(),
        Some(&Write::Word(Register32::NvicEnable, 1 << 12))
    );
}
