//! Singleton ARM backend. No Rust inbox borrow crosses any call into the vendor.
#![allow(
    unsafe_code,
    reason = "All retained pointers, callback ISR synchronization and ARM context checks are isolated here"
)]

use super::{AdvParams, AttReport, EventCallback, SUCCESS};
use crate::runtime::{
    AdvertisementLock, AdvertisingChannels, Backend, Error, Inbox, LinkState, Profile, TX_CAPACITY,
};
use core::{
    arch::asm,
    cell::UnsafeCell,
    marker::PhantomData,
    ptr::{addr_of_mut, read_unaligned, write},
};
use device_ble::{Advertisement, Event};

struct Shared<T>(UnsafeCell<T>);
// SAFETY: Private statics only; mailbox accesses use PRIMASK. Advertising storage
// is written once before handoff and never borrowed/accessed by Rust afterwards.
unsafe impl<T> Sync for Shared<T> {}

struct Mailbox {
    claimed: bool,
    started: bool,
    ready: bool,
    profile: Option<Profile>,
    inbox: Option<Inbox<4>>,
    advertisement: AdvertisementLock,
}
static MAILBOX: Shared<Mailbox> = Shared(UnsafeCell::new(Mailbox {
    claimed: false,
    started: false,
    ready: false,
    profile: None,
    inbox: None,
    advertisement: AdvertisementLock::new(),
}));
struct AdvertisingStorage {
    parameters: AdvParams,
    data: [u8; 31],
    scan: [u8; 31],
}
static ADVERTISING: Shared<AdvertisingStorage> = Shared(UnsafeCell::new(AdvertisingStorage {
    parameters: AdvParams {
        kind: 0,
        peer: [0; 7],
        policy: 0,
        channel: 7,
        interval: 160,
        timeout: 0,
    },
    data: [0; 31],
    scan: [0; 31],
}));

fn masks() -> (u32, u32) {
    let mask: u32;
    let exception: u32;
    // SAFETY: ARMv6-M special registers; no memory is accessed by these instructions.
    unsafe {
        asm!("mrs {mask}, PRIMASK", "mrs {exception}, IPSR",mask=out(reg) mask,exception=out(reg) exception,options(nostack,preserves_flags));
    }
    (mask, exception)
}
fn disable() {
    // SAFETY: Masks configurable IRQs on ARMv6-M; asm acts as a compiler barrier.
    unsafe {
        asm!("cpsid i", options(nostack));
    }
}
fn enable() {
    // SAFETY: Used only to restore an originally enabled context, or by board startup.
    unsafe {
        asm!("cpsie i", options(nostack));
    }
}
fn mailbox<T>(operation: impl FnOnce(&mut Mailbox) -> T) -> T {
    let (previous, _) = masks();
    disable();
    // SAFETY: Single core, IRQs masked, non-reentrant private closure. Never call
    // vendor code or expose the reference/pointers from this critical section.
    let result = operation(unsafe { &mut *MAILBOX.0.get() });
    if previous & 1 == 0 {
        enable();
    }
    result
}
fn foreground() -> Result<(), Error> {
    let (mask, exception) = masks();
    if mask & 1 != 0 || exception != 0 {
        Err(Error::WrongContext)
    } else {
        Ok(())
    }
}
fn state() -> Result<LinkState, Error> {
    mailbox(|storage| {
        if !storage.ready {
            return Err(Error::NotInitialized);
        }
        storage.inbox.as_ref().ok_or(Error::NotInitialized)?.state()
    })
}
fn fault(error: Error) {
    mailbox(|storage| {
        if let Some(inbox) = storage.inbox.as_mut() {
            inbox.fail(error);
        }
    });
}
fn status(value: u8) -> Result<(), Error> {
    if value == SUCCESS {
        Ok(())
    } else {
        Err(Error::VendorRejected)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn syd881x_event_callback(event: *mut u8) {
    // Mask before reading or borrowing vendor memory, not just before queuing.
    // This also protects a reused SDK event buffer from nested maskable IRQs.
    mailbox(|storage| {
        let Some(inbox) = storage.inbox.as_mut() else {
            return;
        };
        if event.is_null() {
            inbox.fail(Error::InvalidEvent);
            return;
        }
        // SAFETY: The pinned SDK guarantees a five-byte GAP prefix during this
        // callback. IRQs stay masked through all reads, decode and owned copies.
        let domain = unsafe { event.read() };
        if domain != 0 {
            return;
        }
        // SAFETY: Code is the unaligned u32 at offset1 of that valid prefix.
        let code = unsafe { read_unaligned(event.add(1).cast::<u32>()) };
        let length = match code {
            0x200 => {
                // SAFETY: The verified active ATT-write variant has a 13-byte prefix.
                let size = usize::from(unsafe { read_unaligned(event.add(11).cast::<u16>()) });
                if size > crate::WRITE_CAPACITY {
                    inbox.fail(Error::InvalidEvent);
                    return;
                }
                13 + size
            }
            4 => 6,
            0x2000 => 11,
            _ => 5,
        };
        // SAFETY: Active variant sizes match pinned DWARF. The borrow is confined
        // to this critical closure and cannot escape or survive the callback.
        let bytes = unsafe { core::slice::from_raw_parts(event, length) };
        let _ = inbox.ingest(bytes); // ingest itself latches all failures and revokes TX.
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn syd881x_attc_callback(event: *mut u8) {
    let (previous, _) = masks();
    disable();
    let result = if event.is_null() {
        Err(Error::InvalidEvent)
    } else {
        // SAFETY: Pinned SDK callback always supplies the two-byte event prefix.
        // Only code2/size2 permits reading its additional two-byte MTU payload;
        // the copied scalar bytes never retain a vendor-memory borrow.
        let prefix = unsafe { [event.read(), event.add(1).read()] };
        if prefix == [2, 2] {
            // SAFETY: Verified active MTU variant contains these two bytes.
            let bytes = unsafe { [2, 2, event.add(2).read(), event.add(3).read()] };
            crate::runtime::mtu_response_for_att_event(&bytes)
        } else {
            crate::runtime::mtu_response_for_att_event(&prefix)
        }
    };
    match result {
        Ok(Some(mtu)) => {
            // SAFETY: Validated request and conservative scalar23; the pinned
            // responder's synchronous callback path was verified on hardware. No
            // mailbox/shared reference or vendor packet borrow crosses the call.
            let selected = unsafe { super::ATTCMTURsp(mtu) };
            if selected != mtu {
                fault(Error::VendorRejected);
            }
        }
        Ok(None) => {}
        Err(error) => fault(error),
    }
    if previous & 1 == 0 {
        enable();
    } else {
        disable();
    }
}

/// Singleton backend; neither Send nor Sync. Dropping does not free static state.
#[derive(Debug)]
pub struct FfiBackend {
    channels: AdvertisingChannels,
    _foreground: PhantomData<*mut ()>,
}
impl FfiBackend {
    /// Claims one BLE stack after verified ROM startup and normal RAM initialization.
    /// # Safety
    /// Must execute on the pinned SYD8810/SYD881x chip/ROM with the matching boot
    /// configuration/GATT profile, reserved RAM intact and no previously running
    /// stack/callbacks. No other code may initialize, reconfigure or call the BLE
    /// stack. The ROM's callback and synchronous-copy TX contracts are required.
    /// Only one foreground owner may use this backend; no NMI may access mailbox.
    /// # Errors
    /// Rejects a second claim or an exception context; ownership is never recycled.
    pub unsafe fn claim() -> Result<Self, Error> {
        // SAFETY: Same caller preconditions, default SDK channel set.
        unsafe { Self::claim_with_advertising_channels(AdvertisingChannels::ALL) }
    }
    /// Claims the singleton with a validated chip-specific advertising channel set.
    /// # Safety
    /// All exclusive ROM/boot/profile/IRQ preconditions of [`Self::claim`] apply.
    /// # Errors
    /// Rejects a second claim or exception context exactly as [`Self::claim`].
    pub unsafe fn claim_with_advertising_channels(
        channels: AdvertisingChannels,
    ) -> Result<Self, Error> {
        if masks().1 != 0 {
            return Err(Error::WrongContext);
        }
        mailbox(|storage| {
            if storage.claimed {
                return Err(Error::AlreadyTaken);
            }
            storage.claimed = true;
            Ok(Self {
                channels,
                _foreground: PhantomData,
            })
        })
    }
}
impl Backend for FfiBackend {
    fn initialize(&mut self, profile: Profile) -> Result<(), Error> {
        if masks().1 != 0 {
            return Err(Error::WrongContext);
        }
        mailbox(|storage| {
            if storage.started {
                return Err(Error::AlreadyTaken);
            }
            storage.started = true;
            storage.profile = Some(profile);
            storage.inbox = Some(Inbox::new(profile));
            Ok(())
        })?;
        let (previous, _) = masks();
        disable();
        let mut registration = EventCallback {
            mask: 0,
            callback: syd881x_event_callback,
        };
        // SAFETY: Exclusive boot lease; caller met ROM/shared-RAM preconditions.
        // No mailbox borrow exists. SetEvtCallback synchronously copies these
        // eight descriptor bytes; the callback function/static storage never dies.
        let result = unsafe {
            status(super::BleInit())
                .and_then(|()| status(super::SetEvtCallback(&raw mut registration)))
                .and_then(|()| status(super::ATTCSetCallback(syd881x_attc_callback)))
        };
        if previous & 1 == 0 {
            enable();
        } else {
            disable();
        }
        match result {
            Ok(()) => {
                mailbox(|storage| storage.ready = true);
                state().map(|_| ())
            }
            Err(error) => {
                fault(error);
                Err(error)
            }
        }
    }
    fn advertise(&mut self, advertisement: &Advertisement<'_>) -> Result<(), Error> {
        foreground()?;
        state()?;
        let first = mailbox(|storage| storage.advertisement.bind(advertisement))?;
        if first {
            let mut owned = AdvertisingStorage {
                parameters: AdvParams {
                    kind: 0,
                    peer: [0; 7],
                    policy: 0,
                    channel: self.channels.mask(),
                    interval: advertisement.interval(),
                    timeout: 0,
                },
                data: [0; 31],
                scan: [0; 31],
            };
            owned.data[..advertisement.data().len()].copy_from_slice(advertisement.data());
            owned.scan[..advertisement.scan_response().len()]
                .copy_from_slice(advertisement.scan_response());
            let data_length =
                u8::try_from(advertisement.data().len()).map_err(|_| Error::PayloadTooLarge)?;
            let scan_length = u8::try_from(advertisement.scan_response().len())
                .map_err(|_| Error::PayloadTooLarge)?;
            let storage = ADVERTISING.0.get();
            // SAFETY: First handoff only. This static is distinct from MAILBOX,
            // never moved/overwritten/borrowed by Rust again, including on failure.
            // Stable addresses remain valid if ROM retains or modifies these bytes.
            let result = unsafe {
                write(storage, owned);
                status(super::SetAdvParams(addr_of_mut!((*storage).parameters))).and_then(|()| {
                    status(super::SetAdvData(
                        addr_of_mut!((*storage).data).cast::<u8>(),
                        data_length,
                        addr_of_mut!((*storage).scan).cast::<u8>(),
                        scan_length,
                    ))
                })
            };
            if let Err(error) = result {
                fault(error);
                return Err(error);
            }
        }
        // SAFETY: Matching stable configuration already handed to ROM; no borrows held.
        let result = unsafe { status(super::StartAdv()) };
        if let Err(error) = result {
            fault(error);
        }
        result
    }
    fn state(&self) -> Result<LinkState, Error> {
        state()
    }
    fn send(&mut self, profile: Profile, epoch: u32, data: &[u8]) -> Result<(), Error> {
        foreground()?;
        if data.len() > TX_CAPACITY {
            return Err(Error::PayloadTooLarge);
        }
        let length = u16::try_from(data.len()).map_err(|_| Error::PayloadTooLarge)?;
        // The SDK unconditionally enables IRQs on its transmit path. Only enter
        // from an originally unmasked foreground context, and hold no Rust shared
        // state borrow across it. Mask just for final authorization-to-call atomicity.
        disable();
        let admission = mailbox(|storage| {
            if !storage.ready {
                return Err(Error::NotInitialized);
            }
            if storage.profile != Some(profile) {
                return Err(Error::InvalidProfile);
            }
            let link = storage
                .inbox
                .as_ref()
                .ok_or(Error::NotInitialized)?
                .state()?;
            if link.epoch() != epoch {
                return Err(Error::StaleConnection);
            }
            if !link.can_notify() {
                return Err(Error::NotSubscribed);
            }
            Ok(())
        });
        let result = admission.and_then(|()| {
            let mut report = AttReport {
                primary: profile.service,
                uuid: profile.tx_uuid(),
                handle: profile.tx_handle(),
                config: profile.cccd,
                value: 1,
            };
            // SAFETY: No mailbox reference survives, input length <=20 and report
            // fully initialized. Fixed SDK synchronously memcpy's input to its
            // own TX buffers and never writes/retains the caller data pointer.
            unsafe {
                status(super::GATTDataSend(
                    1,
                    &raw mut report,
                    length,
                    data.as_ptr().cast_mut(),
                ))
            }
        });
        // Also covers SDK early rejection paths that did not enable IRQs themselves.
        enable();
        result
    }
    fn service(&mut self) -> Result<(), Error> {
        foreground()?;
        state()?;
        // SAFETY: Serialized initialized foreground; no mailbox borrow crosses ROM.
        unsafe {
            super::ble_sched_execute();
        }
        // SDK timeout0 completes a finite burst; original C code restarts it
        // through OSAL. Consume that positive event here in foreground instead.
        // Keep authorization-to-call atomic and no Rust mailbox borrow across FFI.
        disable();
        let restart = mailbox(|storage| {
            let ended = storage
                .inbox
                .as_mut()
                .ok_or(Error::NotInitialized)?
                .take_advertising_end()?;
            Ok(ended && storage.advertisement.is_bound())
        });
        let result = restart.and_then(|ended| {
            if ended {
                // SAFETY: Same immutable advertising storage, positive end event,
                // disconnected callback state, IRQ-masked serialized foreground.
                unsafe { status(super::StartAdv()) }
            } else {
                Ok(())
            }
        });
        enable();
        if let Err(error) = result {
            fault(error);
            return Err(error);
        }
        state().map(|_| ())
    }
    fn next_event(&mut self) -> Option<Event> {
        mailbox(|storage| storage.inbox.as_mut().and_then(Inbox::pop))
    }
}
