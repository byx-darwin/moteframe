//! Actual `ServiceHandler` response admission and event FIFO integration regressions.
#![cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
mod support;
use core::cell::Cell;
use device_content_service::*;
use device_services::ServiceHandler;
#[cfg(feature = "media")]
use device_services::SessionError;
use support::*;
#[cfg(feature = "media")]
#[test]
fn test_should_dispatch_camera_mode_without_executing_before_admission() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(0x33, 0x30, 1, &[1])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[0; 4]);
    owner.with_backend(|backend| {
        assert_eq!(backend.actions.calls, 0);
        Ok(())
    })?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|backend| {
        assert_eq!(backend.actions.calls, 1);
        Ok(())
    })?;
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_close_if_backend_revokes_scope_during_response_admission() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    owner.with_backend(|backend| {
        backend.actions.revoke = true;
        Ok(())
    })?;
    let request = frame(0x33, 0x30, 1, &[1])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(
        handler.response_admitted(request, &response[..length]),
        Err(SessionError::AuthorizationRevoked)
    );
    assert!(handler.must_end_session());
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_keep_exact_five_byte_media_fifo_and_single_pending_lane() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let mut response = [0; 32];
    let mode = frame(0x33, 0x30, 1, &[1])?;
    let length = handler
        .dispatch(mode, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(mode, &response[..length])
        .map_err(|_| Error::Invalid)?;
    let id = handler.request_action(device_content::Action::CameraShutter)?;
    assert_eq!(id, 1);
    assert_eq!(
        handler.request_action(device_content::Action::PhoneMusic(1)),
        Err(Error::Busy)
    );
    let mut event = [0; 32];
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(&event[..length], &[1, 0, 0, 0, 1]);
    let mut retry = [0; 32];
    assert_eq!(handler.prepare_event(&mut retry, 128)?, Some((token, 5)));
    assert_eq!(event, retry);
    let mut short = [0xaa; 4];
    assert_eq!(handler.prepare_event(&mut short, 128), Err(Error::Capacity));
    assert_eq!(short, [0; 4]);
    handler.confirm_event(token)?;
    handler.confirm_event(token)?;
    owner.with_backend(|backend| {
        assert_eq!(backend.actions.calls, 2);
        Ok(())
    })?;
    let wrong = frame(0x33, 0x33, 2, &[1, 0, 0, 0, 0])?;
    let length = handler
        .dispatch(wrong, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[4, 0, 6, 0xff]);
    let ack = frame(0x33, 0x31, 3, &[1, 0, 0, 0, 0])?;
    let length = handler
        .dispatch(ack, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[0; 4]);
    handler
        .response_admitted(ack, &response[..length])
        .map_err(|_| Error::Invalid)?;
    assert_eq!(
        handler.request_action(device_content::Action::PhoneMusic(3))?,
        2
    );
    Ok(())
}
#[test]
fn test_should_reject_stale_generation_and_non_one_zero_catalogue() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(10));
    let mut entries = entries();
    let original = entries[0];
    entries[0].minor = 2;
    assert!(matches!(
        ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock),
        Err(Error::Invalid)
    ));
    entries[0] = original;
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    access.generation.set(2);
    assert!(handler.must_end_session());
    assert_eq!(handler.prepare_event(&mut [0; 32], 128), Err(Error::Closed));
    handler.disconnect()?;
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_validate_decoder_and_capacity_before_any_mutation() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(0x33, 0x32, 1, &[1])?;
    assert_eq!(
        handler.dispatch(request, &mut [0xaa; 3]),
        Err(SessionError::Capacity)
    );
    let mut response = [0; 32];
    let bad = frame(0x33, 0x32, 1, &[0])?;
    let length = handler
        .dispatch(bad, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[1, 0, 8, 0xff]);
    owner.with_backend(|backend| {
        assert_eq!(backend.actions.calls, 0);
        Ok(())
    })?;
    let malformed = frame(0x33, 0x32, 1, &[1, 0])?;
    let length = handler
        .dispatch(malformed, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[1, 0, 0, 0]);
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_close_on_revocation_during_final_event_confirmation() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    handler.request_action(device_content::Action::CameraShutter)?;
    let (token, _) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    owner.with_backend(|backend| {
        backend.actions.revoke = true;
        Ok(())
    })?;
    assert_eq!(handler.confirm_event(token), Err(Error::Denied));
    assert!(handler.must_end_session());
    Ok(())
}
#[cfg(feature = "find")]
#[test]
fn test_should_start_phone_find_only_after_response_and_final_event_admission() -> Result<(), Error>
{
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(6, 0x30, 1, &[1, 3, 5, 0])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[0, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(handler.prepare_event(&mut [0; 32], 128)?, None);
    owner.with_backend(|ports| {
        assert_eq!(ports.find.starts, 0);
        Ok(())
    })?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    let stop = frame(6, 0x31, 2, &[1, 0, 0, 0])?;
    assert!(!handler.response_ready(stop).map_err(|_| Error::Invalid)?);
    let mut event = [0; 32];
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(&event[..length], &[1, 0, 0, 0, 1, 3, 5, 0]);
    clock.0.set(100);
    handler.confirm_event(token)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.find.starts, 1);
        assert_eq!(ports.find.deadline, 5100);
        Ok(())
    })?;
    clock.0.set(5099);
    assert_eq!(handler.poll_find(1)?, device_content::FindState::Active);
    clock.0.set(5100);
    assert_eq!(
        handler.poll_find(1)?,
        device_content::FindState::Terminal(device_content::FindResult::Timeout)
    );
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(&event[..length], &[1, 0, 0, 0, 3]);
    handler.confirm_event(token)?;
    handler.poll_find(1)?;
    assert_eq!(handler.prepare_event(&mut event, 128)?, None);
    Ok(())
}
#[cfg(feature = "find")]
#[test]
fn test_should_stop_device_find_after_response_and_preserve_actual_unknown_id_status()
-> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(6, 0x30, 1, &[0, 3, 5, 0])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    let stop = frame(6, 0x31, 2, &[1, 0, 0, 0])?;
    let length = handler
        .dispatch(stop, &mut response)
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.find.stops, 0);
        Ok(())
    })?;
    handler
        .response_admitted(stop, &response[..length])
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.find.stops, 1);
        Ok(())
    })?;
    let mut event = [0; 32];
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(&event[..length], &[1, 0, 0, 0, 2]);
    handler.confirm_event(token)?;
    let unknown = frame(6, 0x31, 3, &[99, 0, 0, 0])?;
    let length = handler
        .dispatch(unknown, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[8, 0, 0, 0]);
    Ok(())
}
#[cfg(feature = "phone")]
#[test]
fn test_should_route_exact_heartbeat_echo_sos_ack_and_status_push() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    handler.configure_heartbeat(device_content::HeartbeatPolicy {
        enabled: true,
        timeout_s: 10,
        grace_s: 2,
    })?;
    let request = frame(7, 0x30, 1, &[0x78, 0x56, 0x34, 0x12])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12]);
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    let status = handler.request_action(device_content::Action::PhoneStatus)?;
    let (token, length) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(length, 0);
    handler.confirm_event(token)?;
    assert!(
        handler
            .phone_status_received(status, &[2, 0, 1, 0])
            .is_err()
    );
    handler.phone_status_received(status, &[2, 1, 1, 7])?;
    let sos = handler.request_action(device_content::Action::Sos {
        source: 2,
        utc_ms: 0,
        time_valid: false,
    })?;
    let mut event = [0; 32];
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(length, 14);
    assert_eq!(event[4], 2);
    assert_eq!(event[13], 0);
    handler.confirm_event(token)?;
    let mut body = [0; 5];
    body[..4].copy_from_slice(&sos.to_le_bytes());
    body[4] = 1;
    let ack = frame(7, 0x31, 2, &body)?;
    let length = handler
        .dispatch(ack, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[0; 4]);
    handler
        .response_admitted(ack, &response[..length])
        .map_err(|_| Error::Invalid)?;
    Ok(())
}
#[cfg(all(feature = "phone", feature = "media"))]
#[test]
fn test_should_keep_phone_and_media_namespaces_independent_in_one_owner() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    assert_eq!(
        handler.request_action(device_content::Action::Sos {
            source: 1,
            utc_ms: 0,
            time_valid: false
        })?,
        1
    );
    assert_eq!(
        handler.request_action(device_content::Action::CameraShutter)?,
        1
    );
    let (mut first, length) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(first.module(), 7);
    assert_eq!(length, 14);
    handler.confirm_event(first)?;
    let (second, length) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(second.module(), 0x33);
    assert_eq!(length, 5);
    assert_ne!(first, second);
    first = second;
    handler.confirm_event(first)?;
    Ok(())
}
#[cfg(feature = "gimbal")]
#[test]
fn test_should_route_c34_ticket_and_only_emit_actual_seven_byte_e82() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let mut body = [0; 11];
    device_content::GimbalRequest {
        action_id: 1,
        action: 1,
        direction: 0,
        mode: 0,
        value: 1000,
    }
    .encode(&mut body)?;
    let request = frame(0x33, 0x34, 1, &body)?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.gimbal.starts, 0);
        Ok(())
    })?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    assert_eq!(
        handler.poll_gimbal(1)?,
        device_content::GimbalState::Running
    );
    assert_eq!(handler.prepare_event(&mut [0; 32], 128)?, None);
    owner.with_backend(|ports| {
        ports.gimbal.observation = Observation::Terminal;
        Ok(())
    })?;
    assert_eq!(
        handler.poll_gimbal(1)?,
        device_content::GimbalState::Terminal {
            result: device_content::GimbalResult::Completed,
            reason: 0
        }
    );
    let mut event = [0; 32];
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(&event[..length], &[1, 0, 0, 0, 1, 0, 0]);
    handler.confirm_event(token)?;
    let retry = frame(0x33, 0x34, 2, &body)?;
    let length = handler
        .dispatch(retry, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(retry, &response[..length])
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.gimbal.starts, 1);
        Ok(())
    })?;
    body[7] = 1;
    let conflict = frame(0x33, 0x34, 3, &body)?;
    let length = handler
        .dispatch(conflict, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[0x0b, 0, 0, 0]);
    Ok(())
}
#[cfg(feature = "companion-radio")]
#[test]
fn test_should_publish_real_radio_store_revision_before_exact_six_byte_e90() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(8, 0x30, 1, &[1])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.radio.starts, 0);
        assert_eq!(ports.store.publishes, 0);
        Ok(())
    })?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    assert_eq!(
        handler.poll_radio()?,
        device_content::CompanionRadioState::Switching
    );
    let mut event = [0; 32];
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(token.opcode(), 0x90);
    assert_eq!(&event[..length], &[1, 0, 1, 0, 0, 0]);
    handler.confirm_event(token)?;
    owner.with_backend(|ports| {
        ports.radio.stable = true;
        Ok(())
    })?;
    assert_eq!(
        handler.poll_radio()?,
        device_content::CompanionRadioState::Stable
    );
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(&event[..length], &[1, 0, 2, 0, 0, 0]);
    handler.confirm_event(token)?;
    handler.poll_radio()?;
    assert_eq!(handler.prepare_event(&mut event, 128)?, None);
    Ok(())
}
#[cfg(feature = "notifications")]
#[test]
fn test_should_bridge_real_commit_blob_call_result_and_deleted_generation() -> Result<(), Error> {
    let value = device_content::Notification {
        notification: 42,
        category: 1,
        source_app: "phone",
        sender: "A",
        number: "123",
        title: "call",
        body: "",
        action_mask: 7,
        created_utc_ms: 0,
        created_time_valid: false,
    };
    let mut bytes = [0; 128];
    let length = value.encode(&mut bytes, 64).map_err(|_| Error::Invalid)?;
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    handler.stage_notification_bytes(5, &bytes[..length], 1)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.notifications.alerts, 0);
        Ok(())
    })?;
    handler.notification_response_admitted(5)?;
    handler.notification_response_admitted(5)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.notifications.alerts, 1);
        Ok(())
    })?;
    handler.request_notification_action(42, 2)?;
    assert!(!handler.notification_delete_ready(5)?);
    let mut event = [0; 32];
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(&event[..length], &[42, 0, 0, 0, 2]);
    handler.confirm_event(token)?;
    handler.request_notification_action(42, 2)?;
    assert_eq!(handler.prepare_event(&mut event, 128)?, None);
    let request = frame(0x30, 0x30, 1, &[42, 0, 0, 0, 2, 4])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[0; 4]);
    assert!(!handler.notification_delete_ready(5)?);
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    assert!(handler.notification_delete_ready(5)?);
    access.notification_generation.set(0);
    handler.notification_deleted(5)?;
    assert!(!handler.replay_valid(request, &response[..length]));
    owner.with_backend(|ports| {
        assert_eq!(ports.notifications.requests, 1);
        assert_eq!(ports.notifications.withdrawals, 1);
        Ok(())
    })?;
    Ok(())
}
#[cfg(feature = "companion-radio")]
#[test]
fn test_should_not_replace_radio_generation_while_previous_e90_is_queued() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(8, 0x30, 1, &[1])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    handler.poll_radio()?;
    owner.with_backend(|ports| {
        ports.radio.stable = true;
        Ok(())
    })?;
    assert_eq!(handler.poll_radio(), Err(Error::Busy));
    assert_eq!(access.revision.get(), 1);
    let (token, length) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(length, 6);
    handler.confirm_event(token)?;
    handler.poll_radio()?;
    assert_eq!(access.revision.get(), 2);
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_drain_queued_control_event_before_accepting_another_response() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    handler.request_action(device_content::Action::CameraShutter)?;
    let request = frame(0x33, 0x32, 1, &[1])?;
    assert!(
        !handler
            .response_ready(request)
            .map_err(|_| Error::Invalid)?
    );
    assert_eq!(
        handler.dispatch(request, &mut [0; 32]),
        Err(SessionError::Deferred)
    );
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_share_real_session_cache_without_reexecuting_or_releasing_stale_reply()
-> Result<(), Error> {
    use device_services::{BuildProfile, Config, DeviceInfo, Session, SessionState};
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut modules = vec![device_protocol::ModuleEntry {
        module: 0,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }];
    modules.extend_from_slice(&entries);
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let mut session = Session::new(Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 1,
            fw_major: 1,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 1,
        },
        modules: &modules,
        limits: device_protocol::ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10_000,
        },
    })
    .map_err(|_| Error::Invalid)?;
    session
        .control_notifications_enabled()
        .map_err(|_| Error::Invalid)?;
    let mut response = [0; 128];
    session
        .dispatch(frame(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0])?, &mut response)
        .map_err(|_| Error::Invalid)?;
    let request = frame(0x33, 0x30, 2, &[1])?;
    let length = session
        .dispatch_with_handler(request, &mut handler, &mut response)
        .map_err(|_| Error::Invalid)?;
    let mut retry = [0; 128];
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut retry),
        Ok(length)
    );
    assert_eq!(&retry[..length], &response[..length]);
    owner.with_backend(|ports| {
        assert_eq!(ports.actions.calls, 0);
        Ok(())
    })?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut retry),
        Ok(length)
    );
    owner.with_backend(|ports| {
        assert_eq!(ports.actions.calls, 1);
        Ok(())
    })?;
    access.generation.set(2);
    retry.fill(0xaa);
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut retry),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(retry, [0; 128]);
    assert_eq!(session.state(), SessionState::Closed);
    Ok(())
}
#[cfg(all(feature = "find", feature = "media"))]
#[test]
fn test_should_reserve_fifo_space_for_response_first_find_and_retry_terminal_when_full()
-> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 1>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(6, 0x30, 1, &[1, 3, 1, 0])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(
        handler.request_action(device_content::Action::CameraShutter),
        Err(Error::Busy)
    );
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    let (token, _) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    handler.confirm_event(token)?;
    handler.request_action(device_content::Action::CameraShutter)?;
    clock.0.set(1000);
    assert_eq!(handler.poll_find(1), Err(Error::Busy));
    owner.with_backend(|ports| {
        assert_eq!(ports.find.stops, 1);
        Ok(())
    })?;
    let (token, _) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    handler.confirm_event(token)?;
    handler.poll_find(1)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.find.stops, 1);
        Ok(())
    })?;
    let (token, length) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(token.module(), 6);
    assert_eq!(token.opcode(), 0x81);
    assert_eq!(length, 5);
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_clear_fifo_and_mark_unknown_on_disconnect_or_clock_reversal() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(10));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    assert!(matches!(
        ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock),
        Err(Error::Busy)
    ));
    handler.request_action(device_content::Action::CameraShutter)?;
    let (token, _) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    handler.confirm_event(token)?;
    clock.0.set(9);
    let mut bytes = [0xaa; 32];
    assert_eq!(handler.prepare_event(&mut bytes, 128), Err(Error::Clock));
    assert_eq!(bytes, [0; 32]);
    assert!(handler.must_end_session());
    assert_eq!(owner.cleanup_report().unknown_count, 1);
    handler.disconnect()?;
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_keep_camera_desired_state_idempotent_when_ledger_is_full() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 1, 1, 1, 1>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let mut response = [0; 32];
    for message in 1..=2 {
        let request = frame(0x33, 0x30, message, &[1])?;
        let length = handler
            .dispatch(request, &mut response)
            .map_err(|_| Error::Invalid)?;
        assert_eq!(&response[..length], &[0; 4]);
        handler
            .response_admitted(request, &response[..length])
            .map_err(|_| Error::Invalid)?;
    }
    owner.with_backend(|ports| {
        assert_eq!(ports.actions.calls, 1);
        Ok(())
    })?;
    Ok(())
}
#[cfg(feature = "companion-radio")]
#[test]
fn test_should_report_pending_radio_unknown_and_cancel_once_on_disconnect() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(8, 0x30, 1, &[1])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    handler.disconnect()?;
    handler.disconnect()?;
    assert_eq!(owner.cleanup_report().unknown_count, 1);
    owner.with_backend(|ports| {
        assert_eq!(ports.radio.cancels, 1);
        Ok(())
    })?;
    Ok(())
}
#[cfg(feature = "gimbal")]
#[test]
fn test_should_cancel_scope_after_uncertain_gimbal_observation_instead_of_claiming_stopped()
-> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let mut body = [0; 11];
    device_content::GimbalRequest {
        action_id: 1,
        action: 1,
        direction: 0,
        mode: 0,
        value: 1000,
    }
    .encode(&mut body)?;
    let request = frame(0x33, 0x34, 1, &body)?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|ports| {
        ports.gimbal.observation = Observation::Failed;
        Ok(())
    })?;
    assert_eq!(
        handler.poll_gimbal(1),
        Err(Error::Content(device_content::Error::Backend))
    );
    assert!(handler.must_end_session());
    assert_eq!(owner.cleanup_report().unknown_count, 1);
    assert!(!owner.cleanup_report().failed);
    owner.with_backend(|ports| {
        assert_eq!(ports.gimbal.scope_cancels, 1);
        Ok(())
    })?;
    Ok(())
}
#[cfg(feature = "gimbal")]
#[test]
fn test_should_wait_for_actual_old_scope_drain_before_new_binding() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    owner.with_backend(|ports| {
        ports.gimbal.ready = false;
        Ok(())
    })?;
    let mut body = [0; 11];
    device_content::GimbalRequest {
        action_id: 1,
        action: 1,
        direction: 0,
        mode: 0,
        value: 1000,
    }
    .encode(&mut body)?;
    let request = frame(0x33, 0x34, 1, &body)?;
    assert!(
        !handler
            .response_ready(request)
            .map_err(|_| Error::Invalid)?
    );
    owner.with_backend(|ports| {
        ports.gimbal.ready = true;
        ports.gimbal.drain_after_cancel = false;
        Ok(())
    })?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    handler.disconnect()?;
    access.connection.set(2);
    assert!(matches!(
        ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock),
        Err(Error::Busy)
    ));
    owner.with_backend(|ports| {
        ports.gimbal.drained = true;
        Ok(())
    })?;
    let _next = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    Ok(())
}
#[cfg(feature = "gimbal")]
#[test]
fn test_should_latch_real_cleanup_failure_and_refuse_a_new_session() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let mut body = [0; 11];
    device_content::GimbalRequest {
        action_id: 1,
        action: 1,
        direction: 0,
        mode: 0,
        value: 1000,
    }
    .encode(&mut body)?;
    let request = frame(0x33, 0x34, 1, &body)?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|ports| {
        ports.gimbal.cancellation = Cancellation::Failed;
        Ok(())
    })?;
    assert_eq!(handler.disconnect(), Err(Error::Backend));
    assert_eq!(owner.cleanup_report().unknown_count, 1);
    assert!(owner.cleanup_report().failed);
    access.connection.set(2);
    assert!(matches!(
        ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock),
        Err(Error::Backend)
    ));
    Ok(())
}
#[cfg(all(feature = "media", feature = "companion-radio"))]
#[test]
fn test_should_not_activate_a_compiled_radio_module_omitted_from_actual_catalogue()
-> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries()
        .into_iter()
        .filter(|entry| entry.module == 0x33)
        .collect::<Vec<_>>();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    assert_eq!(
        handler.poll_radio(),
        Err(Error::Content(device_content::Error::Unknown))
    );
    owner.with_backend(|ports| {
        assert_eq!(ports.store.publishes, 0);
        Ok(())
    })?;
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_recheck_trusted_source_port_before_releasing_event_bytes() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    handler.request_action(device_content::Action::CameraShutter)?;
    access.coarse.set(false);
    let mut output = [0xaa; 32];
    assert_eq!(handler.prepare_event(&mut output, 128), Err(Error::Denied));
    assert_eq!(output, [0; 32]);
    assert!(handler.must_end_session());
    Ok(())
}
#[cfg(feature = "phone")]
#[test]
fn test_should_observe_heartbeat_deadlines_without_claiming_unreceived_liveness()
-> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    handler.configure_heartbeat(device_content::HeartbeatPolicy {
        enabled: true,
        timeout_s: 10,
        grace_s: 2,
    })?;
    assert_eq!(
        handler.poll_heartbeat()?,
        device_content::HeartbeatState::Waiting
    );
    let request = frame(7, 0x30, 1, &[1, 0, 0, 0])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    assert_eq!(
        handler.poll_heartbeat()?,
        device_content::HeartbeatState::Healthy
    );
    clock.0.set(10_000);
    assert_eq!(
        handler.poll_heartbeat()?,
        device_content::HeartbeatState::Grace
    );
    clock.0.set(12_000);
    assert_eq!(
        handler.poll_heartbeat()?,
        device_content::HeartbeatState::TimedOut
    );
    Ok(())
}
#[cfg(feature = "find")]
#[test]
fn test_should_queue_device_originated_find_with_exact_requested_pattern() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    assert_eq!(
        handler.request_find(device_content::FindRequest {
            target: device_content::FindTarget::Phone,
            pattern: 2,
            duration_s: 5
        })?,
        1
    );
    owner.with_backend(|ports| {
        assert_eq!(ports.find.starts, 0);
        Ok(())
    })?;
    let mut event = [0; 32];
    let (token, length) = handler
        .prepare_event(&mut event, 128)?
        .ok_or(Error::Invalid)?;
    assert_eq!(&event[..length], &[1, 0, 0, 0, 1, 2, 5, 0]);
    handler.confirm_event(token)?;
    owner.with_backend(|ports| {
        assert_eq!(ports.find.starts, 1);
        Ok(())
    })?;
    Ok(())
}
#[cfg(feature = "find")]
#[test]
fn test_should_retain_uncertain_find_stop_and_latch_actual_cleanup_failure() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let request = frame(6, 0x30, 1, &[0, 3, 1, 0])?;
    let mut response = [0; 32];
    let length = handler
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    handler
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    owner.with_backend(|ports| {
        ports.find.stop_fail = true;
        Ok(())
    })?;
    clock.0.set(1000);
    assert_eq!(
        handler.poll_find(1),
        Err(Error::Content(device_content::Error::Backend))
    );
    assert_eq!(owner.cleanup_report().unknown_count, 1);
    assert!(owner.cleanup_report().failed);
    owner.with_backend(|ports| {
        assert_eq!(ports.find.stops, 2);
        Ok(())
    })?;
    Ok(())
}
#[cfg(all(feature = "find", feature = "gimbal"))]
#[test]
fn test_should_not_query_omitted_unused_gimbal_hardware_on_rebinding() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries()
        .into_iter()
        .filter(|entry| entry.module == 6)
        .collect::<Vec<_>>();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    handler.disconnect()?;
    owner.with_backend(|ports| {
        ports.gimbal.drain_error = Some(device_content::Error::Unavailable);
        Ok(())
    })?;
    access.connection.set(2);
    let _next = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    Ok(())
}

#[cfg(feature = "find")]
#[test]
fn test_should_close_after_uncertain_find_submission_with_any_backend_error() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let mut handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    handler.request_find(device_content::FindRequest {
        target: device_content::FindTarget::Phone,
        pattern: 2,
        duration_s: 1,
    })?;
    let (token, _) = handler
        .prepare_event(&mut [0; 32], 128)?
        .ok_or(Error::Invalid)?;
    owner.with_backend(|ports| {
        ports.find.start_error = Some(device_content::Error::State);
        Ok(())
    })?;
    assert_eq!(
        handler.confirm_event(token),
        Err(Error::Content(device_content::Error::State))
    );
    assert!(handler.must_end_session());
    assert_eq!(owner.cleanup_report().unknown_count, 1);
    owner.with_backend(|ports| {
        assert_eq!(ports.find.stops, 1);
        Ok(())
    })?;
    Ok(())
}
