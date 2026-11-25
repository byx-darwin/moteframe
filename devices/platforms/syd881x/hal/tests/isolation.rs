//! Chip ownership must not select another board's devices.
use std::cell::RefCell;
use std::collections::BTreeMap;
use syd881x_hal::drivers::{Board, Error, PinId, SpiConfig, SpiPins, SpiSpeed};
use syd881x_hal::registers::{Register8, Register32, Registers};
#[derive(Debug, Default)]
struct Io(RefCell<BTreeMap<usize, u32>>);
impl Registers for Io {
    fn read8(&self, r: Register8) -> u8 {
        self.read32_at(r.address()).to_le_bytes()[0]
    }
    fn read32(&self, r: Register32) -> u32 {
        self.read32_at(r.address())
    }
    fn write8(&self, r: Register8, v: u8) {
        self.0.borrow_mut().insert(r.address(), u32::from(v));
    }
    fn write32(&self, r: Register32, v: u32) {
        self.0.borrow_mut().insert(r.address(), v);
    }
    fn update8(&self, r: Register8, c: u8, s: u8) {
        self.write8(r, (self.read8(r) & !c) | s);
    }
    fn update32(&self, r: Register32, c: u32, s: u32) {
        self.write32(r, (self.read32(r) & !c) | s);
    }
}
impl Io {
    fn read32_at(&self, a: usize) -> u32 {
        self.0.borrow().get(&a).copied().unwrap_or(0)
    }
}
fn pin(n: u8) -> PinId {
    PinId::new(n).unwrap_or_else(|e| panic!("{e:?}"))
}
#[test]
fn test_should_initialize_chip_gpio_without_selecting_product_slaves() {
    let mut board = Board::new(Io::default());
    device_core::Board::initialize(&mut board).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(board.registers().read32(Register32::Output), 0);
    assert_eq!(board.registers().read32(Register32::GpioMask), u32::MAX);
    assert_eq!(board.registers().read8(Register8::Mux(pin(15))), 0);
}
#[test]
fn test_should_reserve_only_explicit_spi_pins_and_reject_aliases_before_io() {
    let board = Board::new(Io::default());
    assert_eq!(
        SpiPins::new(pin(14), pin(14), None),
        Err(Error::InvalidConfiguration)
    );
    let pins = SpiPins::new(pin(14), pin(19), Some(pin(12))).unwrap_or_else(|e| panic!("{e:?}"));
    let bus = board
        .spi(SpiConfig::new(SpiSpeed::Mhz8, false, false, 4), pins)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(board.output(pin(12), true).is_err());
    assert!(board.output(pin(6), true).is_ok());
    drop(bus);
    assert!(board.output(pin(12), true).is_ok());
}
#[test]
fn test_should_use_supplied_pwm_pin_without_claiming_legacy_backlight() {
    let board = Board::new(Io::default());
    let light = board.pwm0(pin(7), 100).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(board.output(pin(7), true).is_err());
    assert!(board.output(pin(6), true).is_ok());
    drop(light);
    assert!(board.output(pin(7), true).is_ok());
}

#[test]
fn test_should_keep_one_spi_controller_owner_even_with_disjoint_wires() {
    let board = Board::new(Io::default());
    let config = SpiConfig::new(SpiSpeed::Mhz8, false, false, 1);
    let first = SpiPins::new(pin(14), pin(19), None).unwrap_or_else(|e| panic!("{e:?}"));
    let second = SpiPins::new(pin(17), pin(18), None).unwrap_or_else(|e| panic!("{e:?}"));
    let bus = board.spi(config, first).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(matches!(
        board.spi(config, second),
        Err(Error::ResourceBusy)
    ));
    drop(bus);
    assert!(board.spi(config, second).is_ok());
}
#[test]
fn test_should_retain_poisoned_spi_controller_claim_when_switching_wires() {
    let board = Board::new(Io::default());
    let config = SpiConfig::new(SpiSpeed::Mhz8, false, false, 1);
    let first = SpiPins::new(pin(14), pin(19), None).unwrap_or_else(|e| panic!("{e:?}"));
    let second = SpiPins::new(pin(17), pin(18), None).unwrap_or_else(|e| panic!("{e:?}"));
    let mut bus = board.spi(config, first).unwrap_or_else(|e| panic!("{e:?}"));
    board.registers().write8(Register8::SpiControl, 0x40);
    assert_eq!(
        device_core::SpiWrite::write(&mut bus, &[1]),
        Err(Error::Timeout)
    );
    drop(bus);
    assert!(matches!(
        board.spi(config, second),
        Err(Error::ResourceBusy)
    ));
}
#[test]
fn test_should_keep_one_pwm_controller_owner_even_with_disjoint_pins() {
    let board = Board::new(Io::default());
    let light = board.pwm0(pin(6), 100).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(matches!(board.pwm0(pin(7), 100), Err(Error::ResourceBusy)));
    drop(light);
    assert!(board.pwm0(pin(7), 100).is_ok());
}

#[test]
fn test_should_detach_clean_spi_routes_before_the_pins_can_be_rebound() {
    let board = Board::new(Io::default());
    let pins = SpiPins::new(pin(14), pin(19), Some(pin(12))).unwrap_or_else(|e| panic!("{e:?}"));
    let bus = board
        .spi(SpiConfig::new(SpiSpeed::Mhz8, false, false, 1), pins)
        .unwrap_or_else(|e| panic!("{e:?}"));
    drop(bus);
    for number in [14, 19, 12] {
        assert_eq!(
            board.registers().read8(Register8::Mux(pin(number))) & 0x3f,
            0
        );
        assert_ne!(
            board.registers().read32(Register32::Direction) & (1 << number),
            0
        );
    }
}
#[test]
fn test_should_detach_pwm_route_on_clean_drop_and_keep_it_detached_on_rebind() {
    let board = Board::new(Io::default());
    let mut light = board.pwm0(pin(6), 100).unwrap_or_else(|e| panic!("{e:?}"));
    light.set_duty(50).unwrap_or_else(|e| panic!("{e:?}"));
    drop(light);
    assert_eq!(board.registers().read8(Register8::Mux(pin(6))) & 0x3f, 0);
    assert_eq!(board.registers().read8(Register8::PwmControl) & 0x80, 0);
    let mut next = board.pwm0(pin(7), 100).unwrap_or_else(|e| panic!("{e:?}"));
    next.set_duty(50).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(board.registers().read8(Register8::Mux(pin(6))) & 0x3f, 0);
}
#[test]
fn test_should_keep_failed_spi_routes_attached_until_explicit_hardware_recovery() {
    let board = Board::new(Io::default());
    let pins = SpiPins::new(pin(14), pin(19), None).unwrap_or_else(|e| panic!("{e:?}"));
    let mut bus = board
        .spi(SpiConfig::new(SpiSpeed::Mhz8, false, false, 1), pins)
        .unwrap_or_else(|e| panic!("{e:?}"));
    board.registers().write8(Register8::SpiControl, 0x40);
    assert_eq!(
        device_core::SpiWrite::write(&mut bus, &[1]),
        Err(Error::Timeout)
    );
    drop(bus);
    assert_eq!(board.registers().read8(Register8::Mux(pin(14))) & 0x3f, 9);
    assert_eq!(board.registers().read8(Register8::Mux(pin(19))) & 0x3f, 8);
}
