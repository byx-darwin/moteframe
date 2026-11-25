//! FIND response admission, frozen dictionaries and externally timed real backend behavior.
#![cfg(feature = "find")]
use device_content::*;
#[derive(Debug)]
struct Access;
impl AccessPort for Access {
    fn authorize(&self, _: Scope, _: Operation) -> Result<(), Error> {
        Ok(())
    }
}
#[derive(Debug, Default)]
struct Port {
    starts: u32,
    stops: u32,
    missing: bool,
    fail: bool,
    last_deadline: u64,
}
impl FindPort for Port {
    fn capability(&self, _: FindTarget) -> Result<FindCapability, Error> {
        if self.missing {
            return Err(Error::Unavailable);
        }
        Ok(FindCapability {
            patterns: 3,
            min_duration_s: 1,
            max_duration_s: 60,
        })
    }
    fn start(&mut self, _: Scope, _: u32, _: FindRequest, deadline: u64) -> Result<(), Error> {
        self.starts += 1;
        self.last_deadline = deadline;
        if self.fail {
            Err(Error::Backend)
        } else {
            Ok(())
        }
    }
    fn stop(&mut self, _: Scope, _: u32) -> Result<(), Error> {
        self.stops += 1;
        if self.fail {
            Err(Error::Backend)
        } else {
            Ok(())
        }
    }
}
fn request(target: FindTarget) -> FindRequest {
    FindRequest {
        target,
        pattern: 3,
        duration_s: 5,
    }
}
#[test]
fn test_should_reserve_without_actuating_and_use_frozen_target_values() -> Result<(), Error> {
    let mut service = FindService::<3>::new(Scope::new(1, 1)?);
    let port = Port::default();
    assert_eq!(
        service.prepare(request(FindTarget::Device), &Access, &port)?,
        1
    );
    assert_eq!(port.starts, 0);
    assert_eq!(
        FindRequest::decode(&[0, 3, 5, 0])?,
        request(FindTarget::Device)
    );
    assert_eq!(FindRequest::decode(&[2, 3, 5, 0]), Err(Error::Invalid));
    assert_eq!(FindRequest::decode(&[0, 0, 5, 0]), Err(Error::Invalid));
    assert_eq!(FindRequest::decode(&[0, 3, 0, 0]), Err(Error::Invalid));
    let mut event = [0; 8];
    service.request_event(1, &mut event)?;
    assert_eq!(event, [1, 0, 0, 0, 0, 3, 5, 0]);
    Ok(())
}
#[test]
fn test_should_start_duration_at_actual_admission_and_stop_at_external_deadline()
-> Result<(), Error> {
    let mut service = FindService::<3>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    let id = service.prepare(request(FindTarget::Device), &Access, &port)?;
    assert_eq!(
        service.response_admitted(id, 100, &Access, &mut port)?,
        FindState::Active
    );
    assert_eq!(port.last_deadline, 5100);
    service.response_admitted(id, 100, &Access, &mut port)?;
    assert_eq!(port.starts, 1);
    assert_eq!(service.request_event(id, &mut [0; 8]), Err(Error::State));
    assert_eq!(service.poll(id, 5099, &mut port)?, FindState::Active);
    assert_eq!(
        service.poll(id, 5100, &mut port)?,
        FindState::Terminal(FindResult::Timeout)
    );
    assert_eq!(port.stops, 1);
    service.poll(id, 6000, &mut port)?;
    assert_eq!(port.stops, 1);
    assert_eq!(service.poll(id, 5999, &mut port), Err(Error::State));
    let mut event = [0; 5];
    service.result_event(id, &mut event)?;
    assert_eq!(event, [1, 0, 0, 0, 3]);
    Ok(())
}
#[test]
fn test_should_stop_only_after_stop_response_is_admitted() -> Result<(), Error> {
    let mut service = FindService::<3>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    let id = service.prepare(request(FindTarget::Device), &Access, &port)?;
    service.response_admitted(id, 0, &Access, &mut port)?;
    service.prepare_stop(id, &Access)?;
    assert_eq!(port.stops, 0);
    assert_eq!(service.state(id)?, FindState::StopPrepared);
    assert_eq!(
        service.stop_response_admitted(id, &Access, &mut port)?,
        FindState::Terminal(FindResult::UserStop)
    );
    service.prepare_stop(id, &Access)?;
    service.stop_response_admitted(id, &Access, &mut port)?;
    assert_eq!(port.stops, 1);
    assert_eq!(service.prepare_stop(100, &Access), Err(Error::NotFound));
    Ok(())
}
#[test]
fn test_should_keep_frozen_phone_result_mapping_and_terminal_idempotence() -> Result<(), Error> {
    for (wire, result, event) in [
        (1, FindResult::Found, 1),
        (2, FindResult::Timeout, 3),
        (3, FindResult::UserStop, 2),
    ] {
        let mut service = FindService::<2>::new(Scope::new(1, 1)?);
        let mut port = Port::default();
        let id = service.prepare(request(FindTarget::Phone), &Access, &port)?;
        assert_eq!(service.phone_result(id, wire, &Access), Err(Error::State));
        service.response_admitted(id, 0, &Access, &mut port)?;
        service.phone_result(id, wire, &Access)?;
        service.phone_result(id, wire, &Access)?;
        assert_eq!(service.state(id)?, FindState::Terminal(result));
        assert_eq!(
            service.phone_result(id, if wire == 1 { 2 } else { 1 }, &Access),
            Err(Error::Conflict)
        );
        let mut bytes = [0; 5];
        service.result_event(id, &mut bytes)?;
        assert_eq!(bytes[4], event);
    }
    Ok(())
}
#[test]
fn test_should_fail_closed_on_missing_backend_limits_and_scope_loss() -> Result<(), Error> {
    let mut service = FindService::<2>::new(Scope::new(1, 1)?);
    let mut port = Port {
        missing: true,
        ..Port::default()
    };
    assert_eq!(
        service.prepare(request(FindTarget::Device), &Access, &port),
        Err(Error::Unavailable)
    );
    port.missing = false;
    let mut invalid = request(FindTarget::Device);
    invalid.duration_s = 61;
    assert_eq!(
        service.prepare(invalid, &Access, &port),
        Err(Error::Invalid)
    );
    let id = service.prepare(request(FindTarget::Device), &Access, &port)?;
    assert_eq!(
        service.prepare(request(FindTarget::Device), &Access, &port),
        Err(Error::Conflict)
    );
    service.response_admitted(id, 0, &Access, &mut port)?;
    assert_eq!(service.invalidate(&mut port)?, 1);
    assert_eq!(service.state(id)?, FindState::Unknown);
    service.response_admitted(id, 1, &Access, &mut port)?;
    assert_eq!(port.starts, 1);
    service.replace_scope(Scope::new(2, 2)?)?;
    assert_eq!(service.state(id), Err(Error::NotFound));
    Ok(())
}
#[test]
fn test_should_preserve_actual_backend_failure() -> Result<(), Error> {
    let mut service = FindService::<2>::new(Scope::new(1, 1)?);
    let mut port = Port {
        fail: true,
        ..Port::default()
    };
    let id = service.prepare(request(FindTarget::Device), &Access, &port)?;
    assert_eq!(
        service.response_admitted(id, 0, &Access, &mut port),
        Err(Error::Backend)
    );
    assert_eq!(
        service.state(id)?,
        FindState::Terminal(FindResult::BackendFailure)
    );
    assert_eq!(service.result_event(id, &mut [0; 4]), Err(Error::Capacity));
    Ok(())
}
