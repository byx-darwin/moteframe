//! Connection experiments must stop the LL state machine, not only advertising.
#![allow(clippy::unwrap_used, reason = "Unexpected fixture errors abort tests")]
use syd881x_radio::{Bank, ByteRegisters, Radio, RadioError, Register};
#[derive(Debug)]
struct Chip {
    values: [u8; 256],
    stuck: bool,
    polls: usize,
}
impl Chip {
    fn new(stuck: bool) -> Self {
        let mut values = [0; 256];
        values[0x10] = 0xff;
        values[0x60] = 0xff;
        values[0x40] = 0xff;
        Self {
            values,
            stuck,
            polls: 0,
        }
    }
}
impl ByteRegisters for Chip {
    type Error = ();
    fn read8(&mut self, r: Register) -> Result<u8, ()> {
        assert_eq!(r.bank(), Bank::Ll);
        if r.offset() == 0 {
            self.polls += 1;
        }
        Ok(self.values[usize::from(r.offset())])
    }
    fn write8(&mut self, r: Register, value: u8) -> Result<(), ()> {
        assert_eq!(r.bank(), Bank::Ll);
        self.values[usize::from(r.offset())] = if r.offset() == 0 && !self.stuck {
            value & !2
        } else {
            value
        };
        Ok(())
    }
}
#[test]
fn test_should_stop_all_radio_modes_and_reset_link_state_with_a_finite_gate() {
    let mut radio = Radio::new(Chip::new(false), 2).unwrap();
    assert_eq!(radio.stop_link().unwrap(), 1);
    let chip = radio.into_inner();
    for offset in [0x10, 0x60, 0x40] {
        assert_eq!(chip.values[offset], 0xfe);
    }
    assert_eq!(chip.values[0], 0);
}
#[test]
fn test_should_report_stuck_link_reset_instead_of_claiming_stopped() {
    let mut radio = Radio::new(Chip::new(true), 2).unwrap();
    assert_eq!(radio.stop_link(), Err(RadioError::StatusTimeout));
    assert_eq!(radio.into_inner().polls, 4097);
}
