//! Optional, RAM-only application profile on the existing `TrouBLE` Host.

use ble_host_trouble::host::{
    att::{AttClient, AttReq},
    prelude::{
        AttErrorCode, AttributeTable, Characteristic, CharacteristicProp, Error, GattConnection,
        GattConnectionEvent, GattEvent, PacketPool, Service,
    },
};
use embassy_sync::blocking_mutex::raw::RawMutex;
use moteframe_ble::{WriteBufferError, WriteSlot};

/// Custom primary service UUID in Bluetooth byte order.
pub const SERVICE_UUID: [u8; 16] = 0x2f80_29d0_7ff7_48b0_a64e_d6eb_c735_b251_u128.to_le_bytes();
/// Custom read/write characteristic UUID in Bluetooth byte order.
pub const VALUE_UUID: [u8; 16] = 0x2f80_29d1_7ff7_48b0_a64e_d6eb_c735_b251_u128.to_le_bytes();
/// Primary service, characteristic declaration/value, and notification CCCD.
pub const ATTRIBUTE_COUNT: usize = 4;
/// Fixed size of the characteristic and each committed write.
pub const VALUE_LEN: usize = 8;

/// Complete committed writes routed by their characteristic value handle.
///
/// Give each accepted connection a fresh slot. The application consumes the
/// commit with `take_into` before allowing another write.
pub type CommittedWrites = WriteSlot<u16, VALUE_LEN>;

/// Insufficient upstream attribute or handle capacity to add the profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileCapacity;

/// Progress made while handling one upstream connection event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GattProgress {
    /// A request or another event produced no application write.
    Continue,
    /// A full write was committed and is ready in the application slot.
    Committed,
    /// The Host reported a disconnection.
    Disconnected,
}

/// Failure to process an upstream event or record the application commit.
#[derive(Debug)]
pub enum GattError {
    /// The Host could not process or build a reply to the event.
    Host(Error),
    /// The committed write could not be pushed into the application slot.
    Slot(WriteBufferError),
}

/// One primary service and one fixed-length read/write/notify characteristic.
#[derive(Debug)]
pub struct GattProfile {
    value: Characteristic<[u8; VALUE_LEN]>,
}

impl GattProfile {
    /// Adds the profile to the caller's table and initializes its value to eight zero bytes.
    ///
    /// Keep the value storage and attribute server across connections to retain
    /// the last value until a reset. Each connection has its own application slot.
    ///
    /// # Errors
    /// Returns `ProfileCapacity` without mutation if the table has insufficient space.
    pub fn add<'values, M: RawMutex, const N: usize>(
        table: &mut AttributeTable<'values, M, N>,
        storage: &'values mut [u8; VALUE_LEN],
    ) -> Result<Self, ProfileCapacity> {
        let required = table
            .len()
            .checked_add(ATTRIBUTE_COUNT)
            .ok_or(ProfileCapacity)?;
        if required > N || required >= usize::from(u16::MAX) {
            return Err(ProfileCapacity);
        }

        let mut service = table.add_service(Service::new(SERVICE_UUID));
        let value = service
            .add_characteristic(
                VALUE_UUID,
                [
                    CharacteristicProp::Read,
                    CharacteristicProp::Write,
                    CharacteristicProp::Notify,
                ],
                [0; VALUE_LEN],
                storage,
            )
            .build();
        service.build();
        Ok(Self { value })
    }

    pub(crate) async fn echo<P: PacketPool>(
        &self,
        connection: &GattConnection<'_, '_, P>,
        value: &[u8; VALUE_LEN],
    ) -> Result<(), GattError> {
        // The write already updated the attribute. TrouBLE checks this peer's
        // CCCD and queues the notification on the same ATT bearer as the reply.
        self.value
            .notify(connection, value, false)
            .await
            .map_err(GattError::Host)
    }

    /// Lets the Host process one GATT event and records each accepted value write.
    ///
    /// Poll this future alongside the actual Host runner. Write Commands are
    /// silently discarded. Wrong-length requests and writes while the slot is
    /// occupied receive ATT errors before they can change the stored value.
    ///
    /// # Errors
    /// Returns an error if the Host cannot build a reply or the slot cannot record
    /// a committed write. The connection owner should terminate that session.
    pub async fn handle<P: PacketPool>(
        &self,
        event: GattConnectionEvent<'_, '_, P>,
        writes: &mut CommittedWrites,
    ) -> Result<GattProgress, GattError> {
        let event = match event {
            GattConnectionEvent::Gatt { event } => event,
            GattConnectionEvent::Disconnected { .. } => return Ok(GattProgress::Disconnected),
            _ => return Ok(GattProgress::Continue),
        };

        // Consuming the payload prevents the event's drop handler from processing
        // the command as a write or generating a reply for a forbidden handle.
        if matches!(event.payload().incoming(), AttClient::Command(_)) {
            drop(event.into_payload());
            return Ok(GattProgress::Continue);
        }

        let (reply, accepted) = match event {
            GattEvent::Write(write) if write.handle() == self.value.handle => {
                let request = match write.payload().incoming() {
                    AttClient::Request(AttReq::Write { data, .. }) => {
                        <[u8; VALUE_LEN]>::try_from(data)
                            .map_err(|_| AttErrorCode::INVALID_ATTRIBUTE_VALUE_LENGTH)
                    }
                    _ => Err(AttErrorCode::REQUEST_NOT_SUPPORTED),
                };
                match request {
                    Ok(value) if writes.peek().is_none() => (write.accept(), Some(value)),
                    Ok(_) => (write.reject(AttErrorCode::INSUFFICIENT_RESOURCES), None),
                    Err(code) => (write.reject(code), None),
                }
            }
            _ => (event.accept(), None),
        };

        let reply = reply.map_err(GattError::Host)?;
        let progress = if let Some(bytes) = accepted {
            // The upstream accept path has committed the exact value into RAM.
            writes
                .push(self.value.handle, &bytes)
                .map_err(GattError::Slot)?;
            GattProgress::Committed
        } else {
            GattProgress::Continue
        };
        reply.send().await;
        Ok(progress)
    }
}
