//! Reminder object validation does not schedule a physical alert.
#![cfg(feature = "reminders")]
use device_models::Reminder;
fn one_shot() -> Vec<u8> {
    let mut b = vec![1, 0, 0xe8, 3, 0, 0, 0, 0, 0, 0];
    b.extend_from_slice(&[0; 22]);
    b.extend_from_slice(&[3, 0, b'A', b'B', b'C']);
    b
}
#[test]
fn test_should_roundtrip_one_shot_and_reject_unused_fields_or_bad_strings() {
    let b = one_shot();
    let r = Reminder::decode(1, &b, 20).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(r.title, "ABC");
    let mut out = [0; 64];
    let n = r.encode(&mut out, 20).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(&out[..n], b);
    assert!(!format!("{r:?}").contains("ABC"));
    assert!(Reminder::decode(1, &b, 2).is_err());
    let mut bad = b.clone();
    bad[10] = 1;
    assert!(Reminder::decode(1, &bad, 20).is_err());
    let mut bad = b.clone();
    bad[34] = 255;
    assert!(Reminder::decode(1, &bad, 20).is_err());
    let mut bad = b.clone();
    bad[35] = 0;
    assert!(Reminder::decode(1, &bad, 20).is_err());
    assert!(Reminder::decode(5, &b, 20).is_err());
    assert!(Reminder::decode(7, &b, 20).is_err());
}
#[test]
fn test_should_validate_periodic_sedentary_without_enabling_other_alarm_kinds() {
    let mut b = one_shot();
    b[1] = 2;
    b[2..10].fill(0);
    b[12] = 60;
    b[16] = 1;
    b[17] = 0x7f;
    b[28] = 10;
    assert!(Reminder::decode(6, &b, 20).is_ok());
    assert!(Reminder::decode(5, &b, 20).is_err());
    assert!(Reminder::decode(1, &b, 20).is_err());
    b[28] = 0;
    assert!(Reminder::decode(5, &b, 20).is_ok());
    b[12] = 0;
    assert!(Reminder::decode(5, &b, 20).is_err());
}
#[test]
fn test_should_keep_unused_single_shot_work_window_canonical_despite_general_window_toggle() {
    let mut b = one_shot();
    b[17] = 1;
    assert!(Reminder::decode(1, &b, 20).is_err());
}
