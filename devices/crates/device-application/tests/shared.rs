//! One service owner can be referenced by the command table and core DATA bridge without aliasing.
use device_application::{CellBusy, ServiceCell};
use device_services::{ServiceHandler, SessionError};
#[derive(Debug)]
struct Owner {
    closed: bool,
}
impl ServiceHandler for Owner {
    fn required_features(&self, _: &device_protocol::ModuleEntry, _: u8) -> Option<u32> {
        None
    }
    fn authorized(&self, _: device_protocol::Frame<'_>) -> bool {
        !self.closed
    }
    fn dispatch(
        &mut self,
        _: device_protocol::Frame<'_>,
        _: &mut [u8],
    ) -> Result<usize, SessionError> {
        Err(SessionError::InvalidHandlerResponse)
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.closed = true;
        Ok(())
    }
    fn must_end_session(&self) -> bool {
        self.closed
    }
}
#[test]
fn test_should_report_reentrant_borrow_without_panicking_and_keep_one_service_owner() {
    let owner = ServiceCell::new(Owner { closed: false });
    assert_eq!(
        owner.with_mut(|_| owner.with_mut(|_| ())),
        Ok(Err(CellBusy))
    );
    let mut handle = owner.handle();
    assert!(!handle.must_end_session());
    assert_eq!(handle.connection_closed(), Ok(()));
    assert_eq!(owner.with(|inner| inner.closed), Ok(true));
    assert!(owner.handle().must_end_session());
}
