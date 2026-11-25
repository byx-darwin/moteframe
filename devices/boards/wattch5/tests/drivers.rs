//! Drive the production register algorithms against byte-addressed hardware fixtures.
use board_syd8810::drivers::{Board, Error, PinId, Pull, SpiConfig, SpiSpeed};
use board_syd8810::registers::{Register8, Register32, Registers};
use device_core::{OutputPin, SpiWrite};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
struct Io {
    values: RefCell<BTreeMap<usize, u32>>,
    writes: RefCell<Vec<(usize, u32)>>,
    stuck: Cell<bool>,
    spi_reads: Cell<usize>,
    completions: Cell<usize>,
}
impl Io {
    fn seed(&self, address: usize, value: u32) {
        self.values.borrow_mut().insert(address, value);
    }
    fn value(&self, address: usize) -> u32 {
        self.values.borrow().get(&address).copied().unwrap_or(0)
    }
    fn write(&self, address: usize, value: u32) {
        self.seed(address, value);
        self.writes.borrow_mut().push((address, value));
    }
}
impl Registers for Io {
    fn read8(&self, register: Register8) -> u8 {
        if register == Register8::SpiInput {
            assert_eq!(
                self.value(0x5001_0078) & 0x40,
                0,
                "RX read before transfer completed"
            );
        }
        let value = self.value(register.address()).to_le_bytes()[0];
        if register == Register8::SpiControl {
            self.spi_reads.set(self.spi_reads.get() + 1);
        }
        if register == Register8::SpiControl && !self.stuck.get() {
            let ready = if value & 0x40 != 0 {
                self.completions.set(self.completions.get() + 1);
                (value & !0x40) | 0x20
            } else {
                value
            };
            self.seed(register.address(), u32::from(ready));
            ready
        } else {
            value
        }
    }
    fn write8(&self, register: Register8, value: u8) {
        if register == Register8::SpiOutput {
            assert_eq!(
                self.value(Register8::SpiControl.address()) & 0x40,
                0,
                "OUT overwritten while shifting"
            );
        }
        self.write(register.address(), u32::from(value));
    }
    fn read32(&self, register: Register32) -> u32 {
        self.value(register.address())
    }
    fn write32(&self, register: Register32, value: u32) {
        self.write(register.address(), value);
    }
    fn update8(&self, register: Register8, clear: u8, set: u8) {
        self.write8(register, (self.read8(register) & !clear) | set);
    }
    fn update32(&self, register: Register32, clear: u32, set: u32) {
        self.write32(register, (self.read32(register) & !clear) | set);
    }
}

fn pin(id: u8) -> PinId {
    PinId::new(id).unwrap_or_default()
}

#[test]
fn test_should_set_output_latch_before_direction_and_preserve_other_pins() {
    let board = Board::new(Io::default());
    board.registers().seed(0x5000_1024, u32::MAX);
    board.registers().seed(0x5000_1014, 1 << 9);
    let output = board.output(pin(10), true);
    assert!(output.is_ok());
    let writes = board.registers().writes.borrow();
    let latch = writes
        .iter()
        .position(|entry| *entry == (0x5000_1014, 0x600));
    let direction = writes
        .iter()
        .position(|entry| *entry == (0x5000_1024, 0xffff_fbff));
    assert!(latch.is_some_and(|a| direction.is_some_and(|b| a < b)));
    assert_eq!(board.registers().value(0x5000_106a), 0);
    assert!(matches!(
        board.output(pin(10), false),
        Err(Error::ResourceBusy)
    ));
}
#[test]
fn test_should_bound_pin_numbers_and_reclaim_output_on_drop() {
    assert!(PinId::new(32).is_err());
    let board = Board::new(Io::default());
    {
        let mut output = board.output(pin(9), false).ok();
        assert!(
            output
                .as_mut()
                .is_some_and(|pin| pin.set_level(true).is_ok())
        );
    }
    assert!(board.output(pin(9), false).is_ok());
}
#[test]
fn test_should_map_package_pullup_bits_and_enable_real_input() {
    let board = Board::new(Io::default());
    assert!(board.input(pin(11), Pull::Up).is_ok());
    assert_eq!(board.registers().value(0x5000_1080), 1 << 17);
    assert_eq!(board.registers().value(0x5000_1018), 1 << 11);
    assert!(board.input(pin(24), Pull::Up).is_ok());
    assert_eq!(board.registers().value(0x5000_1084), 1);
}
#[test]
fn test_should_poll_spi_to_completion_and_reject_conflicting_pin_ownership() {
    let board = Board::new(Io::default());
    let mut spi = board
        .spi(SpiConfig::new(SpiSpeed::Mhz32, false, false, 3))
        .ok();
    assert!(matches!(
        board.output(pin(14), true),
        Err(Error::ResourceBusy)
    ));
    assert!(
        spi.as_mut()
            .is_some_and(|bus| bus.write(&[0xab, 0xcd]).is_ok())
    );
    let data: Vec<u32> = board
        .registers()
        .writes
        .borrow()
        .iter()
        .filter(|e| e.0 == 0x5001_0079)
        .map(|e| e.1)
        .collect();
    assert_eq!(data, vec![0xab, 0xcd]);
    assert_eq!(board.registers().value(0x5000_106e), 9);
    assert_eq!(board.registers().value(0x5000_1073), 8);
}
#[test]
fn test_should_fail_spi_timeout_and_refuse_next_write_without_silent_success() {
    let board = Board::new(Io::default());
    board.registers().stuck.set(true);
    if let Ok(mut spi) = board.spi(SpiConfig::new(SpiSpeed::Mhz8, false, false, 2)) {
        assert_eq!(spi.write(&[1, 2]), Err(Error::Timeout));
        let count = board.registers().writes.borrow().len();
        assert_eq!(spi.write(&[3]), Err(Error::SpiFault));
        assert_eq!(board.registers().writes.borrow().len(), count);
    } else {
        panic!("SPI construction failed");
    }
}
#[test]
fn test_should_ack_only_enabled_pending_gpio_and_leave_timer3_alone() {
    let board = Board::new(Io::default());
    board.registers().seed(0x4000_0020, 2);
    board.registers().seed(0x4000_0028, 7);
    assert_eq!(board.gpio_interrupt(), 5);
    assert_eq!(board.registers().value(0x4000_0010), 5);
    assert!(
        board
            .registers()
            .writes
            .borrow()
            .iter()
            .all(|(address, _)| *address != 0x5000_100c)
    );
}

#[test]
fn test_should_keep_backlight_off_until_valid_duty_and_release_it_off() {
    let board = Board::new(Io::default());
    assert!(matches!(
        board.backlight(0),
        Err(Error::InvalidConfiguration)
    ));
    if let Ok(mut light) = board.backlight(100) {
        assert_eq!(board.registers().value(0x5001_005e) & 0x80, 0);
        assert!(light.set_duty(75).is_ok());
        assert_eq!(board.registers().value(0x5001_005f), 75);
        assert_eq!(board.registers().value(0x5001_005e) & 0xf0, 0xd0);
        assert_eq!(light.set_duty(101), Err(Error::InvalidConfiguration));
        assert_eq!(board.registers().value(0x5001_005f), 75);
    } else {
        panic!("PWM initialization failed");
    }
    assert_eq!(board.registers().value(0x5001_005e) & 0x80, 0);
    assert!(board.output(pin(6), false).is_ok());
}

#[test]
fn test_should_initialize_only_board_gpio_and_deselect_shared_spi_slaves() {
    let mut board = Board::new(Io::default());
    assert!(device_core::Board::initialize(&mut board).is_ok());
    assert_eq!(board.registers().value(0xe000_e180), 1 << 12);
    assert_eq!(board.registers().value(0x4000_0020), u32::MAX);
    assert_eq!(board.registers().value(0x5000_1014) & 0x18000, 0x18000);
    assert!(
        board
            .registers()
            .writes
            .borrow()
            .iter()
            .all(|(address, _)| *address != 0x5000_100d)
    );
}

#[test]
fn test_should_wait_each_byte_but_avoid_redundant_ready_reads_between_bytes() {
    let board = Board::new(Io::default());
    let Ok(mut spi) = board.spi(SpiConfig::new(SpiSpeed::Mhz8, false, false, 3)) else {
        panic!("SPI initialization failed")
    };
    assert!(spi.write(&[0xab, 0xcd, 0xef]).is_ok());
    assert_eq!(board.registers().completions.get(), 3);
    assert!(
        board.registers().spi_reads.get() <= 4,
        "three immediately-ready bytes need only initial ready plus three completions"
    );
    assert_eq!(board.registers().value(0x5001_0078), 0x8a);
    let count = board.registers().writes.borrow().len();
    let reads = board.registers().spi_reads.get();
    assert!(spi.write(&[]).is_ok());
    assert_eq!(board.registers().writes.borrow().len(), count);
    assert_eq!(board.registers().spi_reads.get(), reads);
}
#[test]
fn test_should_not_overwrite_output_when_already_busy_and_poison_after_timeout() {
    let board = Board::new(Io::default());
    let Ok(mut spi) = board.spi(SpiConfig::new(SpiSpeed::Mhz8, false, false, 2)) else {
        panic!("SPI initialization failed")
    };
    board.registers().seed(0x5001_0078, 0xca);
    board.registers().stuck.set(true);
    let writes = board.registers().writes.borrow().len();
    assert_eq!(spi.write(&[1]), Err(Error::Timeout));
    assert_eq!(board.registers().writes.borrow().len(), writes);
    assert_eq!(spi.write(&[2]), Err(Error::SpiFault));
}

#[test]
fn test_should_release_open_drain_without_ever_driving_high() {
    use board_syd8810::drivers::i2c::Line;
    let board = Board::new(Io::default());
    let wire = pin(21);
    board.registers().seed(0x5000_1014, 1 << 21);
    let mut line = board.open_drain(wire).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(board.registers().value(0x5000_1014) & (1 << 21), 0);
    line.low();
    assert_eq!(board.registers().value(0x5000_1024) & (1 << 21), 0);
    assert_ne!(board.registers().value(0x5000_1018) & (1 << 21), 0);
    line.release();
    assert_ne!(board.registers().value(0x5000_1024) & (1 << 21), 0);
    assert_eq!(board.registers().value(0x5000_1014) & (1 << 21), 0);
    assert!(matches!(board.output(wire, true), Err(Error::ResourceBusy)));
    drop(line);
    assert!(board.output(wire, true).is_ok());
}

#[test]
fn test_should_reserve_miso_and_read_rx_only_after_spi_completion() {
    use embedded_hal::spi::SpiBus;
    let board = Board::new(Io::default());
    board.registers().seed(0x5000_1020, u32::MAX);
    board.registers().seed(0x5001_007a, 0xa5);
    let mut spi = board
        .spi_full_duplex(SpiConfig::new(SpiSpeed::Mhz8, false, false, 4))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(board.registers().value(0x5000_106c) & 0x3f, 10);
    assert_ne!(board.registers().value(0x5000_1018) & (1 << 12), 0);
    assert_eq!(board.registers().value(0x5000_1020) & (1 << 12), 0);
    assert!(matches!(
        board.input(pin(12), Pull::Up),
        Err(Error::ResourceBusy)
    ));
    let mut bytes = [0; 3];
    spi.read(&mut bytes).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(bytes, [0xa5; 3]);
    assert_eq!(board.registers().completions.get(), 3);
    board.registers().stuck.set(true);
    assert_eq!(spi.read(&mut bytes), Err(Error::Timeout));
    assert_eq!(spi.read(&mut bytes), Err(Error::SpiFault));
    drop(spi);
    assert!(matches!(
        board.input(pin(12), Pull::Up),
        Err(Error::ResourceBusy)
    ));
}
