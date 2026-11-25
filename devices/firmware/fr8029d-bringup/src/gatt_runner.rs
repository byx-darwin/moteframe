//! Foreground peripheral application over an independently supplied radio backend.

use crate::gatt::{
    ATTRIBUTE_COUNT, CommittedWrites, GattError, GattProfile, GattProgress, VALUE_LEN,
};
use ble_controller::{
    AdvertisingData, Controller, HciController, HciError, LinkBackend, RandomAddress, Version,
};
pub use ble_host_trouble::Resources;
use ble_host_trouble::{
    build,
    host::prelude::{
        Advertisement, AdvertisementParameters, AttributeServer, AttributeTable, BleHostError,
        DefaultPacketPool, Error, GAP_SERVICE_ATTRIBUTE_COUNT, GapConfig, Stack,
    },
    poll_host_runners,
};
use core::{
    convert::Infallible,
    fmt,
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use moteframe_ble::WriteBufferError;

const ATTRIBUTES: usize = GAP_SERVICE_ATTRIBUTE_COUNT + ATTRIBUTE_COUNT;

/// Static identity and legacy advertising payload assigned by the board.
#[derive(Clone, Copy)]
pub struct GattIdentity {
    /// Validated random static advertiser address.
    pub address: RandomAddress,
    /// Local HCI version returned during Host initialization.
    pub version: Version,
    /// Readable GAP Device Name.
    pub name: &'static str,
    /// Complete legacy advertising data with a maximum length of 31 bytes.
    pub advertising_data: &'static [u8],
}

impl fmt::Debug for GattIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GattIdentity")
            .field("address", &self.address)
            .field("version", &self.version)
            .field("name", &self.name)
            .field("advertising_data_len", &self.advertising_data.len())
            .finish()
    }
}

/// A full RAM value consumed from the connection's `MoteFrame` write slot.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct CommittedValue {
    /// Original attribute handle carried by the write.
    pub handle: u16,
    /// Bytes accepted by the GATT server.
    pub bytes: [u8; VALUE_LEN],
}

impl fmt::Debug for CommittedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommittedValue")
            .field("handle", &self.handle)
            .field("length", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// Application milestones delivered in connection order.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GattReport {
    /// The Controller enabled legacy connectable advertising.
    Advertising,
    /// The Host accepted a connection and attached the attribute server.
    Connected,
    /// The application consumed a committed write after Host reply admission.
    Committed(CommittedValue),
    /// The Host reported the active connection had ended.
    Disconnected,
}

/// The first fault that stopped a foreground run.
#[derive(Debug)]
pub enum GattRunError<E> {
    /// The GAP configuration was invalid.
    Gap(&'static str),
    /// The upstream table could not accommodate the application profile.
    Profile,
    /// A Host or Controller operation failed.
    Host(BleHostError<HciError<E>>),
    /// A GATT event could not be handled.
    Gatt(GattError),
    /// An application write could not be consumed.
    WriteDelivery(WriteBufferError),
    /// The profile reported a commit without a complete value.
    MissingWrite,
    /// A background Host lane returned without a fault.
    HostStopped,
}

/// Polls upstream Host tasks and runs the lab peripheral until the first fault.
///
/// Pin and poll on a single foreground executor with a board-provided critical
/// section and Embassy monotonic time driver. Leave the table and value alive
/// across connections. The reporter should return promptly to allow radio polling.
///
/// Returns the first fault for the board to handle after the pending tasks drop.
pub async fn run<B: LinkBackend, F: FnMut(GattReport)>(
    backend: B,
    resources: &mut Resources,
    identity: GattIdentity,
    report: F,
) -> GattRunError<B::Error> {
    if AdvertisingData::new(identity.advertising_data).is_err() {
        return GattRunError::Host(BleHostError::BleHost(Error::InvalidValue));
    }
    let hci = HciController::new(Controller::new(backend, identity.version));
    let stack = build(hci, resources, identity.address);
    let (mut rx, mut control, mut tx) = stack.runner().split();
    let mut rx = pin!(rx.run());
    let mut tx = pin!(tx.run());
    let mut control = pin!(control.run());
    let mut app = pin!(serve(&stack, identity, report));
    poll_fn(|cx| {
        if let Poll::Ready(result) =
            poll_host_runners(tx.as_mut(), rx.as_mut(), control.as_mut(), cx)
        {
            return Poll::Ready(
                result
                    .err()
                    .map_or(GattRunError::HostStopped, GattRunError::Host),
            );
        }
        app.as_mut().poll(cx).map(|result| match result {
            Ok(never) => match never {},
            Err(error) => error,
        })
    })
    .await
}

async fn serve<B: LinkBackend, F: FnMut(GattReport)>(
    stack: &Stack<'_, HciController<B>, DefaultPacketPool>,
    identity: GattIdentity,
    mut report: F,
) -> Result<Infallible, GattRunError<B::Error>> {
    let mut storage = [0; VALUE_LEN];
    let mut table = AttributeTable::<NoopRawMutex, ATTRIBUTES>::new();
    GapConfig::default(identity.name)
        .build(&mut table)
        .map_err(GattRunError::Gap)?;
    let profile = GattProfile::add(&mut table, &mut storage).map_err(|_| GattRunError::Profile)?;
    let server = AttributeServer::<_, DefaultPacketPool, ATTRIBUTES, 1>::new(table);
    let mut peripheral = stack.peripheral();
    loop {
        let acceptor = peripheral
            .advertise(
                &AdvertisementParameters::default(),
                Advertisement::ConnectableScannableUndirected {
                    adv_data: identity.advertising_data,
                    scan_data: &[],
                },
            )
            .await
            .map_err(GattRunError::Host)?;
        report(GattReport::Advertising);
        let connection = acceptor
            .accept()
            .await
            .and_then(|connection| connection.with_attribute_server(&server))
            .map_err(|error| GattRunError::Host(BleHostError::BleHost(error)))?;
        report(GattReport::Connected);
        let mut writes = CommittedWrites::new();
        loop {
            match profile
                .handle(connection.next().await, &mut writes)
                .await
                .map_err(GattRunError::Gatt)?
            {
                GattProgress::Continue => {}
                GattProgress::Committed => {
                    let mut bytes = [0; VALUE_LEN];
                    let meta = writes
                        .take_into(&mut bytes)
                        .map_err(GattRunError::WriteDelivery)?
                        .filter(|meta| meta.length == VALUE_LEN)
                        .ok_or(GattRunError::MissingWrite)?;
                    report(GattReport::Committed(CommittedValue {
                        handle: meta.target,
                        bytes,
                    }));
                    profile
                        .echo(&connection, &bytes)
                        .await
                        .map_err(GattRunError::Gatt)?;
                }
                GattProgress::Disconnected => break,
            }
        }
        report(GattReport::Disconnected);
    }
}
