//! Emulator-only debugger entry. The reset vector never invokes this function.
#[cfg(not(model_sim))]
use crate::qspi_slots::QspiSlots;
use core::num::NonZeroU32;
#[cfg(not(model_sim))]
use fr8029d_bringup::qspi_config::Partition;
use moteframe_storage::deferred::{DeferredConfig, SaveState, SaveWindow};

#[cfg(not(model_sim))]
#[unsafe(link_section = ".ramfunc.qspi_timeout")]
#[unsafe(no_mangle)]
extern "C" fn model_stall() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(not(model_sim))]
fn bind() -> Option<QspiSlots> {
    // SAFETY: The model runner maps this mailbox before calling the entry.
    let start = unsafe { (0x6000_4000 as *const u32).read_volatile() };
    let region = Partition::new(start, 0x40000, &[0x40000..0x60000, 0xe0000..0x100000]).ok()?;
    // SAFETY: This fixture runs on an exclusive emulated core with no DMA or
    // interrupt producers. The runner traps model_stall at its SRAM entry.
    unsafe { QspiSlots::claim(region, model_stall) }.ok()
}

#[unsafe(no_mangle)]
pub extern "C" fn trial(request: u32) -> u32 {
    let Some(mut media) = bind() else { return 9 };
    #[cfg(model_sim)]
    let opened = moteframe_storage::snapshot::ConfigStore::open(&mut media, 512, 1..=2);
    #[cfg(not(model_sim))]
    let opened = media.open_config(512, 1..=2);
    let Ok(mut store) = opened else {
        return 1;
    };
    let budget = NonZeroU32::new(100).unwrap();
    #[cfg(not(model_full))]
    {
        // SAFETY: The runner supplies this exclusively borrowed configuration.
        let buf = unsafe { &mut *(0x6000_3000 as *mut [u8; 300]) };
        if request == 0 {
            return match store.load(buf) {
                Ok(Some((version, 300))) => version,
                _ => 0,
            };
        }
        let window = match request >> 8 {
            0 => SaveWindow::Acquiring,
            available_us => SaveWindow::Reserved { available_us },
        };
        let mut config = DeferredConfig::new(request & 255, &buf[..], budget, store);
        match config.flush(window) {
            Ok(SaveState::Saved) => 2,
            Ok(SaveState::Pending) => 1,
            Err(_) => 0,
        }
    }
    #[cfg(model_full)]
    {
        if request == 2 || request == 4 {
            let mut bytes = [0; 300];
            let version = request / 2;
            return u32::from(
                store.load(&mut bytes) != Ok(Some((version, 300))) || bytes.iter().any(|&byte| byte != version as u8),
            );
        }
        let window = SaveWindow::Reserved { available_us: 100 };
        let mut config = DeferredConfig::new(1, [1; 300], budget, store);
        if request == 0 {
            return u32::from(config.flush(window) != Ok(SaveState::Saved));
        }
        config.apply(2, [2; 300]);
        if config.active().iter().any(|&byte| byte != 2)
            || config.flush(SaveWindow::Acquiring) != Ok(SaveState::Pending)
            || config.flush(SaveWindow::Reserved { available_us: 99 }) != Ok(SaveState::Pending)
        {
            return 2;
        }
        if request == 1 {
            return 0;
        }
        u32::from(config.flush(window) != Ok(SaveState::Saved))
    }
}

#[cfg(model_sim)]
#[path = "model_nor.rs"]
mod model_nor;
#[cfg(model_sim)]
fn bind() -> Option<model_nor::Media> {
    Some(model_nor::Media)
}
