// SPDX-License-Identifier: Apache-2.0
//! Product-side scheduling for a single owner's configuration snapshot.

use core::num::NonZeroU32;

/// Time reserved by the acquisition and radio scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveWindow {
    /// Sampling is active. No media access is permitted.
    Acquiring,
    /// Sampling is stopped. The caller holds this interval against restart
    /// and competing radio deadlines for the duration of the synchronous call.
    Reserved {
        /// Guaranteed interval remaining, in microseconds.
        available_us: u32,
    },
}

/// Persistence status of the currently active RAM value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveState {
    /// Applied in RAM, with no confirmed matching persistent snapshot yet.
    Pending,
    /// A selected snapshot matched the version, length and all active bytes.
    Saved,
}

/// Bounded operations on one exclusively owned snapshot region.
/// Implementations enforce layout, write permissions and power-loss safety.
pub trait ConfigMedia {
    /// Backend error, including busy or an unresolved prior operation.
    type Error;

    /// Rescan the medium and select its valid committed snapshot, if any.
    /// After any ambiguous write, reconcile the original instance first;
    /// return an error while the controller or medium is still unresolved.
    ///
    /// # Errors
    /// Returns the backend error if a trustworthy selection is unavailable.
    fn selected(&mut self) -> Result<Option<(u32, usize)>, Self::Error>;

    /// Read exactly the requested bytes of the selected persistent snapshot.
    /// The selection stays stable until the next commit or rescan.
    ///
    /// # Errors
    /// Returns the backend error when physical readback fails.
    fn read_selected(&mut self, offset: usize, out: &mut [u8]) -> Result<(), Self::Error>;

    /// Commit through the backend's validated write path.
    ///
    /// # Errors
    /// A failure can have an unknown commit outcome.
    fn commit(&mut self, version: u32, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// Active product-owned payload and its single backing store. No heap or
/// second payload buffer is required. The payload must remain stable until
/// replaced through `apply`.
pub struct DeferredConfig<T, M> {
    version: u32,
    value: T,
    state: SaveState,
    required_us: NonZeroU32,
    media: M,
}

impl<T, M> core::fmt::Debug for DeferredConfig<T, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeferredConfig")
            .field("version", &self.version)
            .field("state", &self.state)
            .field("required_us", &self.required_us)
            .finish_non_exhaustive()
    }
}

impl<T: AsRef<[u8]>, M: ConfigMedia> DeferredConfig<T, M> {
    /// Apply the initial value in RAM. The reserved budget must cover the
    /// entire worst-case flush, including both scans and all readback.
    pub const fn new(version: u32, value: T, required_us: NonZeroU32, media: M) -> Self {
        Self {
            version,
            value,
            required_us,
            state: SaveState::Pending,
            media,
        }
    }

    /// Replace the active value immediately. No media operations are performed.
    pub fn apply(&mut self, version: u32, value: T) {
        self.version = version;
        self.value = value;
        self.state = SaveState::Pending;
    }

    /// Bytes currently used by the application, including pending changes.
    pub fn active(&self) -> &[u8] {
        self.value.as_ref()
    }

    /// Whether the current RAM revision has been confirmed on the medium.
    pub const fn state(&self) -> SaveState {
        self.state
    }

    /// Attempt one save in a reserved window. A readback mismatch leaves the
    /// value pending. The caller decides when to offer another window.
    ///
    /// # Errors
    /// Propagates media failures and keeps the active value pending.
    pub fn flush(&mut self, window: SaveWindow) -> Result<SaveState, M::Error> {
        if self.state == SaveState::Saved
            || !matches!(window, SaveWindow::Reserved { available_us } if available_us >= self.required_us.get())
        {
            return Ok(self.state);
        }
        if !self.matches_selected()? {
            self.media.commit(self.version, self.value.as_ref())?;
            if !self.matches_selected()? {
                return Ok(SaveState::Pending);
            }
        }
        self.state = SaveState::Saved;
        Ok(self.state)
    }

    fn matches_selected(&mut self) -> Result<bool, M::Error> {
        let bytes = self.value.as_ref();
        if self.media.selected()? != Some((self.version, bytes.len())) {
            return Ok(false);
        }
        let mut scratch = [0; 32];
        let mut offset = 0;
        for chunk in bytes.chunks(scratch.len()) {
            let out = &mut scratch[..chunk.len()];
            self.media.read_selected(offset, out)?;
            if out != chunk {
                return Ok(false);
            }
            offset += chunk.len();
        }
        Ok(true)
    }
}
