//! Runtime admission uses the real inbox state machine, not a fake BLE success path.
use ble_syd881x::runtime::{Attribute, Backend, Error, Inbox, Profile, Runtime};
use device_ble::{Advertisement, Channel, Event, Peripheral};
use std::{cell::RefCell, rc::Rc};

fn profile() -> Profile {
    match Profile::new(
        0xefe0,
        Attribute::new(0xefe1, 0x1d),
        Attribute::new(0xefe3, 0x1f),
        0x20,
        Channel(7),
    ) {
        Ok(profile) => profile,
        Err(error) => panic!("profile: {error:?}"),
    }
}
#[derive(Debug)]
struct State {
    inbox: Inbox<2>,
    sent: Vec<Vec<u8>>,
    reject: bool,
    service_calls: usize,
}
#[derive(Debug)]
struct Stack(Rc<RefCell<State>>);
impl Backend for Stack {
    fn initialize(&mut self, _: Profile) -> Result<(), Error> {
        Ok(())
    }
    fn advertise(&mut self, _: &Advertisement<'_>) -> Result<(), Error> {
        Ok(())
    }
    fn state(&self) -> Result<ble_syd881x::runtime::LinkState, Error> {
        self.0.borrow().inbox.state()
    }
    fn send(&mut self, _: Profile, epoch: u32, data: &[u8]) -> Result<(), Error> {
        let mut state = self.0.borrow_mut();
        if state.reject {
            return Err(Error::VendorRejected);
        }
        if state.inbox.state()?.epoch() != epoch {
            return Err(Error::StaleConnection);
        }
        state.sent.push(data.to_vec());
        Ok(())
    }
    fn service(&mut self) -> Result<(), Error> {
        self.0.borrow_mut().service_calls += 1;
        Ok(())
    }
    fn next_event(&mut self) -> Option<Event> {
        self.0.borrow_mut().inbox.pop()
    }
}
fn setup() -> (Runtime<Stack>, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State {
        inbox: Inbox::new(profile()),
        sent: vec![],
        reject: false,
        service_calls: 0,
    }));
    let runtime = Runtime::new(Stack(state.clone()), profile());
    match runtime {
        Ok(runtime) => (runtime, state),
        Err(error) => panic!("initialization: {error:?}"),
    }
}
fn connected(state: &Rc<RefCell<State>>) {
    assert!(state.borrow_mut().inbox.ingest(&[0, 2, 0, 0, 0]).is_ok());
}
fn subscribed(state: &Rc<RefCell<State>>) {
    assert!(
        state
            .borrow_mut()
            .inbox
            .ingest(&[0, 0, 0x20, 0, 0, 0xe0, 0xef, 0x20, 0, 1, 0])
            .is_ok()
    );
}
#[test]
fn test_should_reject_disconnected_unsubscribed_wrong_channel_and_long_notifications() {
    let (mut runtime, state) = setup();
    assert_eq!(runtime.notify(Channel(7), &[1]), Err(Error::Disconnected));
    connected(&state);
    assert_eq!(runtime.notify(Channel(7), &[1]), Err(Error::NotSubscribed));
    subscribed(&state);
    assert_eq!(runtime.notify(Channel(8), &[1]), Err(Error::UnknownChannel));
    assert_eq!(
        runtime.notify(Channel(7), &[1; 21]),
        Err(Error::PayloadTooLarge)
    );
    assert!(runtime.notify(Channel(7), &[9, 8]).is_ok());
    assert_eq!(state.borrow().sent, vec![vec![9, 8]]);
    assert!(runtime.service().is_ok());
    assert_eq!(state.borrow().service_calls, 1);
}
#[test]
fn test_should_revoke_subscription_before_disconnect_is_drained() {
    let (mut runtime, state) = setup();
    connected(&state);
    subscribed(&state);
    assert!(
        state
            .borrow_mut()
            .inbox
            .ingest(&[0, 4, 0, 0, 0, 19])
            .is_ok()
    );
    assert_eq!(runtime.notify(Channel(7), &[]), Err(Error::Disconnected));
    assert_eq!(runtime.next_event(), Some(Event::Connected));
    assert_eq!(
        runtime.next_event(),
        Some(Event::Disconnected { reason: 19 })
    );
    connected(&state);
    assert_eq!(runtime.notify(Channel(7), &[]), Err(Error::NotSubscribed));
}
#[test]
fn test_should_latch_overflow_and_revoke_tx_even_if_disconnect_event_does_not_fit() {
    let (mut runtime, state) = setup();
    connected(&state);
    subscribed(&state);
    assert!(state.borrow_mut().inbox.ingest(&[0, 2, 0, 0, 0]).is_ok());
    assert_eq!(
        state.borrow_mut().inbox.ingest(&[0, 4, 0, 0, 0, 19]),
        Err(Error::EventOverflow)
    );
    assert_eq!(runtime.notify(Channel(7), &[]), Err(Error::EventOverflow));
    assert_eq!(runtime.service(), Err(Error::EventOverflow));
}
#[test]
fn test_should_filter_profile_and_ignore_indication_only_cccd() {
    let (mut runtime, state) = setup();
    connected(&state);
    assert!(
        state
            .borrow_mut()
            .inbox
            .ingest(&[0, 0, 0x20, 0, 0, 0xe0, 0xef, 0x20, 0, 2, 0])
            .is_ok()
    );
    assert_eq!(runtime.notify(Channel(7), &[]), Err(Error::NotSubscribed));
    assert!(
        state
            .borrow_mut()
            .inbox
            .ingest(&[0, 0, 2, 0, 0, 0xe0, 0xef, 0xe1, 0xef, 0x1d, 0, 2, 0, 9, 8])
            .is_ok()
    );
    assert_eq!(runtime.next_event(), Some(Event::Connected));
    assert!(matches!(
        runtime.next_event(),
        Some(Event::Received {
            channel: Channel(7),
            ..
        })
    ));
    assert!(
        state
            .borrow_mut()
            .inbox
            .ingest(&[0, 0, 2, 0, 0, 0xe1, 0xef, 0xe1, 0xef, 0x1d, 0, 0, 0])
            .is_ok()
    );
    assert_eq!(runtime.next_event(), None);
}
#[test]
fn test_should_propagate_vendor_rejection_without_automatic_duplicate_send() {
    let (mut runtime, state) = setup();
    connected(&state);
    subscribed(&state);
    state.borrow_mut().reject = true;
    assert_eq!(runtime.notify(Channel(7), &[1]), Err(Error::VendorRejected));
    assert!(state.borrow().sent.is_empty());
}
#[test]
fn test_should_reject_zero_handles_and_aliasing_profile() {
    assert!(
        Profile::new(
            0xefe0,
            Attribute::new(1, 0),
            Attribute::new(2, 3),
            4,
            Channel(0)
        )
        .is_err()
    );
    assert!(
        Profile::new(
            0xefe0,
            Attribute::new(1, 2),
            Attribute::new(2, 2),
            4,
            Channel(0)
        )
        .is_err()
    );
}

#[test]
fn test_should_never_overwrite_advertisement_storage_once_rom_may_retain_it() {
    use ble_syd881x::runtime::AdvertisementLock;
    let mut lock = AdvertisementLock::new();
    let first = Advertisement::new(&[2, 1, 6], &[], 160);
    let different = Advertisement::new(&[2, 1, 5], &[], 160);
    match (first, different) {
        (Ok(first), Ok(different)) => {
            assert_eq!(lock.bind(&first), Ok(true));
            assert_eq!(lock.bind(&first), Ok(false));
            assert_eq!(lock.bind(&different), Err(Error::AdvertisementLocked));
            assert_eq!(lock.bind(&first), Ok(false));
        }
        _ => panic!("bad fixture"),
    }
}

#[test]
fn test_should_latch_malformed_ingress_and_reject_zero_capacity_without_panicking() {
    let mut inbox = Inbox::<0>::new(profile());
    assert_eq!(inbox.ingest(&[0, 2, 0, 0, 0]), Err(Error::EventOverflow));
    let mut inbox = Inbox::<2>::new(profile());
    assert_eq!(inbox.ingest(&[0, 0, 2, 0]), Err(Error::InvalidEvent));
    assert_eq!(inbox.ingest(&[0, 2, 0, 0, 0]), Err(Error::InvalidEvent));
}

#[test]
fn test_should_restart_only_after_observed_advertising_end_while_disconnected() {
    let profile = profile();
    let mut inbox = ble_syd881x::runtime::Inbox::<4>::new(profile);
    assert_eq!(inbox.ingest(&[0, 1, 0, 0, 0]), Ok(()));
    assert_eq!(inbox.take_advertising_end(), Ok(true));
    assert_eq!(inbox.take_advertising_end(), Ok(false));
    assert_eq!(inbox.ingest(&[0, 2, 0, 0, 0]), Ok(()));
    assert_eq!(inbox.ingest(&[0, 1, 0, 0, 0]), Ok(()));
    assert_eq!(inbox.take_advertising_end(), Ok(false));
    let mut inbox = ble_syd881x::runtime::Inbox::<4>::new(profile);
    assert_eq!(inbox.ingest(&[0, 1, 0, 0, 0]), Ok(()));
    assert_eq!(inbox.ingest(&[0, 2, 0, 0, 0]), Ok(()));
    assert_eq!(inbox.take_advertising_end(), Ok(false));
    inbox.fail(ble_syd881x::runtime::Error::InvalidEvent);
    assert!(inbox.take_advertising_end().is_err());
}

#[test]
fn test_should_validate_chip_specific_advertising_channel_masks() {
    use ble_syd881x::runtime::AdvertisingChannels;
    for mask in 1..=7 {
        assert_eq!(
            AdvertisingChannels::new(mask).map(AdvertisingChannels::mask),
            Ok(mask)
        );
    }
    for mask in [0, 8, 0xff] {
        assert!(AdvertisingChannels::new(mask).is_err());
    }
}

#[test]
fn test_should_reply_to_only_complete_valid_sdk_mtu_requests_with_mtu23() {
    use ble_syd881x::runtime::mtu_response_for_att_event;
    assert_eq!(mtu_response_for_att_event(&[2, 2, 185, 0]), Ok(Some(23)));
    assert_eq!(mtu_response_for_att_event(&[2, 2, 23, 0]), Ok(Some(23)));
    assert_eq!(mtu_response_for_att_event(&[3, 2]), Ok(None));
    for bytes in [
        &[][..],
        &[2][..],
        &[2, 1, 23][..],
        &[2, 2, 23][..],
        &[2, 2, 22, 0][..],
    ] {
        assert_eq!(mtu_response_for_att_event(bytes), Err(Error::InvalidEvent));
    }
}
