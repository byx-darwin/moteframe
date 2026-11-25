//! The native reset request must mask IRQs and preserve every unrelated PWR bit.
use syd881x_runtime::reset::{PowerResetRegisters, request_native_reset};

#[derive(Debug)]
struct Port {
    power: u8,
    events: Vec<String>,
}
impl PowerResetRegisters for Port {
    fn mask_interrupts(&mut self) {
        self.events.push("mask-irqs".into());
    }
    fn power_control(&mut self) -> u8 {
        self.events.push("read-power".into());
        self.power
    }
    fn set_power_control(&mut self, value: u8) {
        self.events.push(format!("power-{value}"));
        self.power = value;
    }
}

#[test]
fn test_should_mask_interrupts_before_read_modify_write_of_native_reset() {
    let mut port = Port {
        power: 0xdf,
        events: Vec::new(),
    };
    request_native_reset(&mut port);
    assert_eq!(port.events, ["mask-irqs", "read-power", "power-95"]);
    assert_eq!(port.power, 0x5f);
}

#[test]
fn test_should_clear_only_the_native_reset_bit_for_every_prior_value() {
    for original in 0..=u8::MAX {
        let mut port = Port {
            power: original,
            events: Vec::new(),
        };
        request_native_reset(&mut port);
        assert_eq!(port.power, original % 128);
    }
}
