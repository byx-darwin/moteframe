//! Action admission, immutable acknowledgement, source access and invalidation regressions.
#![cfg(any(feature = "media", feature = "phone"))]
use device_content::*;
#[derive(Debug)]
struct Access;
impl AccessPort for Access {
    fn authorize(&self, _: Scope, _: Operation) -> Result<(), Error> {
        Ok(())
    }
}
#[derive(Debug)]
struct Deny;
impl AccessPort for Deny {
    fn authorize(&self, _: Scope, _: Operation) -> Result<(), Error> {
        Err(Error::Denied)
    }
}
#[derive(Debug, Default)]
struct Port {
    calls: u32,
    missing: bool,
    fail: bool,
    local: bool,
}
impl ActionPort for Port {
    fn available(&self, _: Action) -> bool {
        !self.missing
    }
    fn execute(&mut self, _: Scope, _: u32, _: Action) -> Result<Submission, Error> {
        self.calls += 1;
        if self.fail {
            return Err(Error::Backend);
        }
        if self.local {
            Ok(Submission::Completed)
        } else {
            Ok(Submission::AwaitingResult)
        }
    }
}
#[cfg(feature = "media")]
fn action() -> Action {
    Action::CameraShutter
}
#[cfg(all(feature = "phone", not(feature = "media")))]
fn action() -> Action {
    Action::Sos {
        source: 1,
        utc_ms: 0,
        time_valid: false,
    }
}
#[test]
fn test_should_reserve_nonzero_monotonic_events_without_executing() -> Result<(), Error> {
    let mut service = ActionService::<4>::new(Scope::new(1, 1)?);
    let port = Port::default();
    assert_eq!(service.prepare(action(), &Access, &port)?, 1);
    assert_eq!(service.prepare(action(), &Access, &port)?, 2);
    assert_eq!(port.calls, 0);
    Ok(())
}
#[test]
fn test_should_execute_only_once_after_admission_and_refuse_event_replay() -> Result<(), Error> {
    let mut service = ActionService::<4>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    let id = service.prepare(action(), &Access, &port)?;
    let mut bytes = [0; 16];
    assert!(service.event_bytes(id, &mut bytes).is_ok());
    assert_eq!(
        service.response_admitted(id, &Access, &mut port)?,
        ActionState::AwaitingResult
    );
    assert_eq!(
        service.response_admitted(id, &Access, &mut port)?,
        ActionState::AwaitingResult
    );
    assert_eq!(service.event_bytes(id, &mut bytes), Err(Error::State));
    assert_eq!(port.calls, 1);
    Ok(())
}
#[test]
fn test_should_require_real_backend_and_source_authorization() -> Result<(), Error> {
    let mut service = ActionService::<2>::new(Scope::new(1, 1)?);
    let mut port = Port {
        missing: true,
        ..Port::default()
    };
    assert_eq!(
        service.prepare(action(), &Access, &port),
        Err(Error::Unavailable)
    );
    port.missing = false;
    assert_eq!(service.prepare(action(), &Deny, &port), Err(Error::Denied));
    let id = service.prepare(action(), &Access, &port)?;
    assert_eq!(
        service.response_admitted(id, &Deny, &mut port),
        Err(Error::Denied)
    );
    assert_eq!(port.calls, 0);
    assert_eq!(service.settle(id, 1, &Deny), Err(Error::Denied));
    assert_eq!(service.settle(999, 1, &Access), Err(Error::NotFound));
    Ok(())
}
#[test]
fn test_should_report_unconfirmed_actions_unknown_on_disconnect() -> Result<(), Error> {
    let mut service = ActionService::<2>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    let id = service.prepare(action(), &Access, &port)?;
    service.response_admitted(id, &Access, &mut port)?;
    let lost = service.invalidate();
    assert_eq!(lost.len(), 1);
    assert_eq!(lost.id(0)?, id);
    assert_eq!(service.state(id)?, ActionState::Unknown);
    service.response_admitted(id, &Access, &mut port)?;
    assert_eq!(port.calls, 1);
    service.replace_scope(Scope::new(1, 2)?)?;
    assert_eq!(service.state(id), Err(Error::NotFound));
    Ok(())
}
#[test]
fn test_should_keep_backend_failure_terminal_without_retry() -> Result<(), Error> {
    let mut service = ActionService::<1>::new(Scope::new(1, 1)?);
    let mut port = Port {
        fail: true,
        ..Port::default()
    };
    let id = service.prepare(action(), &Access, &port)?;
    assert_eq!(
        service.response_admitted(id, &Access, &mut port),
        Err(Error::Backend)
    );
    assert_eq!(
        service.response_admitted(id, &Access, &mut port)?,
        ActionState::Failed
    );
    assert_eq!(port.calls, 1);
    assert_eq!(
        service.prepare(action(), &Access, &port),
        Err(Error::Capacity)
    );
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_preserve_all_camera_music_ack_values_and_immutable_results() -> Result<(), Error> {
    for action in [Action::CameraShutter, Action::PhoneMusic(3)] {
        for result in 0..=3 {
            let mut service = ActionService::<2>::new(Scope::new(1, 1)?);
            let mut port = Port::default();
            let id = service.prepare(action, &Access, &port)?;
            assert_eq!(service.settle(id, result, &Access), Err(Error::State));
            service.response_admitted(id, &Access, &mut port)?;
            service.settle(id, result, &Access)?;
            service.settle(id, result, &Access)?;
            assert_eq!(
                service.settle(id, (result + 1) % 4, &Access),
                Err(Error::Conflict)
            );
            assert_eq!(service.settle(id, 4, &Access), Err(Error::Invalid));
            assert_eq!(port.calls, 1);
        }
    }
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_keep_camera_mode_idempotent_and_device_music_local() -> Result<(), Error> {
    let mut service = MediaService::<4>::new(Scope::new(1, 1)?);
    let mut port = Port {
        local: true,
        ..Port::default()
    };
    let id = service.prepare(Action::CameraMode(true), &Access, &port)?;
    service.response_admitted(id, &Access, &mut port)?;
    assert_eq!(
        service.prepare(Action::CameraMode(true), &Access, &port)?,
        id
    );
    let music = service.prepare(Action::DeviceMusic(2), &Access, &port)?;
    assert_eq!(service.event_bytes(music, &mut [0; 16]), Err(Error::State));
    assert_eq!(
        service.response_admitted(music, &Access, &mut port)?,
        ActionState::Completed
    );
    assert_eq!(
        service.prepare(Action::DeviceMusic(0), &Access, &port),
        Err(Error::Invalid)
    );
    assert_eq!(port.calls, 2);
    Ok(())
}
#[cfg(feature = "phone")]
#[test]
fn test_should_encode_sos_validity_and_settle_only_real_result() -> Result<(), Error> {
    let mut service = PhoneService::<4>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    assert_eq!(
        service.prepare(
            Action::Sos {
                source: 1,
                utc_ms: 2,
                time_valid: false
            },
            &Access,
            &port
        ),
        Err(Error::Invalid)
    );
    let id = service.prepare(
        Action::Sos {
            source: 2,
            utc_ms: 0,
            time_valid: false,
        },
        &Access,
        &port,
    )?;
    let mut bytes = [0; 14];
    assert_eq!(service.event_bytes(id, &mut bytes)?, 14);
    assert_eq!(bytes[4], 2);
    assert_eq!(bytes[13], 0);
    service.response_admitted(id, &Access, &mut port)?;
    assert_eq!(service.settle(id, 0, &Access), Err(Error::Invalid));
    service.settle(id, 1, &Access)?;
    Ok(())
}
#[cfg(feature = "phone")]
#[test]
fn test_should_coalesce_status_until_actual_p0002_push() -> Result<(), Error> {
    let mut service = PhoneService::<4>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    let id = service.prepare(Action::PhoneStatus, &Access, &port)?;
    assert_eq!(service.prepare(Action::PhoneStatus, &Access, &port)?, id);
    assert_eq!(service.event_bytes(id, &mut [])?, 0);
    service.response_admitted(id, &Access, &mut port)?;
    service.status_received(id, &Access)?;
    assert_eq!(service.state(id)?, ActionState::Completed);
    assert!(service.prepare(Action::PhoneStatus, &Access, &port)? > id);
    Ok(())
}
