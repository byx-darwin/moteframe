//! Actual Runtime/Session tests using only a bounded test transport, never linked into the image.
#![allow(clippy::unwrap_used)]
use device_connection::{FirstFragmentQueue, FirstSlotError, FirstSlotOutcome};
use device_runtime::{Budgets, OwnedRx, PortError, Runtime, Scope, Transport};
use device_services::DeviceInfo;
use wattch5_bringup::{CoreFactory, UnboundStorage};
#[derive(Debug)]
struct Wire {
    scope: Option<Scope>,
    rx: Option<OwnedRx>,
    sent: Vec<Vec<u8>>,
}
#[derive(Debug)]
struct Queue<'a>(&'a mut Wire, Scope);
impl FirstFragmentQueue for Queue<'_> {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        if self.0.scope != Some(self.1) {
            return Err(FirstSlotError::Contract);
        }
        let mut out = [0; 20];
        let (n, v) = fill(&mut out).map_err(FirstSlotError::Fill)?;
        self.0.sent.push(out[..n].to_vec());
        Ok(FirstSlotOutcome::Admitted(v))
    }
}
impl Transport for Wire {
    type Queue<'a> = Queue<'a>;
    fn poll(&mut self, _: u64, _: u8) -> Result<(), PortError> {
        Ok(())
    }
    fn scope(&self) -> Result<Option<Scope>, PortError> {
        Ok(self.scope)
    }
    fn receive(&mut self) -> Result<Option<OwnedRx>, PortError> {
        Ok(self.rx.take())
    }
    fn queue(&mut self, s: Scope) -> Result<Queue<'_>, PortError> {
        Ok(Queue(self, s))
    }
    fn close_scope(&mut self, s: Scope) -> Result<(), PortError> {
        if self.scope == Some(s) {
            self.scope = None;
        }
        Ok(())
    }
}
fn factory() -> CoreFactory {
    CoreFactory::new(DeviceInfo {
        product_id: 0,
        fw_major: 0,
        fw_minor: 1,
        fw_patch: 0,
        build_id: 1,
    })
    .unwrap()
}
fn turns(rt: &mut Runtime<'_, CoreFactory>, w: &mut Wire, time: &mut u64, n: u64) {
    for _ in 0..n {
        let r = rt.step(*time, w, &mut UnboundStorage, &mut UnboundStorage);
        assert!(r.connection.is_ok(), "{r:?}");
        assert_eq!(r.background, Err(PortError::Unavailable));
        *time += 1;
    }
}
fn request(
    rt: &mut Runtime<'_, CoreFactory>,
    w: &mut Wire,
    time: &mut u64,
    module: u8,
    opcode: u8,
    id: u16,
    payload: &[u8],
) -> Vec<u8> {
    let mut bytes = vec![
        0x10,
        module,
        opcode,
        0,
        id as u8,
        (id >> 8) as u8,
        0,
        0,
        payload.len() as u8,
        0,
    ];
    bytes.extend(payload);
    w.rx = Some(OwnedRx::new(w.scope.unwrap(), &bytes).unwrap());
    w.sent.clear();
    turns(rt, w, time, 5);
    let mut body = vec![];
    for (i, v) in w.sent.iter().enumerate() {
        let f = device_protocol::Frame::decode(v, 20, 128).unwrap();
        assert_eq!(f.header.message_id, id);
        assert_eq!(f.header.offset as usize, body.len());
        assert!(v.len() <= 20);
        body.extend(f.payload);
        assert!(i < 4);
    }
    body
}
#[test]
fn test_should_dispatch_only_anonymous_core_and_fragment_actual_responses() {
    let f = factory();
    let mut rt = Runtime::new(&f, Budgets::new(1, 1, 2).unwrap());
    let mut w = Wire {
        scope: Some(Scope::new(1, 20).unwrap()),
        rx: None,
        sent: vec![],
    };
    let mut time = 0;
    let hello = request(
        &mut rt,
        &mut w,
        &mut time,
        0,
        1,
        1,
        &[1, 1, 128, 0, 0, 0, 3, 0],
    );
    assert_eq!(hello.len(), 22);
    assert_eq!(w.sent.len(), 3);
    let hello = device_protocol::HelloResponse::decode(&hello[4..]).unwrap();
    assert_eq!(hello.session, 1);
    assert_eq!(hello.data_max, 0);
    assert_eq!(hello.access, 1);
    assert_eq!(hello.profile, 1);
    let info = request(&mut rt, &mut w, &mut time, 0, 2, 2, &[]);
    assert_eq!(DeviceInfo::LENGTH, 14);
    assert_eq!(DeviceInfo::decode(&info[4..]).unwrap().product_id, 0);
    let modules = request(&mut rt, &mut w, &mut time, 0, 3, 3, &[0, 0, 8]);
    assert_eq!(modules.len(), 23);
    assert_eq!(modules[11], 0);
    assert_eq!(modules[15..19], [0; 4]);
    let limits = request(&mut rt, &mut w, &mut time, 0, 4, 4, &[]);
    assert_eq!(&limits[..4], &[0, 0, 0, 0]);
    let ping = request(&mut rt, &mut w, &mut time, 0, 5, 5, &[9, 8, 7, 6]);
    assert_eq!(&ping[4..], &[9, 8, 7, 6]);
    let access = request(&mut rt, &mut w, &mut time, 0, 6, 6, &[]);
    assert_eq!(access[4], 1);
    for (id, module, op) in [(7, 0, 7), (8, 0, 8), (9, 2, 0x30), (10, 0, 9)] {
        let reply = request(&mut rt, &mut w, &mut time, module, op, id, &[]);
        assert_ne!(reply[0], 0);
    }
}
#[test]
fn test_should_open_fresh_runtime_session_after_cccd_off_on_and_reconnect() {
    let f = factory();
    let mut rt = Runtime::new(&f, Budgets::new(1, 1, 2).unwrap());
    let mut w = Wire {
        scope: None,
        rx: None,
        sent: vec![],
    };
    let mut time = 0;
    for generation in 1..=3 {
        w.scope = Some(Scope::new(generation, 20).unwrap());
        let body = request(
            &mut rt,
            &mut w,
            &mut time,
            0,
            1,
            1,
            &[1, 1, 128, 0, 0, 0, 3, 0],
        );
        assert_eq!(
            device_protocol::HelloResponse::decode(&body[4..])
                .unwrap()
                .session,
            generation as u32
        );
        w.scope = None;
        turns(&mut rt, &mut w, &mut time, 1);
    }
}
