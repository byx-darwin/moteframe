//! BLE ownership and bounded buffering contracts.
use device_ble::{Advertisement, Event, EventQueue, Packet, QueueFull};

#[test]
fn test_should_own_received_bytes_and_bound_packet_size() {
    let mut source = [7; 20];
    let packet = Packet::try_from_slice(&source);
    source.fill(0);
    assert_eq!(
        packet.as_ref().map(Packet::as_slice),
        Ok([7; 20].as_slice())
    );
    assert!(Packet::try_from_slice(&[0; 245]).is_err());
    assert!(Packet::try_from_slice(&[]).is_ok());
}
#[test]
fn test_should_preserve_fifo_on_full_and_after_wrap() {
    let mut queue = EventQueue::<2>::new();
    assert_eq!(queue.push(Event::Connected), Ok(()));
    assert_eq!(queue.push(Event::Disconnected { reason: 8 }), Ok(()));
    assert_eq!(queue.push(Event::Connected), Err(QueueFull));
    assert_eq!(queue.pop(), Some(Event::Connected));
    assert_eq!(queue.push(Event::Connected), Ok(()));
    assert_eq!(queue.pop(), Some(Event::Disconnected { reason: 8 }));
    assert_eq!(queue.pop(), Some(Event::Connected));
    assert_eq!(queue.pop(), None);
    assert_eq!(
        EventQueue::<0>::new().push(Event::Connected),
        Err(QueueFull)
    );
}
#[test]
fn test_should_bound_legacy_advertisement_data() {
    assert!(Advertisement::new(&[0; 32], &[], 160).is_err());
    assert!(Advertisement::new(&[], &[0; 32], 160).is_err());
    assert!(Advertisement::new(&[], &[], 0).is_err());
    assert!(Advertisement::new(&[], &[], 31).is_err());
    assert!(Advertisement::new(&[], &[], 16385).is_err());
    assert!(Advertisement::new(&[2, 1, 6], &[], 160).is_ok());
}
