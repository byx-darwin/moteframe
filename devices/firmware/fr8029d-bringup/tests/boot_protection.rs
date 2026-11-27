#[path = "../src/boot_protection.rs"]
mod boot_protection;

fn run(script: &[(u32, u32, u32)], expected: bool) {
    let mut steps = script.iter().copied();
    let ok = boot_protection::ensure(|op, data| {
        let (want_op, want_data, reply) = steps.next().unwrap();
        assert_eq!((op, data), (want_op, want_data));
        reply
    });
    assert_eq!(ok, expected);
    assert_eq!(steps.next(), None);
}

const ID: (u32, u32, u32) = (0x9fa0_0000, 0, 0x146085);
const QE: (u32, u32, u32) = (0x3580_0000, 0, 2);
const LOCKED: (u32, u32, u32) = (0x0580_0000, 0, 0x7c);
const OPEN: (u32, u32, u32) = (0x0580_0000, 0, 0);
const ARM: (u32, u32, u32) = (0x5000_0000, 0, 0);
const LOCK: (u32, u32, u32) = (0x0100_8000, 0x7c, 0);

#[test]
fn accepts_existing_protection_without_writing() {
    run(&[ID, QE, LOCKED], true);
}

#[test]
fn restores_rom_state_and_verifies_both_registers() {
    run(&[ID, QE, OPEN, ARM, LOCK, LOCKED, QE], true);
    run(&[ID, QE, OPEN, ARM, LOCK, OPEN], false);
    run(&[ID, QE, OPEN, ARM, LOCK, LOCKED, (QE.0, 0, 0)], false);
}

#[test]
fn unexpected_device_or_status_never_arms_a_write() {
    run(&[(ID.0, 0, 0)], false);
    run(&[ID, (QE.0, 0, 0)], false);
    for bits in [1, 2, 4, 0x80] {
        run(&[ID, QE, (OPEN.0, 0, bits)], false);
    }
}
