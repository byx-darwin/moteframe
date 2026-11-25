//! Exact action body tests. They do not execute hardware actions.
#![cfg(all(
    feature = "system",
    feature = "action-find",
    feature = "action-media",
    feature = "action-phone",
    feature = "action-call",
    feature = "action-notification"
))]
use device_application::actions::{Action, ActionError, DeferredSystem, SystemBackend, SystemScope};
#[test]
fn test_should_decode_known_actions_and_reject_unknown_enums_or_trailing_bytes() {
    for (module, opcode, b) in [
        (2, 0x30, vec![3, 0, 0, 0]),
        (2, 0x31, vec![3, 0, 0, 0]),
        (2, 0x32, vec![]),
        (6, 0x30, vec![0, 3, 30, 0]),
        (6, 0x31, vec![1, 0, 0, 0]),
        (6, 0x32, vec![1, 0, 0, 0, 1]),
        (7, 0x30, vec![0x78, 0x56, 0x34, 0x12]),
        (8, 0x30, vec![1]),
        (0x30, 0x30, vec![1, 0, 0, 0, 1, 1]),
        (0x33, 0x30, vec![1]),
        (0x33, 0x31, vec![1, 0, 0, 0, 0]),
        (0x33, 0x32, vec![5]),
        (0x33, 0x33, vec![1, 0, 0, 0, 3]),
        (0x33, 0x34, vec![1, 0, 0, 0, 1, 1, 1, 90, 0, 0, 0]),
    ] {
        let a = Action::decode(module, opcode, &b).unwrap_or_else(|e| panic!("{e:?}"));
        let mut out = [0; 32];
        let n = a.encode(&mut out).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(&out[..n], b);
    }
    for (module, opcode, b) in [
        (2, 0x30, vec![0; 4]),
        (6, 0x30, vec![2, 3, 30, 0]),
        (6, 0x30, vec![0, 0, 30, 0]),
        (6, 0x31, vec![0; 4]),
        (8, 0x30, vec![2]),
        (0x33, 0x32, vec![0]),
        (0x33, 0x33, vec![1, 0, 0, 0, 4]),
        (0x33, 0x34, vec![0; 7]),
        (0x33, 0x30, vec![1, 0]),
    ] {
        assert!(Action::decode(module, opcode, &b).is_err());
    }
}
#[derive(Debug, Default)]
struct Hardware {
    scheduled: Vec<(bool, u32)>,
}
impl SystemBackend for Hardware {
    fn schedule(&mut self, reboot: bool, delay_s: u32) -> Result<(), ActionError> {
        self.scheduled.push((reboot, delay_s));
        Ok(())
    }
}
struct Scope(bool);
impl SystemScope for Scope {
    fn allowed(&self) -> bool {
        self.0
    }
}
#[test]
fn test_should_defer_reboot_until_response_admitted_and_never_replay_after_revoke() {
    let mut engine = DeferredSystem::new();
    let mut hw = Hardware::default();
    let a = Action::decode(2, 0x30, &[3, 0, 0, 0]).unwrap_or_else(|e| panic!("{e:?}"));
    let ticket = engine
        .prepare(a, &Scope(true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(hw.scheduled.is_empty());
    assert_eq!(
        engine.confirm_response(ticket, &Scope(false), &mut hw),
        Err(ActionError::Denied)
    );
    assert!(hw.scheduled.is_empty());
    assert_eq!(
        engine.confirm_response(ticket, &Scope(true), &mut hw),
        Err(ActionError::InvalidState)
    );
    let ticket = engine
        .prepare(a, &Scope(true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    engine
        .confirm_response(ticket, &Scope(true), &mut hw)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(hw.scheduled, [(true, 3)]);
    assert_eq!(
        engine.confirm_response(ticket, &Scope(true), &mut hw),
        Err(ActionError::InvalidState)
    );
    let ticket = engine
        .prepare(a, &Scope(true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    engine
        .response_failed(ticket)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(hw.scheduled.len(), 1);
}
