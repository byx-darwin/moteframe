//! Resource authorization is independent of the informational access state.
use core::cell::Cell;
use device_protocol::{Frame, Header, Kind, ModuleEntry, ProtocolLimits};
use device_services::{
    AccessSnapshot, AccessState, BuildProfile, Config, CoreRequest, CoreResourceHandler,
    CoreResources, DatasetIdentity, DeviceInfo, GrantRequest, ResourceFailure, Session,
    SessionError, SessionState, TrustedAccessProvider,
};
const CORE: ModuleEntry = ModuleEntry {
    module: 0,
    major: 1,
    minor: 2,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
struct Provider<'a> {
    allowed: &'a Cell<bool>,
    active: &'a Cell<bool>,
    identity: &'a Cell<[u8; 16]>,
    grants: u32,
}
impl TrustedAccessProvider for Provider<'_> {
    fn access_snapshot(&self) -> AccessSnapshot {
        AccessSnapshot {
            state: AccessState::Authorized,
            reason: 0,
        }
    }
}
impl CoreResources for Provider<'_> {
    fn authorized(&self, _: CoreRequest) -> bool {
        self.allowed.get()
    }
    fn validate(&self, request: CoreRequest) -> Result<(), ResourceFailure> {
        if let CoreRequest::Grant(_) = request
            && !self.active.get()
        {
            return Err(ResourceFailure::NotFound);
        }
        Ok(())
    }
    fn grant(&mut self, request: GrantRequest) -> Result<u32, ResourceFailure> {
        self.grants += 1;
        Ok(request.cumulative)
    }
    fn dataset(&mut self) -> Result<DatasetIdentity, ResourceFailure> {
        DatasetIdentity::new(self.identity.get())
    }
    fn replay_valid(&self, request: CoreRequest, body: &[u8]) -> bool {
        match request {
            CoreRequest::Grant(_) => self.active.get(),
            CoreRequest::Dataset => body == self.identity.get(),
        }
    }
}
fn request(op: u8, id: u16, b: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0,
            opcode: op,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(b.len()).unwrap_or_else(|e| panic!("{e:?}")),
        },
        payload: b,
    }
}
fn session() -> Session<'static> {
    static ENTRIES: [ModuleEntry; 1] = [CORE];
    let mut s = Session::new(Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Development,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 0,
        },
        modules: &ENTRIES,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 128,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    })
    .unwrap_or_else(|e| panic!("{e:?}"));
    s.control_notifications_enabled()
        .unwrap_or_else(|e| panic!("{e:?}"));
    s.dispatch(request(1, 1, &[1, 1, 128, 0, 128, 0, 3, 0]), &mut [0; 128])
        .unwrap_or_else(|e| panic!("{e:?}"));
    s
}
#[test]
fn test_should_grant_actual_stream_once_and_reject_cached_success_after_context_ends() {
    let allowed = Cell::new(true);
    let active = Cell::new(true);
    let identity = Cell::new([1; 16]);
    let mut p = Provider {
        allowed: &allowed,
        active: &active,
        identity: &identity,
        grants: 0,
    };
    let mut h = CoreResourceHandler::new(CORE, &mut p).unwrap_or_else(|e| panic!("{e:?}"));
    let mut s = session();
    let mut out = [0; 128];
    assert_eq!(
        s.dispatch_with_handler(request(7, 2, &[1, 9, 0, 3, 0, 0, 0]), &mut h, &mut out),
        Ok(11)
    );
    assert_eq!(&out[..11], &[0, 0, 0, 0, 1, 9, 0, 3, 0, 0, 0]);
    assert_eq!(
        s.dispatch_with_handler(request(7, 2, &[1, 9, 0, 3, 0, 0, 0]), &mut h, &mut out),
        Ok(11)
    );
    active.set(false);
    assert_eq!(
        s.dispatch_with_handler(request(7, 2, &[1, 9, 0, 3, 0, 0, 0]), &mut h, &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(s.state(), SessionState::Closed);

    assert_eq!(p.grants, 1);
}
#[test]
fn test_should_never_release_cached_dataset_through_no_handler_or_changed_identity() {
    let allowed = Cell::new(true);
    let active = Cell::new(true);
    let identity = Cell::new([1; 16]);
    let mut p = Provider {
        allowed: &allowed,
        active: &active,
        identity: &identity,
        grants: 0,
    };
    let mut h = CoreResourceHandler::new(CORE, &mut p).unwrap_or_else(|e| panic!("{e:?}"));
    let mut s = session();
    let mut out = [0; 128];
    assert_eq!(
        s.dispatch_with_handler(request(8, 2, &[]), &mut h, &mut out),
        Ok(20)
    );
    assert_eq!(&out[4..20], &[1; 16]);
    assert_eq!(s.dispatch(request(8, 2, &[]), &mut out), Ok(4));
    assert_eq!(&out[..4], &[5, 0, 3, 255]);
    identity.set([2; 16]);
    assert!(
        s.dispatch_with_handler(request(8, 2, &[]), &mut h, &mut out)
            .is_err()
    );
    assert_eq!(s.state(), SessionState::Closed);
}
#[test]
fn test_should_not_treat_authorized_info_as_scope_or_accept_invalid_identity() {
    let allowed = Cell::new(false);
    let active = Cell::new(true);
    let identity = Cell::new([1; 16]);
    let mut p = Provider {
        allowed: &allowed,
        active: &active,
        identity: &identity,
        grants: 0,
    };
    let mut h = CoreResourceHandler::new(CORE, &mut p).unwrap_or_else(|e| panic!("{e:?}"));
    let mut s = session();
    let mut out = [0; 128];
    assert_eq!(
        s.dispatch_with_handler(request(6, 2, &[]), &mut h, &mut out),
        Ok(7)
    );
    assert_eq!(out[4], 2);
    assert_eq!(
        s.dispatch_with_handler(request(8, 3, &[]), &mut h, &mut out),
        Ok(4)
    );
    assert_eq!(out[0], 5);
    assert!(DatasetIdentity::new([0; 16]).is_err());
}

#[cfg(not(feature = "raw-data"))]
#[test]
fn test_should_remove_raw_credit_alias_from_default_core_handler_codec() {
    assert!(CoreRequest::decode(7, &[2, 1, 0, 1, 0, 0, 0]).is_err());
}
