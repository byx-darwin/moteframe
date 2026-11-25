//! Thin integration of the upstream `TrouBLE` Host; callers supply all GATT services.
#![no_std]
#![forbid(unsafe_code)]

mod polling;
pub use polling::{HostRunners, HostRunnersClosed, poll_host_runners};

use ble_controller::RandomAddress;
/// Upstream Host APIs, including caller-defined attribute tables and GATT events.
pub use trouble_host as host;
/// Fixed resources for one peripheral and three L2CAP channels, without bonds.
pub type Resources = host::HostResources<host::prelude::DefaultPacketPool, 1, 3, 1, 0>;
/// Builds the actual upstream Host with a validated static random identity.
///
/// Poll its runner concurrently with peripheral/GATT futures. The final board
/// binary must supply real critical sections and an embassy monotonic time
/// driver. No service, application protocol or scene is created here.
pub fn build<C: host::Controller>(
    controller: C,
    resources: &mut Resources,
    address: RandomAddress,
) -> host::Stack<'_, C, host::prelude::DefaultPacketPool> {
    host::new(controller, resources)
        .set_random_address(host::Address::random(address.bytes()))
        .build()
}
