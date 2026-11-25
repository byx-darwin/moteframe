//! Offline trusted clock fixture and real durable time owner, never production data.
use super::medium;
use device_services::{ClockSourceError, TimeAccess};
use device_storage::persistence::{AtomicSnapshot, Layout};
use device_time_store::{ClockReading, TimeStore, TrustedClock};
use std::cell::Cell;
#[derive(Debug)]
pub struct Clock(Cell<u64>);
impl TrustedClock for Clock {
    fn reading(&self) -> Result<ClockReading, ClockSourceError> {
        Ok(ClockReading {
            source_id: 7,
            epoch: 11,
            mono_us: self.0.get(),
        })
    }
}
pub struct TimeScope;
impl TimeAccess for TimeScope {
    fn allowed(&self, _: bool) -> bool {
        true
    }
}
pub type TimeOwner = TimeStore<AtomicSnapshot<medium::Memory, 128>, Clock>;
pub fn time_owner() -> TimeOwner {
    let owner = TimeStore::open(
        AtomicSnapshot::open(
            medium::Memory::new(),
            Layout {
                start: 0,
                bank_bytes: 1024,
            },
        )
        .unwrap(),
        Clock(Cell::new(0)),
        20,
    )
    .unwrap();
    owner.synchronize(1_761_696_000_000, 4, &TimeScope).unwrap();
    owner
}
