//! Raw ABI for the pinned ARMv6-M SDK. No safe initialized runtime is implied.
//!
//! # Safety
//! Every call requires exclusive stack ownership, correct ROM/clock/IRQ/GATT
//! startup and the matching normalized library. Call only from contexts allowed
//! by that SDK. Pointers must be valid, properly sized and remain alive for all
//! vendor use; callbacks must never unwind, retain borrowed event pointers or
//! race foreground access. Board code must copy/queue events under appropriate
//! synchronization. No retention assumptions are encoded in a safe API.
#![allow(
    unsafe_code,
    reason = "Only crate boundary allowed to describe and call the vendor ABI"
)]

/// Library success status (failure is zero).
pub const SUCCESS: u8 = 1;

/// Packed notification attribute descriptor; fields must be copied, not referenced.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct AttReport {
    /// Primary service UUID.
    pub primary: u16,
    /// Characteristic UUID.
    pub uuid: u16,
    /// Profile attribute handle.
    pub handle: u16,
    /// Profile CCCD configuration.
    pub config: u16,
    /// Attribute value descriptor.
    pub value: u16,
}

/// Persistent callback registration, using the SDK's exact packed ABI.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EventCallback {
    /// SDK suppression bitmask: zero reports all events, one suppresses that bit.
    pub mask: u32,
    /// Callback receives vendor-owned event bytes; only valid during callback.
    pub callback: unsafe extern "C" fn(*mut u8),
}

/// Packed SDK advertising parameters; consult the board's verified boot profile.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct AdvParams {
    /// SDK advertising packet kind.
    pub kind: u8,
    /// Address type followed by six peer address bytes.
    pub peer: [u8; 7],
    /// SDK filter policy.
    pub policy: u8,
    /// SDK channel mask.
    pub channel: u8,
    /// Interval in 0.625 ms units.
    pub interval: u16,
    /// SDK timeout units.
    pub timeout: u16,
}

// Const assertions execute for the real ARM target, including pointer width.
const _: () = {
    assert!(core::mem::size_of::<AttReport>() == 10);
    assert!(core::mem::align_of::<AttReport>() == 1);
    assert!(core::mem::offset_of!(AttReport, handle) == 4);
    assert!(core::mem::size_of::<EventCallback>() == 8);
    assert!(core::mem::offset_of!(EventCallback, callback) == 4);
    assert!(core::mem::size_of::<AdvParams>() == 14);
    assert!(core::mem::offset_of!(AdvParams, interval) == 10);
};

unsafe extern "C" {
    /// Initializes the vendor stack. See module safety contract; returns SDK status.
    pub fn BleInit() -> u8;
    /// Registers a persistent callback descriptor. See module safety contract.
    pub fn SetEvtCallback(callback: *mut EventCallback) -> u8;
    /// Installs a permanent SDK ATT callback. See module safety contract.
    pub fn ATTCSetCallback(callback: unsafe extern "C" fn(*mut u8)) -> u8;
    /// Replies to an ATT MTU request; returns selected MTU, not send status.
    /// The fixed SDK clamps above185; caller must supply at least23 and handle
    /// the synchronous callback/IRQ context without retaining shared Rust borrows.
    pub fn ATTCMTURsp(mtu: u16) -> u16;
    /// Configures advertising. See module safety contract and pointer lifetime.
    pub fn SetAdvParams(params: *mut AdvParams) -> u8;
    /// Sets legacy advertising/scan bytes. See module safety contract.
    pub fn SetAdvData(data: *mut u8, length: u8, scan: *mut u8, scan_length: u8) -> u8;
    /// Starts advertising; see module safety contract.
    pub fn StartAdv() -> u8;
    /// Sends SDK notification kind 1 or SDK indication kind 2; see safety contract.
    pub fn GATTDataSend(kind: u8, report: *mut AttReport, length: u16, data: *mut u8) -> u8;
    /// Supplies bytes for a GATT read response; see safety contract.
    pub fn SetGATTReadRsp(length: u8, data: *mut u8) -> u8;
    /// Runs the vendor scheduler. See module safety contract.
    pub fn ble_sched_execute();
}

/// ARMCC runtime helper for potentially unaligned reads.
/// # Safety
/// `address` must be readable for four bytes, without concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __aeabi_uread4(address: *const u32) -> u32 {
    // SAFETY: The caller provides four readable bytes; unaligned access is intended.
    unsafe { core::ptr::read_unaligned(address) }
}

/// ARMCC runtime helper; returns the value written per ARM RTABI32.
/// # Safety
/// `address` must be exclusively writable for four bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __aeabi_uwrite4(value: u32, address: *mut u32) -> u32 {
    // SAFETY: The caller provides four exclusively writable bytes.
    unsafe { core::ptr::write_unaligned(address, value) };
    value
}

pub mod backend;

unsafe extern "C" {
    /// Selects MCU clock; board startup uses selector0 (64M RC). See SDK safety contract.
    pub fn MCUClockSwitch(selector: u8) -> u8;
    /// Performs the SDK RC calibration. See SDK safety contract.
    pub fn RCOSCCalibration() -> u8;
    /// Selects 32K source; current project uses selector0 (internal RC).
    pub fn ClockSwitch(selector: u8) -> u8;
    /// Performs the SDK low-power oscillator calibration.
    pub fn LPOCalibration() -> u8;
}
