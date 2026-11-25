//! Core routes must share the same stream and persistent identity owners as business handlers.
use core::cell::Cell;
use device_application::{CreditPort, DatasetPort, ResourceRouter};
use device_protocol::DataKind;
use device_services::{
    AccessSnapshot, AccessState, CoreRequest, CoreResources, DatasetIdentity, GrantRequest,
    ResourceFailure, TrustedAccessProvider,
};
#[derive(Debug)]
struct Info;
impl TrustedAccessProvider for Info {
    fn access_snapshot(&self) -> AccessSnapshot {
        AccessSnapshot {
            state: AccessState::Authorized,
            reason: 0,
        }
    }
}
#[derive(Debug)]
struct Credit {
    scope: Cell<bool>,
    stream: Cell<u16>,
    total: u32,
}
impl CreditPort for Credit {
    fn authorized(&self) -> bool {
        self.scope.get()
    }
    fn validate(&self, id: u16, total: u32) -> Result<(), ResourceFailure> {
        if id != self.stream.get() {
            Err(ResourceFailure::NotFound)
        } else if total < self.total {
            Err(ResourceFailure::Conflict)
        } else {
            Ok(())
        }
    }
    fn grant(&mut self, id: u16, total: u32) -> Result<u32, ResourceFailure> {
        self.validate(id, total)?;
        self.total = total;
        Ok(total)
    }
    fn replay_valid(&self, id: u16, accepted: u32) -> bool {
        id == self.stream.get() && accepted <= self.total
    }
}
#[derive(Debug)]
struct Dataset {
    scope: bool,
    id: [u8; 16],
}
impl DatasetPort for Dataset {
    fn authorized(&self) -> bool {
        self.scope
    }
    fn identity(&self) -> Result<DatasetIdentity, ResourceFailure> {
        DatasetIdentity::new(self.id)
    }
}
#[test]
fn test_should_route_history_credit_and_dataset_to_actual_owners_without_using_info_as_scope() {
    let mut credits = Credit {
        scope: Cell::new(true),
        stream: Cell::new(7),
        total: 0,
    };
    let mut dataset = Dataset {
        scope: false,
        id: [1; 16],
    };
    let mut router = ResourceRouter::new(&Info, &mut credits, &mut dataset);
    assert!(!router.authorized(CoreRequest::Dataset));
    assert_eq!(router.dataset(), Err(ResourceFailure::AccessDenied));
    let request = GrantRequest {
        kind: DataKind::History,
        stream_id: 7,
        cumulative: 3,
    };
    assert!(router.authorized(CoreRequest::Grant(request)));
    assert_eq!(router.grant(request), Ok(3));
    assert!(router.replay_valid(CoreRequest::Grant(request), &[1, 7, 0, 3, 0, 0, 0]));
    assert!(!router.replay_valid(CoreRequest::Grant(request), &[1, 7, 0, 4, 0, 0, 0]));
    assert!(!router.replay_valid(CoreRequest::Grant(request), &[2, 7, 0, 3, 0, 0, 0]));
    assert_eq!(
        router.validate(CoreRequest::Grant(GrantRequest {
            stream_id: 8,
            ..request
        })),
        Err(ResourceFailure::NotFound)
    );
    assert_eq!(
        router.grant(GrantRequest {
            cumulative: 2,
            ..request
        }),
        Err(ResourceFailure::Conflict)
    );
    assert_eq!(credits.total, 3);
}
#[test]
fn test_should_read_verified_nonzero_identity_and_reject_wrong_cached_binding() {
    let mut credits = Credit {
        scope: Cell::new(false),
        stream: Cell::new(7),
        total: 0,
    };
    let mut dataset = Dataset {
        scope: true,
        id: [9; 16],
    };
    let mut router = ResourceRouter::new(&Info, &mut credits, &mut dataset);
    assert_eq!(router.dataset().map(|id| *id.bytes()), Ok([9; 16]));
    assert!(router.replay_valid(CoreRequest::Dataset, &[9; 16]));
    assert!(!router.replay_valid(CoreRequest::Dataset, &[8; 16]));
    assert!(!router.authorized(CoreRequest::Grant(GrantRequest {
        kind: DataKind::History,
        stream_id: 7,
        cumulative: 1
    })));
}
