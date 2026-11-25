//! Packet ownership, wire bytes and bounded queue behavior.
#![allow(
    clippy::unwrap_used,
    reason = "Unexpected setup failure aborts the test"
)]
use ble_controller::{FrameKind, LinkFrame, OwnedQueue};

#[test]
fn test_should_preserve_owned_payload_and_set_then_clear_only_the_md_bit() {
    let mut bytes = [1, 0, 4, 0, 0x13];
    let frame = LinkFrame::new(FrameKind::Start, &bytes).unwrap();
    bytes[4] = 0xff;
    assert_eq!(bytes[4], 0xff);
    assert_eq!(
        frame.with_more_data(true).as_pdu(),
        &[0x12, 5, 1, 0, 4, 0, 0x13]
    );
    assert_eq!(
        frame.with_more_data(true).with_more_data(false).as_pdu(),
        &[2, 5, 1, 0, 4, 0, 0x13]
    );
    let control = LinkFrame::new(FrameKind::Control, &[7, 0x14]).unwrap();
    assert_eq!(control.with_more_data(true).as_pdu(), &[0x13, 2, 7, 0x14]);
}

#[test]
fn test_should_reject_invalid_link_payloads_and_keep_empty_continuations_explicit() {
    assert!(LinkFrame::new(FrameKind::Control, &[]).is_err());
    assert!(LinkFrame::new(FrameKind::Start, &[0; 3]).is_err());
    assert!(LinkFrame::new(FrameKind::Continuation, &[0; 28]).is_err());
    assert_eq!(
        LinkFrame::new(FrameKind::Continuation, &[])
            .unwrap()
            .as_pdu(),
        &[1, 0]
    );
    assert_eq!(
        LinkFrame::new(FrameKind::Continuation, &[9])
            .unwrap()
            .as_pdu(),
        &[1, 1, 9]
    );
}

#[test]
fn test_should_keep_bounded_fifo_order_through_full_and_ring_wrap() {
    let mut q = OwnedQueue::<u8, 2>::new();
    q.push(10).unwrap();
    q.push(11).unwrap();
    assert!(q.push(12).is_err());
    assert_eq!(q.front(), Some(&10));
    assert_eq!(q.pop(), Some(10));
    q.push(12).unwrap();
    assert_eq!(q.pop(), Some(11));
    assert_eq!(q.pop(), Some(12));
    assert!(q.is_empty());
    q.push(13).unwrap();
    q.clear();
    assert_eq!(q.pop(), None);
    assert!(OwnedQueue::<u8, 0>::new().push(1).is_err());
}
