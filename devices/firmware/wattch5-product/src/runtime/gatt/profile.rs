use ble_host_trouble::host::prelude::{
    AsGatt, AttributeTable, Characteristic, CharacteristicProp, Service, Uuid,
};
use device_runtime::PortError;
use embassy_sync::blocking_mutex::raw::RawMutex;

/// Actual value ceiling supported by the pinned MTU27 packet pool (ATT MTU23).
pub const CONTROL_MAX: usize = 20;
/// Caller-owned product identity; this module invents no service or characteristic UUID.
#[derive(Debug, Clone, Copy)]
pub struct ControlUuids {
    /// Actual CONTROL service UUID.
    pub service: Uuid,
    /// Actual incoming write characteristic UUID.
    pub rx: Uuid,
    /// Actual outgoing notification characteristic UUID.
    pub tx: Uuid,
}
#[derive(Debug)]
struct EmptyControl;
impl AsGatt for EmptyControl {
    const MIN_SIZE: usize = 0;
    const MAX_SIZE: usize = CONTROL_MAX;
    fn as_gatt(&self) -> &[u8] {
        &[]
    }
}
/// Handles built from the actual caller-supplied table and UUIDs.
#[derive(Debug)]
pub struct ControlProfile {
    rx: Characteristic<[u8]>,
    tx: Characteristic<[u8]>,
}
impl ControlProfile {
    /// Add variable-length CONTROL Write and Notify values with exact caller storage.
    /// No transport security or application authority is inferred from these handles.
    /// # Errors
    /// Insufficient table/handle capacity is rejected before any table mutation.
    pub fn add<'values, M: RawMutex, const N: usize>(
        table: &mut AttributeTable<'values, M, N>,
        uuids: ControlUuids,
        rx: &'values mut [u8; CONTROL_MAX],
        tx: &'values mut [u8; CONTROL_MAX],
    ) -> Result<Self, PortError> {
        let required = table.len().checked_add(6).ok_or(PortError::InvalidInput)?;
        if required > N || required >= usize::from(u16::MAX) {
            return Err(PortError::InvalidInput);
        }
        let mut service = table.add_service(Service::new(uuids.service));
        let rx = service
            .add_characteristic(uuids.rx, [CharacteristicProp::Write], EmptyControl, rx)
            .to_raw()
            .build();
        let tx = service
            .add_characteristic(uuids.tx, [CharacteristicProp::Notify], EmptyControl, tx)
            .to_raw()
            .build();
        service.build();
        Ok(Self { rx, tx })
    }
    /// Actual incoming variable-length characteristic handle.
    #[must_use]
    pub const fn rx(&self) -> &Characteristic<[u8]> {
        &self.rx
    }
    /// Actual outgoing variable-length characteristic and CCCD handles.
    #[must_use]
    pub const fn tx(&self) -> &Characteristic<[u8]> {
        &self.tx
    }
}
