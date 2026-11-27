//! Foreground peripheral application over an independently supplied radio backend.

use crate::gatt::{
    ATTRIBUTE_COUNT, CommittedWrites, GattError, GattProfile, GattProgress, Notification, VALUE_LEN,
};
use ble_controller::{
    AdvertisingData, Controller, ControllerError, HciController, HciError, LinkBackend,
    RandomAddress, Version,
};
pub use ble_host_trouble::Resources;
use ble_host_trouble::{
    HostPacketPool as DefaultPacketPool, build,
    host::prelude::{
        Advertisement, AdvertisementParameters, AttributeServer, AttributeTable, BleHostError,
        ConnectionEvent, Error, GAP_SERVICE_ATTRIBUTE_COUNT, GapConfig, Stack,
    },
    poll_host_runners,
};
use core::{
    convert::Infallible,
    fmt,
    future::{Future, poll_fn},
    pin::{Pin, pin},
    task::{Context, Poll},
};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use moteframe_ble::WriteBufferError;

const ATTRIBUTES: usize =
    GAP_SERVICE_ATTRIBUTE_COUNT + ATTRIBUTE_COUNT + if cfg!(feature = "ota-v1") { 3 } else { 0 };

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
    /// Number of valid bytes at the start of the buffer.
    pub length: usize,
    /// Storage for the value, zero-padded after the accepted bytes.
    pub bytes: [u8; VALUE_LEN],
}

impl fmt::Debug for CommittedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommittedValue")
            .field("handle", &self.handle)
            .field("length", &self.length)
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
pub fn run<B: LinkBackend, F: FnMut(GattReport)>(
    backend: B,
    resources: &mut Resources,
    identity: GattIdentity,
    report: F,
) -> impl Future<Output = GattRunError<B::Error>> {
    #[cfg(feature = "ble-gatt-dle")]
    {
        // Borrow the endpoint so moving the Host does not copy long-packet queues
        // into another future on the BLE task stack.
        async move {
            let hci = HciController::new(Controller::new(backend, identity.version));
            run_hci::<false, _, _, _>(hci.borrowed(), resources, identity, report, None, || Ok(()))
                .await
        }
    }
    #[cfg(not(feature = "ble-gatt-dle"))]
    {
        let hci = HciController::new(Controller::new(backend, identity.version));
        run_hci::<false, _, _, _>(hci, resources, identity, report, None, || Ok(()))
    }
}

/// Runs the same GATT application with owner-driven radio service.
/// The board advances the radio on IRQ, timer and software wakes, including
/// while the Host receive lane is blocked.
#[cfg(feature = "ble-gatt-event-driven")]
pub async fn run_event_driven<B: LinkBackend, F: FnMut(GattReport)>(
    backend: B,
    resources: &mut Resources,
    identity: GattIdentity,
    report: F,
) -> GattRunError<B::Error> {
    run_events(backend, resources, identity, report, None).await
}

/// Run the event-driven Host with the layout-v1 OTA write service. A `None`
/// frame signals a new connection and clears any abandoned session.
#[cfg(all(feature = "ble-gatt-event-driven", feature = "ota-v1"))]
pub async fn run_event_driven_ota<B: LinkBackend, F: FnMut(GattReport)>(
    backend: B,
    resources: &mut Resources,
    identity: GattIdentity,
    report: F,
    receive: &mut dyn FnMut(Option<&[u8]>) -> bool,
) -> GattRunError<B::Error> {
    run_events(backend, resources, identity, report, Some(receive)).await
}

type OtaCallback<'a> = Option<&'a mut dyn FnMut(Option<&[u8]>) -> bool>;

#[cfg(feature = "ble-gatt-event-driven")]
async fn run_events<B: LinkBackend, F: FnMut(GattReport)>(
    backend: B,
    resources: &mut Resources,
    identity: GattIdentity,
    report: F,
    ota: OtaCallback<'_>,
) -> GattRunError<B::Error> {
    let hci = HciController::new_with_external_wake(Controller::new(backend, identity.version));
    run_hci::<true, _, _, _>(hci.borrowed(), resources, identity, report, ota, || {
        hci.service()
    })
    .await
}

async fn run_hci<const EXTERNAL: bool, C, E, F>(
    hci: C,
    resources: &mut Resources,
    identity: GattIdentity,
    report: F,
    ota: OtaCallback<'_>,
    mut service: impl FnMut() -> Result<(), HciError<E>>,
) -> GattRunError<E>
where
    C: ble_host_trouble::host::Controller<Error = HciError<E>>,
    E: fmt::Debug,
    F: FnMut(GattReport),
{
    if AdvertisingData::new(identity.advertising_data).is_err() {
        return GattRunError::Host(BleHostError::BleHost(Error::InvalidValue));
    }
    let stack = build(hci, resources, identity.address);
    let (mut rx, mut control, mut tx) = stack.runner().split();
    let mut rx = pin!(rx.run());
    let mut transmit = pin!(async {
        loop {
            let result = tx.run().await;
            // For an initialized Host's data ACL, State from send_acl
            // denotes a backpressured write from an ended connection.
            if !EXTERNAL
                || !matches!(
                    result,
                    Err(BleHostError::Controller(HciError::Controller(
                        ControllerError::State
                    )))
                )
            {
                return result;
            }
        }
    });
    let mut control = pin!(control.run());
    let mut app = pin!(serve(&stack, identity, report, ota));
    poll_fn(|cx| {
        if let Err(error) = service() {
            return Poll::Ready(GattRunError::Host(BleHostError::Controller(error)));
        }
        let result = if EXTERNAL {
            // The Controller has already advanced. Deliver its observations
            // before the Host resumes any outgoing data.
            poll_host_runners(rx.as_mut(), transmit.as_mut(), control.as_mut(), cx)
        } else {
            poll_host_runners(transmit.as_mut(), rx.as_mut(), control.as_mut(), cx)
        };
        if let Poll::Ready(result) = result {
            return Poll::Ready(
                result
                    .err()
                    .map_or(GattRunError::HostStopped, GattRunError::Host),
            );
        }
        let application = if EXTERNAL {
            poll_application(app.as_mut(), cx)
        } else {
            app.as_mut().poll(cx)
        };
        application.map(|result| match result {
            Ok(never) => match never {},
            Err(error) => error,
        })
    })
    .await
}

// Keep the application poll frame separate from the controller and Host lanes.
#[inline(never)]
fn poll_application<F: Future>(app: Pin<&mut F>, cx: &mut Context<'_>) -> Poll<F::Output> {
    app.poll(cx)
}

#[cfg(feature = "ota-v1")]
fn add_ota_service(table: &mut AttributeTable<'_, NoopRawMutex, ATTRIBUTES>) -> u16 {
    use ble_host_trouble::host::prelude::{CharacteristicProp, Service};
    let mut service = table.add_service(Service::new(
        0x2f8029e0_7ff7_48b0_a64e_d6ebc735b251_u128.to_le_bytes(),
    ));
    let handle = service
        .add_characteristic_small(
            0x2f8029e1_7ff7_48b0_a64e_d6ebc735b251_u128.to_le_bytes(),
            [CharacteristicProp::Write],
            0_u8,
        )
        .build()
        .handle;
    service.build();
    handle
}

async fn serve<C, E, F>(
    stack: &Stack<'_, C, DefaultPacketPool>,
    identity: GattIdentity,
    mut report: F,
    mut ota: OtaCallback<'_>,
) -> Result<Infallible, GattRunError<E>>
where
    C: ble_host_trouble::host::Controller<Error = HciError<E>>,
    E: fmt::Debug,
    F: FnMut(GattReport),
{
    let mut storage = [0; VALUE_LEN];
    let mut table = AttributeTable::<NoopRawMutex, ATTRIBUTES>::new();
    GapConfig::default(identity.name)
        .build(&mut table)
        .map_err(GattRunError::Gap)?;
    let profile = GattProfile::add(&mut table, &mut storage).map_err(|_| GattRunError::Profile)?;
    #[cfg(feature = "ota-v1")]
    let ota_handle = ota.as_ref().map(|_| add_ota_service(&mut table));
    #[cfg(not(feature = "ota-v1"))]
    let ota_handle = None;
    let server = AttributeServer::<_, DefaultPacketPool, ATTRIBUTES, 1>::new(table);
    let mut parameters = AdvertisementParameters::default();
    if cfg!(all(
        feature = "ble-gatt-mac-probe",
        not(feature = "ble-gatt-xip")
    )) {
        // Increase the chances of overlapping the Mac's initiator scan window.
        let interval = core::time::Duration::from_millis(30)
            .try_into()
            .map_err(|_| GattRunError::Profile)?;
        parameters.interval_min = interval;
        parameters.interval_max = interval;
    }
    let mut peripheral = stack.peripheral();
    loop {
        let acceptor = peripheral
            .advertise(
                &parameters,
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
        if cfg!(feature = "ota-v1")
            && let Some(receive) = ota.as_mut()
        {
            receive(None);
        }
        report(GattReport::Connected);
        let mut writes = CommittedWrites::new();
        let out_of_memory = loop {
            let mut bytes = [0; VALUE_LEN];
            let notification = match profile
                .handle_with_ota(
                    connection.next().await,
                    &mut writes,
                    ota_handle,
                    &mut |frame| ota.as_mut().is_some_and(|f| f(frame)),
                )
                .await
            {
                Err(GattError::Host(Error::OutOfMemory)) => break true,
                Err(error) => return Err(GattRunError::Gatt(error)),
                Ok(GattProgress::Continue) => continue,
                Ok(GattProgress::SdkSubscribed) => Notification::SdkSubscription,
                Ok(GattProgress::Committed) => {
                    let meta = writes
                        .take_into(&mut bytes)
                        .map_err(GattRunError::WriteDelivery)?
                        .ok_or(GattRunError::MissingWrite)?;
                    report(GattReport::Committed(CommittedValue {
                        handle: meta.target,
                        length: meta.length,
                        bytes,
                    }));
                    Notification::Echo(&bytes[..meta.length])
                }
                Ok(GattProgress::Disconnected) => break false,
            };
            match profile.notify(&connection, notification).await {
                Err(GattError::Host(Error::OutOfMemory)) => break true,
                Err(error) => return Err(GattRunError::Gatt(error)),
                Ok(()) => {}
            }
        };
        if out_of_memory {
            // A write may already have been accepted. Do not replay it.
            let link = connection.raw();
            link.disconnect();
            while !matches!(link.next().await, ConnectionEvent::Disconnected { .. }) {}
        }
        report(GattReport::Disconnected);
    }
}
