//! Same-handler request/event bridge through actual first-fragment FIFO reservation.
#![cfg(all(feature = "event-pump", feature = "phone"))]
mod support;
use core::cell::Cell;
use device_connection::{
    ControlConnection, EventChannel, EventPlane, EventPump, FirstFragmentQueue, FirstSlotError,
    FirstSlotOutcome, SharedControlChannel,
};
use device_content_service::*;
use support::*;
#[derive(Debug)]
struct Queue {
    available: bool,
    calls: usize,
    frames: Vec<Vec<u8>>,
}
impl FirstFragmentQueue for Queue {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        if !self.available {
            return Ok(FirstSlotOutcome::NoSlot);
        }
        let mut slot = [0; 20];
        self.calls += 1;
        let (length, value) = fill(&mut slot).map_err(FirstSlotError::Fill)?;
        if length == 0 || length > slot.len() {
            return Err(FirstSlotError::Contract);
        }
        self.frames.push(slot[..length].to_vec());
        Ok(FirstSlotOutcome::Admitted(value))
    }
}
fn limits() -> device_protocol::ProtocolLimits {
    device_protocol::ProtocolLimits {
        rx_frame_max: 20,
        tx_frame_max: 20,
        control_max: 128,
        data_max: 0,
        idle_ms: 2000,
        fragment_ms: 100,
        response_ms: 10_000,
    }
}
fn catalogue() -> Vec<device_protocol::ModuleEntry> {
    let mut catalogue = entries();
    catalogue.insert(
        0,
        device_protocol::ModuleEntry {
            module: 0,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 128,
            output_max: 128,
        },
    );
    catalogue
}
fn paired<'a>(
    owner: &'a SharedControlChannel,
    entries: &'a [device_protocol::ModuleEntry],
    handler: &mut impl device_services::ServiceHandler,
) -> Result<(ControlConnection<'a>, EventChannel<'a>), Error> {
    let config = device_services::Config {
        session: 1,
        capability_rev: 1,
        profile: device_services::BuildProfile::Commercial,
        info: device_services::DeviceInfo {
            product_id: 0,
            fw_major: 1,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 0,
        },
        modules: entries,
        limits: limits(),
    };
    let (mut control, token) =
        ControlConnection::with_events(config, owner).map_err(|_| Error::Invalid)?;
    control
        .notifications_enabled()
        .map_err(|_| Error::Invalid)?;
    core_request(&mut control, handler, 1, 1, &[1, 2, 128, 0, 0, 0, 3, 0])?;
    drain(&mut control, handler)?;
    Ok((control, token))
}
fn core_request(
    control: &mut ControlConnection<'_>,
    handler: &mut impl device_services::ServiceHandler,
    opcode: u8,
    id: u16,
    body: &[u8],
) -> Result<(), Error> {
    let request = frame(0, opcode, id, body)?;
    let mut bytes = [0; 20];
    let n = request.encode(&mut bytes).map_err(|_| Error::Invalid)?;
    control
        .receive(&bytes[..n], 0, handler)
        .map_err(|_| Error::Invalid)?;
    Ok(())
}
fn drain(
    control: &mut ControlConnection<'_>,
    handler: &mut impl device_services::ServiceHandler,
) -> Result<(), Error> {
    while control.response_pending() {
        control
            .next_frame(handler, &mut [0; 20])
            .map_err(|_| Error::Invalid)?;
        control.admit_frame(handler).map_err(|_| Error::Invalid)?;
    }
    Ok(())
}
#[test]
fn test_should_wait_for_actual_final_fifo_fragment_before_action_submission() -> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let host = ContentHost::new(handler);
    host.with_handler(|handler| {
        handler.request_action(device_content::Action::Sos {
            source: 1,
            utc_ms: 0,
            time_valid: false,
        })
    })?;
    let channel_owner = SharedControlChannel::new();
    let catalogue = catalogue();
    let (_control, event_channel) = paired(&channel_owner, &catalogue, &mut host.requests())?;
    let mut pump = EventPump::new(event_channel, host.events()).map_err(|_| Error::Invalid)?;
    let mut queue = Queue {
        available: false,
        calls: 0,
        frames: Vec::new(),
    };
    assert!(
        !pump
            .flush(0, EventPlane::Control, &mut queue)
            .map_err(|_| Error::Invalid)?
    );
    assert_eq!(queue.calls, 0);
    queue.available = true;
    assert!(
        pump.flush(0, EventPlane::Control, &mut queue)
            .map_err(|_| Error::Invalid)?
    );
    assert!(pump.pending());
    owner.with_backend(|ports| {
        assert_eq!(ports.actions.calls, 0);
        Ok(())
    })?;
    assert!(
        pump.flush(1, EventPlane::Control, &mut queue)
            .map_err(|_| Error::Invalid)?
    );
    assert!(!pump.pending());
    owner.with_backend(|ports| {
        assert_eq!(ports.actions.calls, 1);
        Ok(())
    })?;
    assert_eq!(queue.frames.len(), 2);
    Ok(())
}
#[test]
fn test_should_share_request_and_event_handles_without_holding_a_mutable_handler_borrow()
-> Result<(), Error> {
    use device_services::ServiceHandler;
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let host = ContentHost::new(handler);
    let mut requests = host.requests();
    let request = frame(7, 0x30, 1, &[0x78, 0x56, 0x34, 0x12])?;
    let mut response = [0; 32];
    let length = requests
        .dispatch(request, &mut response)
        .map_err(|_| Error::Invalid)?;
    assert_eq!(&response[..length], &[0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12]);
    requests
        .response_admitted(request, &response[..length])
        .map_err(|_| Error::Invalid)?;
    host.with_handler(|handler| handler.request_action(device_content::Action::PhoneStatus))?;
    let channel_owner = SharedControlChannel::new();
    let catalogue = catalogue();
    let (mut control, event_channel) = paired(&channel_owner, &catalogue, &mut host.requests())?;
    let mut pump = EventPump::new(event_channel, host.events()).map_err(|_| Error::Invalid)?;
    let mut queue = Queue {
        available: true,
        calls: 0,
        frames: Vec::new(),
    };
    core_request(&mut control, &mut requests, 5, 2, &[0, 0, 0, 0])?;
    assert!(
        !pump
            .flush(0, EventPlane::Control, &mut queue)
            .map_err(|_| Error::Invalid)?
    );
    assert_eq!(queue.calls, 0);
    drain(&mut control, &mut requests)?;
    assert!(
        pump.flush(0, EventPlane::Control, &mut queue)
            .map_err(|_| Error::Invalid)?
    );
    assert_eq!(queue.frames[0].len(), 10);
    assert!(!requests.must_end_session());
    pump.close().map_err(|_| Error::Invalid)?;
    assert!(requests.must_end_session());
    Ok(())
}
#[test]
fn test_should_mute_remaining_fragments_if_scope_changes_after_first_fifo_admission()
-> Result<(), Error> {
    let access = Access::new();
    let owner = SharedContentOwner::<_, 16, 4, 8, 8>::new(Ports::new(&access));
    let clock = Clock(Cell::new(0));
    let entries = entries();
    let handler = ContentHandler::new(&owner, binding(&access, &entries)?, &access, &clock)?;
    let host = ContentHost::new(handler);
    host.with_handler(|handler| {
        handler.request_action(device_content::Action::Sos {
            source: 1,
            utc_ms: 0,
            time_valid: false,
        })
    })?;
    let channel_owner = SharedControlChannel::new();
    let catalogue = catalogue();
    let (_control, event_channel) = paired(&channel_owner, &catalogue, &mut host.requests())?;
    let mut pump = EventPump::new(event_channel, host.events()).map_err(|_| Error::Invalid)?;
    let mut queue = Queue {
        available: true,
        calls: 0,
        frames: Vec::new(),
    };
    assert!(
        pump.flush(0, EventPlane::Control, &mut queue)
            .map_err(|_| Error::Invalid)?
    );
    access.generation.set(2);
    let error = pump
        .flush(1, EventPlane::Control, &mut queue)
        .err()
        .ok_or(Error::Invalid)?;
    assert_eq!(
        error.failure,
        device_connection::PumpFailure::AuthorizationRevoked
    );
    assert_eq!(queue.frames.len(), 1);
    owner.with_backend(|ports| {
        assert_eq!(ports.actions.calls, 0);
        Ok(())
    })?;
    assert_eq!(owner.cleanup_report().unknown_count, 1);
    Ok(())
}
