//! Optional, RAM-only application profile on the existing `TrouBLE` Host.

use ble_host_trouble::host::{
    att::{AttClient, AttReq},
    prelude::{
        AttErrorCode, AttributeTable, Characteristic, CharacteristicProp, Error, GattConnection,
        GattConnectionEvent, GattEvent, PacketPool, PermissionLevel, Service,
    },
};
use embassy_sync::blocking_mutex::raw::RawMutex;
use moteframe_ble::{WriteBufferError, WriteSlot};

/// Custom primary service UUID in Bluetooth byte order.
pub const SERVICE_UUID: [u8; 16] = 0x2f80_29d0_7ff7_48b0_a64e_d6eb_c735_b251_u128.to_le_bytes();
/// Custom read/write characteristic UUID in Bluetooth byte order.
pub const VALUE_UUID: [u8; 16] = 0x2f80_29d1_7ff7_48b0_a64e_d6eb_c735_b251_u128.to_le_bytes();
/// Four `MoteFrame` attributes followed by the eight SDK simple peripheral attributes.
pub const ATTRIBUTE_COUNT: usize = 4 + 8;
const SDK_SERVICE_UUID: u16 = 0xfff0;
const SDK_VALUE_UUID: u16 = 0xfff3;
const SDK_READ_ONLY_UUID: [u8; 16] = 0x0783_b03e_8535_b5a0_7140_a304_f4ff_5cba_u128.to_le_bytes();
const USER_DESCRIPTION_UUID: u16 = 0x2901;
const SDK_NOTIFICATION: &[u8] = &[1, 2, 3, 4, 5];
/// Maximum application value at the default ATT MTU (23 minus opcode and handle).
pub const VALUE_LEN: usize = 20;
type Value = heapless::Vec<u8, VALUE_LEN>;

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
    /// The SDK notification CCCD accepted a subscription.
    SdkSubscribed,
    /// The Host reported a disconnection.
    Disconnected,
}

pub(crate) enum Notification<'a> {
    Echo(&'a [u8]),
    SdkSubscription,
}

/// Failure to process an upstream event or record the application commit.
#[derive(Debug)]
pub enum GattError {
    /// The Host could not process or build a reply to the event.
    Host(Error),
    /// The committed write could not be pushed into the application slot.
    Slot(WriteBufferError),
}

/// The `MoteFrame` echo service and the `FR802x` SDK simple peripheral service.
#[derive(Debug)]
pub struct GattProfile {
    value: Characteristic<[u8]>,
    sdk_value: Characteristic<[u8]>,
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

        let properties = [
            CharacteristicProp::Read,
            CharacteristicProp::Write,
            CharacteristicProp::Notify,
        ];
        let mut service = table.add_service(Service::new(SERVICE_UUID));
        let value = service
            .add_characteristic(
                VALUE_UUID,
                properties,
                Value::from_slice(&[0; 8]).map_err(|_| ProfileCapacity)?,
                storage,
            )
            .build()
            .to_raw();
        service.build();

        let mut sdk = table.add_service(Service::new(SDK_SERVICE_UUID));
        let mut first =
            sdk.add_characteristic_small(SDK_VALUE_UUID, properties, [0_u8, 1, 2, 3, 4]);
        // The SDK exposes the C strings including their terminating zero byte.
        first.add_descriptor_ro(
            USER_DESCRIPTION_UUID,
            PermissionLevel::Allowed,
            b"Characteristic 1\0",
        );
        let sdk_value = first.build().to_raw();
        let mut second =
            sdk.add_characteristic_ro(SDK_READ_ONLY_UUID, &[0x11_u8, 0x22, 0x33, 0x44, 0x55]);
        second.add_descriptor_ro(
            USER_DESCRIPTION_UUID,
            PermissionLevel::Allowed,
            b"Characteristic 2\0",
        );
        second.build();
        sdk.build();
        Ok(Self { value, sdk_value })
    }

    pub(crate) async fn notify<P: PacketPool>(
        &self,
        connection: &GattConnection<'_, '_, P>,
        notification: Notification<'_>,
    ) -> Result<(), GattError> {
        // Both responses leave the readable value intact. TrouBLE checks this
        // connection's corresponding CCCD before sending on the ATT bearer.
        let (characteristic, value) = match notification {
            Notification::Echo(bytes) => (&self.value, bytes),
            Notification::SdkSubscription => (&self.sdk_value, SDK_NOTIFICATION),
        };
        characteristic
            .notify_raw(connection, value, false)
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
        self.handle_with_ota(event, writes, None, &mut |_| false)
            .await
    }

    pub(crate) async fn handle_with_ota<P: PacketPool>(
        &self,
        event: GattConnectionEvent<'_, '_, P>,
        writes: &mut CommittedWrites,
        ota_handle: Option<u16>,
        dispatch: &mut impl FnMut(Option<&[u8]>) -> bool,
    ) -> Result<GattProgress, GattError> {
        let event = match event {
            GattConnectionEvent::Gatt { event } => event,
            GattConnectionEvent::Disconnected { .. } => return Ok(GattProgress::Disconnected),
            _ => return Ok(GattProgress::Continue),
        };

        let (reply, progress) = match event {
            GattEvent::Write(write) => {
                let AttClient::Request(AttReq::Write { handle, data }) = write.payload().incoming()
                else {
                    // Drop the payload itself: dropping the event would process a Write Command.
                    drop(write.into_payload());
                    return Ok(GattProgress::Continue);
                };
                if cfg!(feature = "ota-v1") && Some(handle) == ota_handle {
                    let reply = if dispatch(Some(data)) {
                        write.accept_unprocessed()
                    } else {
                        write.reject(AttErrorCode::VALUE_NOT_ALLOWED)
                    };
                    reply.map_err(GattError::Host)?.send().await;
                    return Ok(GattProgress::Continue);
                }
                let progress = if handle == self.value.handle {
                    if !(1..=VALUE_LEN).contains(&data.len()) {
                        Err(AttErrorCode::INVALID_ATTRIBUTE_VALUE_LENGTH)
                    } else if writes.peek().is_some() {
                        Err(AttErrorCode::INSUFFICIENT_RESOURCES)
                    } else {
                        writes.push(handle, data).map_err(GattError::Slot)?;
                        Ok(GattProgress::Committed)
                    }
                } else if Some(handle) == self.sdk_value.cccd_handle {
                    match data {
                        [0, 0] => Ok(GattProgress::Continue),
                        [1, 0] => Ok(GattProgress::SdkSubscribed),
                        [_, _] => Err(AttErrorCode::VALUE_NOT_ALLOWED),
                        _ => Err(AttErrorCode::INVALID_ATTRIBUTE_VALUE_LENGTH),
                    }
                } else if handle == self.sdk_value.handle && data.len() > VALUE_LEN {
                    Err(AttErrorCode::INVALID_ATTRIBUTE_VALUE_LENGTH)
                } else {
                    Ok(GattProgress::Continue)
                };
                match progress {
                    Err(code) => (write.reject(code), GattProgress::Continue),
                    Ok(progress) => {
                        // The SDK acknowledges value writes without storing them.
                        let reply = if handle == self.sdk_value.handle {
                            write.accept_unprocessed()
                        } else {
                            write.accept()
                        };
                        if reply.is_err() && progress == GattProgress::Committed {
                            writes.clear();
                        }
                        (reply, progress)
                    }
                }
            }
            _ => (event.accept(), GattProgress::Continue),
        };
        let reply = reply.map_err(GattError::Host)?;
        reply.send().await;
        Ok(progress)
    }
}
