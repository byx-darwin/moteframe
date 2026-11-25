//! Complete ordered board profile contract recovered from the pinned original Flash.
use crate::InitStep;
use crate::profiles::legacy_128::{DIMENSIONS, INITIALIZATION, OFFSET};
#[test]
fn test_should_match_the_original_flash_command_data_delay_trace() {
    assert_eq!(
        (DIMENSIONS.width, DIMENSIONS.height, OFFSET),
        (128, 128, (2, 1))
    );
    let mut trace = std::vec::Vec::<u32>::new();
    for step in INITIALIZATION {
        match step {
            InitStep::Command(command, data) => {
                trace.push(0x100 | u32::from(*command));
                trace.extend(data.iter().copied().map(u32::from));
            }
            InitStep::DelayMs(ms) => trace.push(0x10000 | u32::from(*ms)),
        }
    }
    assert_eq!(trace, REQUIRED);
}
const REQUIRED: &[u32] = &[
    510, 510, 495, 433, 192, 435, 3, 438, 1, 419, 17, 310, 216, 314, 5, 436, 33, 486, 56, 0, 487,
    96, 0, 496, 26, 61, 19, 78, 119, 45, 48, 0, 40, 12, 11, 21, 21, 15, 497, 26, 42, 36, 62, 105,
    45, 48, 0, 10, 13, 13, 20, 19, 15, 510, 511, 309, 0, 324, 0, 438, 1, 419, 11, 289, 296, 273,
    65686, 297, 65656,
];

#[test]
fn test_should_use_the_original_runtime_spi_mode_zero() {
    assert_eq!(
        (
            crate::profiles::legacy_128::SPI_POLARITY,
            crate::profiles::legacy_128::SPI_PHASE
        ),
        (false, false)
    );
}

#[derive(Debug, PartialEq)]
enum ResetEvent {
    Level(bool),
    Delay(u16),
}
struct Pin<'a>(
    &'a std::cell::RefCell<std::vec::Vec<ResetEvent>>,
    usize,
    usize,
);
impl device_core::OutputPin for Pin<'_> {
    type Error = usize;
    fn set_level(&mut self, high: bool) -> Result<(), usize> {
        self.1 += 1;
        self.0.borrow_mut().push(ResetEvent::Level(high));
        if self.1 == self.2 {
            Err(self.1)
        } else {
            Ok(())
        }
    }
}
struct Delay<'a>(&'a std::cell::RefCell<std::vec::Vec<ResetEvent>>);
impl device_core::DelayMs for Delay<'_> {
    fn delay_ms(&mut self, ms: u16) {
        self.0.borrow_mut().push(ResetEvent::Delay(ms));
    }
}
#[test]
fn reset_preserves_high_low_high_and_delays() {
    let events = std::cell::RefCell::default();
    assert_eq!(
        crate::profiles::legacy_128::reset(&mut Pin(&events, 0, 0), &mut Delay(&events)),
        Ok(())
    );
    assert_eq!(
        *events.borrow(),
        [
            ResetEvent::Level(true),
            ResetEvent::Delay(50),
            ResetEvent::Level(false),
            ResetEvent::Delay(50),
            ResetEvent::Level(true),
            ResetEvent::Delay(120)
        ]
    );
}
#[test]
fn reset_stops_before_delay_when_gpio_fails() {
    for fail_at in 1..=3 {
        let events = std::cell::RefCell::default();
        assert_eq!(
            crate::profiles::legacy_128::reset(&mut Pin(&events, 0, fail_at), &mut Delay(&events)),
            Err(fail_at)
        );
        assert_eq!(events.borrow().len(), fail_at * 2 - 1);
        assert!(matches!(events.borrow().last(), Some(ResetEvent::Level(_))));
    }
}
