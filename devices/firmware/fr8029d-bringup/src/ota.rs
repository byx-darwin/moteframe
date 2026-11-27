//! Layout-v1 OTA receiver. The GATT client never supplies a physical write address.
use crate::flash_layout::{APP, COPY_UNIT, STAGING, UpgradePlan};
use sha2::{Digest, Sha256};

/// SDK secondary boot-information sector.
pub const INFO: u32 = 0x1000;
/// Last word of the SDK OTA header, separate from its protected prefix.
pub const MARKER: u32 = INFO + 48;
/// SDK install magic (written last).
pub const MAGIC: u32 = 0x5152_5251;
const PAGE: u32 = 256;
const SECTOR: u32 = 4096;

/// Receiver rejection. The connection must start a fresh transfer after failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Malformed frame, incompatible image, wrong sequence or incomplete download.
    Invalid,
    /// Storage or readback failed.
    Media,
    /// There is already a published install.
    Pending,
}

/// Exclusive board adapter. Returning success means the operation has completed.
pub trait Medium {
    /// Read bytes from an allowed mapped range.
    ///
    /// # Errors
    /// Rejects invalid spans and reports controller or media failures.
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error>;
    /// Erase one aligned sector.
    ///
    /// # Errors
    /// Rejects invalid spans and reports controller or media failures.
    fn erase(&mut self, offset: u32) -> Result<(), Error>;
    /// Program within one page without erasing it.
    ///
    /// # Errors
    /// Rejects invalid spans and reports controller or media failures.
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error>;
}

fn word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[path = "ota_policy.rs"]
mod policy;
use policy::crc_update;
pub use policy::manifest;

#[derive(Debug)]
struct Transfer {
    size: u32,
    crc: u32,
    received: u32,
    erased_end: u32,
    page: [u8; PAGE as usize],
}

/// Connection-local transfer state and a product-authorized image digest.
pub struct Receiver<M> {
    medium: M,
    trusted: [u8; 40],
    transfer: Option<Transfer>,
}

impl<M> core::fmt::Debug for Receiver<M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Receiver")
            .field("active", &self.transfer.is_some())
            .finish_non_exhaustive()
    }
}

impl<M: Medium> Receiver<M> {
    /// Create the receiver for an explicitly approved raw image.
    pub const fn new(medium: M, trusted: [u8; 40]) -> Self {
        Self {
            medium,
            trusted,
            transfer: None,
        }
    }

    /// Discard uncommitted state on disconnect.
    pub fn abort(&mut self) {
        self.transfer = None;
    }

    /// Handle one ATT Write Request. All multibyte fields are little endian.
    /// BEGIN(1,length,crc32), DATA(2,offset,bytes), FINISH(3).
    ///
    /// # Errors
    /// Any failure abandons the current download; only FINISH can publish it.
    pub fn receive(&mut self, frame: &[u8]) -> Result<(), Error> {
        let result = self.dispatch(frame);
        if result.is_err() {
            self.abort();
        }
        result
    }

    fn dispatch(&mut self, frame: &[u8]) -> Result<(), Error> {
        match frame.split_first() {
            Some((&1, data)) if data.len() == 8 && self.transfer.is_none() => {
                let size = word(data);
                if data != &self.trusted[..8]
                    || size < 36
                    || UpgradePlan::check(size, size as usize, STAGING.start, APP.start, COPY_UNIT)
                        .is_none()
                {
                    return Err(Error::Invalid);
                }
                let mut marker = [0; 4];
                self.medium.read(MARKER, &mut marker)?;
                if word(&marker) == MAGIC {
                    return Err(Error::Pending);
                }
                self.transfer = Some(Transfer {
                    size,
                    crc: word(&data[4..]),
                    received: 0,
                    erased_end: STAGING.start,
                    page: [0xff; PAGE as usize],
                });
                Ok(())
            }
            Some((&2, data)) if data.len() > 4 => {
                let t = self.transfer.as_mut().ok_or(Error::Invalid)?;
                let payload = &data[4..];
                if word(data) != t.received || payload.len() > (t.size - t.received) as usize {
                    return Err(Error::Invalid);
                }
                for &byte in payload {
                    t.page[(t.received % PAGE) as usize] = byte;
                    t.received += 1;
                    if t.received.is_multiple_of(PAGE) || t.received == t.size {
                        let address = STAGING.start + (t.received - 1) / PAGE * PAGE;
                        if address == t.erased_end {
                            self.medium.erase(address)?;
                            t.erased_end += SECTOR;
                        }
                        self.medium.program(address, &t.page)?;
                        t.page.fill(0xff);
                    }
                }
                Ok(())
            }
            Some((&3, [])) => self.finish(),
            _ => Err(Error::Invalid),
        }
    }

    fn finish(&mut self) -> Result<(), Error> {
        let mut t = self.transfer.take().ok_or(Error::Invalid)?;
        let plan = UpgradePlan::check(
            t.size,
            t.received as usize,
            STAGING.start,
            APP.start,
            COPY_UNIT,
        )
        .ok_or(Error::Invalid)?;
        while t.erased_end < plan.staging().end {
            self.medium.erase(t.erased_end)?;
            t.erased_end += SECTOR;
        }
        let mut hash = Sha256::new();
        let mut crc = u32::MAX;
        let mut version = 0;
        for address in plan.staging().step_by(PAGE as usize) {
            let offset = (address - STAGING.start) as usize;
            let len = (t.size as usize).saturating_sub(offset).min(PAGE as usize);
            self.medium.read(address, &mut t.page)?;
            if offset == 0 {
                let sp = word(&t.page);
                let pc = word(&t.page[4..]);
                if !sp.is_multiple_of(8)
                    || !(0x2000_0008..=0x2001_8000).contains(&sp)
                    || pc & 1 == 0
                    || !(0x1000_2000..0x1000_2000 + t.size).contains(&(pc & !1))
                {
                    return Err(Error::Invalid);
                }
                version = word(&t.page[32..]);
            }
            hash.update(&t.page[..len]);
            crc = crc_update(crc, &t.page[..len]);
            if t.page[len..].iter().any(|&b| b != 0xff) {
                return Err(Error::Media);
            }
        }
        if !crc != t.crc || hash.finalize()[..] != self.trusted[8..] {
            return Err(Error::Invalid);
        }
        // SDK v0.3.6 examples/common/ota/ota.h: 12 words preceding ota_magic.
        let mut prefix = [0u8; 48];
        for (out, value) in prefix.chunks_exact_mut(4).zip([
            version,
            STAGING.start,
            t.size,
            APP.start,
            COPY_UNIT,
            8,
            t.crc,
            MAGIC,
            0,
            0x00d0_2e83,
            0,
        ]) {
            out.copy_from_slice(&value.to_le_bytes());
        }
        let info_crc = !crc_update(u32::MAX, &prefix[..44]);
        prefix[44..].copy_from_slice(&info_crc.to_le_bytes());
        self.medium.erase(INFO)?;
        self.medium.program(INFO, &prefix)?;
        let mut actual = [0; 52];
        self.medium.read(INFO, &mut actual)?;
        if actual[..48] != prefix || word(&actual[48..]) != u32::MAX {
            return Err(Error::Media);
        }
        UpgradePlan::check(
            word(&actual[8..]),
            t.received as usize,
            word(&actual[4..]),
            word(&actual[12..]),
            word(&actual[16..]),
        )
        .ok_or(Error::Invalid)?;
        self.medium.program(MARKER, &MAGIC.to_le_bytes())?;
        self.medium.read(MARKER, &mut actual[..4])?;
        if word(&actual) != MAGIC {
            return Err(Error::Media);
        }
        Ok(())
    }
}
