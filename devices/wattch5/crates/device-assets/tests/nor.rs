//! Exact read-only command allowlist and bounds, with no program or erase API.
use device_assets::{ReadAt, ReadOnlyNor};
use device_core::DelayMs;
use embedded_hal::spi::{ErrorType, Operation, SpiDevice};
use std::{cell::RefCell, rc::Rc};
#[derive(Debug)]
struct Bus {
    commands: Rc<RefCell<Vec<Vec<u8>>>>,
    id: [u8; 3],
    command: u8,
}
impl ErrorType for Bus {
    type Error = embedded_hal::spi::ErrorKind;
}
impl SpiDevice for Bus {
    fn transaction(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for op in ops {
            match op {
                Operation::Write(data) => {
                    self.command = data[0];
                    self.commands.borrow_mut().push(data.to_vec());
                }
                Operation::Read(out) => {
                    if self.command == 0x9f {
                        out.copy_from_slice(&self.id);
                    } else {
                        out.fill(0x55);
                    }
                }
                _ => return Err(Self::Error::Other),
            }
        }
        Ok(())
    }
}
#[derive(Debug)]
struct Delay;
impl DelayMs for Delay {
    fn delay_ms(&mut self, ms: u16) {
        assert!(ms >= 1);
    }
}
#[test]
fn test_should_only_wake_identify_and_read_bounded_24bit_ranges() {
    let commands = Rc::default();
    let dev = Bus {
        commands: Rc::clone(&commands),
        id: [0xef, 0x40, 0x16],
        command: 0,
    };
    let mut nor = ReadOnlyNor::open(dev, &mut Delay).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(nor.capacity(), 0x0040_0000);
    assert_eq!(nor.id(), [0xef, 0x40, 0x16]);
    let mut out = [0; 3];
    nor.read_exact(0x12_3456, &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, [0x55; 3]);
    assert_eq!(
        *commands.borrow(),
        [vec![0xab], vec![0x9f], vec![3, 0x12, 0x34, 0x56]]
    );
    let n = commands.borrow().len();
    assert!(nor.read_exact(nor.capacity() - 1, &mut out).is_err());
    assert!(nor.read_exact(0, &mut [0; 257]).is_err());
    assert!(nor.read_exact(nor.capacity(), &mut []).is_ok());
    assert_eq!(commands.borrow().len(), n);
}
#[test]
fn test_should_reject_blank_identity_and_unsupported_address_capacity() {
    for id in [[0; 3], [255; 3], [0xef, 0x40, 0x19], [0xef, 0, 0x16]] {
        let dev = Bus {
            commands: Rc::default(),
            id,
            command: 0,
        };
        assert!(ReadOnlyNor::open(dev, &mut Delay).is_err());
    }
}
