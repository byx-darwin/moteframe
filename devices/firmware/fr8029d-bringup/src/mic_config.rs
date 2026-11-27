//! Foreground MIC sample decimation and deferred, readback-checked persistence.
use core::sync::atomic::{AtomicU32, Ordering};

const DEFAULT: u16 = 4;
const DIRTY: u32 = 1 << 16;
static ACTIVE: AtomicU32 = AtomicU32::new(DEFAULT as u32);

pub(crate) fn value() -> u16 {
    let bytes = ACTIVE.load(Ordering::Relaxed).to_le_bytes();
    u16::from_le_bytes([bytes[0], bytes[1]])
}

pub(crate) fn pending() -> bool {
    ACTIVE.load(Ordering::Relaxed) & DIRTY != 0
}

/// Immediately changes the sample stride, without touching QSPI. Reapplying a
/// default also requests persistence when the slots were initially blank.
pub(crate) fn apply(value: u16) {
    if (1..=64).contains(&value) {
        ACTIVE.store(u32::from(value) | DIRTY, Ordering::Relaxed);
    }
}

/// Boot-only, before acquisition or advertising can start.
pub(crate) fn restore() {
    #[cfg(feature = "mic1-config-layout-v1")]
    layout::restore();
}

/// Called by the single board foreground after physical disconnect, with ADC
/// stopped, before requesting the next advertisement. That foreground owns
/// radio scheduling and keeps this entire synchronous interval reserved.
pub(crate) fn maintenance() {
    #[cfg(feature = "mic1-config-layout-v1")]
    layout::save();
}

#[cfg(feature = "mic1-config-layout-v1")]
#[path = "qspi_slots.rs"]
mod qspi_slots;

#[cfg(feature = "mic1-config-layout-v1")]
mod layout {
    use super::{ACTIVE, DIRTY, Ordering, qspi_slots::QspiSlots};
    use crate::qspi_config::{Error, PAGE, Partition};
    use moteframe_storage::{
        deferred::ConfigMedia,
        snapshot::{ConfigStore, StoreError},
    };

    // Product schema identifier ("MIC" + version 1); another product's snapshot
    // in these slots must never be treated as a writable MIC configuration.
    const VERSION: u32 = u32::from_le_bytes(*b"MIC\x01");

    #[unsafe(link_section = ".ramfunc.mic_config_stall")]
    extern "C" fn stall() -> ! {
        loop {
            core::hint::spin_loop();
        }
    }

    fn claim() -> Result<QspiSlots, Error> {
        // SAFETY: This opt-in image dedicates layout-v1's pair to this schema.
        // The board owner reserves QSPI and cache (including DMA/debugger),
        // signs off the immutable exception vectors, and provides independent
        // recovery. Boot/maintenance callers exclude ADC and radio activity.
        // QspiSlots checks the chip, XIP image bounds, controller and protection.
        unsafe { QspiSlots::claim(Partition::layout_v1(), stall) }
    }

    fn read(
        store: &mut ConfigStore<'_, QspiSlots, PAGE>,
    ) -> Result<Option<u16>, StoreError<Error>> {
        let mut bytes = [0; 2];
        match store.load(&mut bytes)? {
            None => Ok(None),
            Some((VERSION, 2)) => {
                let stride = u16::from_le_bytes(bytes);
                if (1..=64).contains(&stride) {
                    Ok(Some(stride))
                } else {
                    Err(StoreError::InvalidRecord)
                }
            }
            Some(_) => Err(StoreError::InvalidRecord),
        }
    }

    pub(super) fn restore() {
        if let Ok(mut slots) = claim()
            && let Ok(mut store) = slots.open_config(2, VERSION..=VERSION)
            && let Ok(Some(value)) = read(&mut store)
        {
            ACTIVE.store(u32::from(value), Ordering::Relaxed);
        }
    }

    pub(super) fn save() {
        let requested = ACTIVE.load(Ordering::Relaxed);
        if requested & DIRTY == 0 {
            return;
        }
        let bytes = requested.to_le_bytes();
        let value = u16::from_le_bytes([bytes[0], bytes[1]]);
        let Ok(mut slots) = claim() else { return };
        let Ok(mut store) = slots.open_config(2, VERSION..=VERSION) else {
            return;
        };
        let Ok(selected) = read(&mut store) else {
            return;
        };
        if selected != Some(value)
            && (store.commit(VERSION, &value.to_le_bytes()).is_err()
                || !matches!(read(&mut store), Ok(Some(saved)) if saved == value))
        {
            return;
        }
        // Keep a newer request dirty should it arrive during storage access.
        let _ = ACTIVE.compare_exchange(
            requested,
            u32::from(value),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }
}
