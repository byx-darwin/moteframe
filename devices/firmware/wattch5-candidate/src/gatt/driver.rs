use super::{Bridge, ControlProfile, fifo::Frame};
use ble_host_trouble::host::{
    att::{AttClient, AttCmd},
    prelude::{AttErrorCode, GattConnection, GattConnectionEvent, GattEvent, PacketPool},
};
use core::{
    future::{Future, poll_fn},
    pin::pin,
    task::{Context, Poll},
};
use device_runtime::PortError;

fn poll_event<'stack, 'server, P: PacketPool>(
    connection: &GattConnection<'stack, 'server, P>,
    cx: &mut Context<'_>,
) -> Poll<GattConnectionEvent<'stack, 'server, P>> {
    pin!(connection.next()).as_mut().poll(cx)
}
fn handle<P: PacketPool, const RX: usize, const TX: usize>(
    event: GattConnectionEvent<'_, '_, P>,
    connection: &GattConnection<'_, '_, P>,
    profile: &ControlProfile,
    bridge: &Bridge<RX, TX>,
    epoch: u64,
) -> Result<bool, PortError> {
    match event {
        GattConnectionEvent::Disconnected { .. } => {
            bridge.end(epoch)?;
            return Ok(true);
        }
        GattConnectionEvent::Gatt {
            event: GattEvent::Write(write),
        } if write.handle() == profile.rx().handle => {
            let command = matches!(
                write.payload().incoming(),
                AttClient::Command(AttCmd::Write { .. })
            );
            let admitted = write.with_data(|offset, bytes| bridge.incoming(epoch, offset, bytes));
            let reply = match admitted {
                Ok(()) => write.accept_unprocessed(),
                Err(error) => {
                    let code = match error {
                        PortError::InvalidInput => AttErrorCode::INVALID_ATTRIBUTE_VALUE_LENGTH,
                        PortError::Unavailable => AttErrorCode::INSUFFICIENT_RESOURCES,
                        PortError::Fault | PortError::Uncertain => AttErrorCode::UNLIKELY_ERROR,
                    };
                    let reply = write.reject(code).map_err(|_| PortError::Fault)?;
                    if command {
                        drop(reply);
                        return Err(PortError::Fault);
                    }
                    Ok(reply)
                }
            }
            .map_err(|_| PortError::Fault)?;
            reply.try_send().map_err(|_| PortError::Uncertain)?;
        }
        GattConnectionEvent::Gatt {
            event: GattEvent::Write(write),
        } if Some(write.handle()) == profile.tx().cccd_handle => {
            let valid = write.with_data(|offset, bytes| offset == 0 && matches!(bytes, [0 | 1, 0]));
            if valid {
                write
                    .accept()
                    .map_err(|_| PortError::Fault)?
                    .try_send()
                    .map_err(|_| PortError::Uncertain)?;
                bridge.subscription(epoch, profile.tx().should_notify(connection))?;
            } else {
                write
                    .reject(AttErrorCode::VALUE_NOT_ALLOWED)
                    .map_err(|_| PortError::Fault)?
                    .try_send()
                    .map_err(|_| PortError::Uncertain)?;
            }
        }
        GattConnectionEvent::Gatt {
            event: GattEvent::Write(write),
        } if Some(write.handle()) == profile.data().cccd_handle => {
            let valid = write.with_data(|offset, bytes| offset == 0 && matches!(bytes, [0 | 1, 0]));
            if valid {
                write
                    .accept()
                    .map_err(|_| PortError::Fault)?
                    .try_send()
                    .map_err(|_| PortError::Uncertain)?;
                bridge.data_subscription(epoch, profile.data().should_notify(connection))?;
            } else {
                write
                    .reject(AttErrorCode::VALUE_NOT_ALLOWED)
                    .map_err(|_| PortError::Fault)?
                    .try_send()
                    .map_err(|_| PortError::Uncertain)?;
            }
        }
        GattConnectionEvent::Gatt { event } => {
            event
                .accept()
                .map_err(|_| PortError::Fault)?
                .try_send()
                .map_err(|_| PortError::Uncertain)?;
        }
        _ => {}
    }
    Ok(false)
}
async fn yield_once() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}
async fn notify<P: PacketPool, const RX: usize, const TX: usize>(
    connection: &GattConnection<'_, '_, P>,
    profile: &ControlProfile,
    bridge: &Bridge<RX, TX>,
    epoch: u64,
    frame: &Frame,
) -> Result<bool, PortError> {
    let characteristic = match frame.plane {
        device_connection::EventPlane::Control => profile.tx(),
        device_connection::EventPlane::Data => profile.data(),
    };
    let mut future = pin!(characteristic.notify_raw(connection, &frame.bytes[..frame.len], false));
    poll_fn(|cx| {
        if bridge.disconnect_requested(epoch)? {
            return Poll::Ready(Err(PortError::Fault));
        }
        if let Poll::Ready(event) = poll_event(connection, cx)
            && handle(event, connection, profile, bridge, epoch)?
        {
            return Poll::Ready(Ok(true));
        }
        // Upstream notify_raw silently returns Ok for an unsubscribed connection.
        // Never treat that branch as a completed send, and never poll stale work.
        if bridge.scope()? != Some(frame.scope) {
            return Poll::Ready(Ok(false));
        }
        if !characteristic.should_notify(connection) {
            match frame.plane {
                device_connection::EventPlane::Control => bridge.subscription(epoch, false)?,
                device_connection::EventPlane::Data => bridge.data_subscription(epoch, false)?,
            };
            return Poll::Ready(Ok(false));
        }
        match future.as_mut().poll(cx) {
            Poll::Ready(result) => {
                Poll::Ready(result.map(|()| false).map_err(|_| PortError::Uncertain))
            }
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}
async fn run<P: PacketPool, const RX: usize, const TX: usize>(
    connection: &GattConnection<'_, '_, P>,
    profile: &ControlProfile,
    bridge: &Bridge<RX, TX>,
    epoch: u64,
) -> Result<(), PortError> {
    bridge.subscription(epoch, profile.tx().should_notify(connection))?;
    bridge.data_subscription(epoch, profile.data().should_notify(connection))?;
    loop {
        let frame = poll_fn(|cx| {
            if bridge.disconnect_requested(epoch)? {
                return Poll::Ready(Err(PortError::Fault));
            }
            let mut handled = false;
            if let Poll::Ready(event) = poll_event(connection, cx) {
                if handle(event, connection, profile, bridge, epoch)? {
                    return Poll::Ready(Ok(None));
                }
                handled = true;
            }
            if let Some(frame) = bridge.outgoing(epoch)? {
                return Poll::Ready(Ok(Some(Some(frame))));
            }
            if handled {
                Poll::Ready(Ok(Some(None)))
            } else {
                Poll::Pending
            }
        })
        .await?;
        let Some(frame) = frame else {
            return Ok(());
        };
        if let Some(frame) = frame
            && notify(connection, profile, bridge, epoch, &frame).await?
        {
            return Ok(());
        }
        yield_once().await;
    }
}
/// Drive one actual accepted upstream connection, its CONTROL writes and notifications.
///
/// Keep this future pinned externally and lend it, with the actual pinned Host
/// `runner.run()` future, to `TroubleTransport`. New accepted connections reuse
/// the same `Bridge`; no identity, advertising or trust is manufactured here.
/// One driver poll handles at most two GATT events and one notification poll,
/// then yields. Pending notification futures retain their exact owned frame.
/// No FIFO guard or borrowed vendor value crosses an await.
/// # Errors
/// Invalid actual MTU, concurrent driver ownership, Host/ATT admission or notification
/// failures mute/drop this connection's work and explicitly request disconnect.
pub async fn drive_connection<P: PacketPool, const RX: usize, const TX: usize>(
    connection: &GattConnection<'_, '_, P>,
    profile: &ControlProfile,
    bridge: &Bridge<RX, TX>,
) -> Result<(), PortError> {
    if !connection.raw().is_connected() || connection.raw().att_mtu() < 23 {
        return Err(PortError::InvalidInput);
    }
    let epoch = bridge.begin()?;
    let result = run(connection, profile, bridge, epoch).await;
    let cleanup = bridge.end(epoch);
    if result.is_err() || cleanup.is_err() {
        connection.raw().disconnect();
    }
    result.and(cleanup)
}
