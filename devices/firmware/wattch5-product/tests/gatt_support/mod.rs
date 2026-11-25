//! Scripted link fixture only; the production upstream Host and GATT remain real.
#![allow(clippy::unwrap_used)]
use ble_controller::{
    ConnectionParameters, FrameKind, LinkBackend, LinkEvent, LinkFrame, Peer, TxAdmission,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
#[derive(Debug, Default)]
pub struct State {
    pub advertised: bool,
    pub events: VecDeque<LinkEvent>,
    pub sent: Vec<LinkFrame>,
}
#[derive(Clone, Debug, Default)]
pub struct Backend(pub Rc<RefCell<State>>);
impl LinkBackend for Backend {
    type Error = ();
    fn reset(&mut self) -> Result<(), ()> {
        self.0.borrow_mut().events.clear();
        Ok(())
    }
    fn advertise(&mut self, _: &ble_controller::Advertisement) -> Result<(), ()> {
        self.0.borrow_mut().advertised = true;
        Ok(())
    }
    fn stop(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<LinkEvent>, ()> {
        Ok(self.0.borrow_mut().events.pop_front())
    }
    fn transmit(&mut self, t: u32, p: LinkFrame) -> Result<TxAdmission, ()> {
        let mut s = self.0.borrow_mut();
        s.sent.push(p);
        s.events.push_back(LinkEvent::Acknowledged(t));
        Ok(TxAdmission::Committed)
    }
    fn connection_update(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
    fn channel_map(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}
pub fn inject(b: &Backend, att: &[u8]) {
    let mut payload = vec![u8::try_from(att.len()).unwrap(), 0, 4, 0];
    payload.extend_from_slice(att);
    b.0.borrow_mut().events.push_back(LinkEvent::Received(
        LinkFrame::new(FrameKind::Start, &payload).unwrap(),
    ));
}
pub fn count(b: &Backend, opcode: u8) -> usize {
    b.0.borrow()
        .sent
        .iter()
        .filter(|f| f.kind() == FrameKind::Start && f.payload().get(4) == Some(&opcode))
        .count()
}

impl Backend {
    pub fn connect(&self) {
        self.0.borrow_mut().events.push_back(LinkEvent::Connected(
            Peer::new([7; 6], true),
            ConnectionParameters::new(24, 0, 72).unwrap(),
        ));
    }
    pub fn disconnect(&self) {
        self.0
            .borrow_mut()
            .events
            .push_back(LinkEvent::Disconnected(0x13));
    }
    pub fn att(&self, opcode: u8) -> Vec<Vec<u8>> {
        self.0
            .borrow()
            .sent
            .iter()
            .filter(|f| f.kind() == FrameKind::Start && f.payload().get(4) == Some(&opcode))
            .map(|f| f.payload()[4..].to_vec())
            .collect()
    }
}

pub mod medium;
pub mod product;

// Upstream's default packet pool is global; real Host tests must not compete for it.
static HOST_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn serialized() -> std::sync::MutexGuard<'static, ()> {
    HOST_TEST.lock().unwrap()
}
