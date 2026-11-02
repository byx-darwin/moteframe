//! Debug must not disclose current or logically removed application values.
#![allow(clippy::unwrap_used)]
use moteframe_ble::WriteSlot;

#[derive(Clone, Copy)]
struct PrivateTarget;

#[test]
fn debug_does_not_require_or_call_target_debug() {
    let mut slot = WriteSlot::<PrivateTarget, 1>::new();
    slot.push(PrivateTarget, &[211]).unwrap();
    assert!(!format!("{slot:?}").contains("211"));
}

#[test]
fn debug_omits_current_cleared_taken_and_reused_payloads() {
    let mut slot = WriteSlot::<u32, 8>::new();
    slot.push(987_654_321, &[211, 212, 213]).unwrap();
    for state in 0..4 {
        let text = format!("{slot:?}");
        assert!(!text.contains("211"), "payload was exposed: {text}");
        assert!(!text.contains("987654321"), "target was exposed: {text}");
        match state {
            0 => {
                slot.clear();
            }
            1 => {
                slot.push(987_654_321, &[7]).unwrap();
            }
            2 => {
                slot.take_into(&mut [0; 8]).unwrap();
            }
            _ => {}
        }
    }
}
