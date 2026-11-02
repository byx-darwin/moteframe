// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Explicit bridges from embedded-hal 1.0; no GPIO configuration or hidden retries.
//!
//! Wrapped operations, including `flush`, must return within the platform's
//! finite wait budget. These bridges cannot interrupt a blocked HAL call.
//! Own a HAL object or borrow it through `&mut T`; `into_inner` returns it.

use embedded_hal::{delay::DelayNs, digital, spi::SpiBus};

use crate::{DelayMs, OutputPin, SpiWrite};

/// Bridges an exclusively owned HAL SPI bus to synchronous byte writes.
///
/// This is a bus, not a `SpiDevice`: the caller controls chip select separately.
/// Each write attempts one flush, even after a write error, retaining the first
/// error. A successful return means the transfer has completed. Failure can
/// leave the physical bus state uncertain and never triggers a retry.
#[derive(Debug)]
pub struct HalSpiBus<S> {
    inner: S,
}
impl<S> HalSpiBus<S> {
    /// Wraps an already configured HAL bus without performing I/O.
    #[must_use]
    pub const fn new(inner: S) -> Self {
        Self { inner }
    }

    /// Returns the original bus without flushing, resetting or repairing it.
    #[must_use]
    pub fn into_inner(self) -> S {
        self.inner
    }
}
impl<S: SpiBus<u8>> SpiWrite for HalSpiBus<S> {
    type Error = S::Error;

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let written = self.inner.write(bytes);
        let flushed = self.inner.flush();
        written.and(flushed)
    }
}

/// Bridges HAL output levels without changing pin configuration or polarity.
#[derive(Debug)]
pub struct HalOutputPin<P> {
    inner: P,
}
impl<P> HalOutputPin<P> {
    /// Wraps an already configured output pin without changing its level.
    #[must_use]
    pub const fn new(inner: P) -> Self {
        Self { inner }
    }

    /// Returns the original pin without changing its level.
    #[must_use]
    pub fn into_inner(self) -> P {
        self.inner
    }
}
impl<P: digital::OutputPin> OutputPin for HalOutputPin<P> {
    type Error = P::Error;

    fn set_level(&mut self, high: bool) -> Result<(), Self::Error> {
        if high {
            self.inner.set_high()
        } else {
            self.inner.set_low()
        }
    }
}

/// Bridges millisecond delays to the HAL's overflow-safe delay method.
#[derive(Debug)]
pub struct HalDelay<D> {
    inner: D,
}
impl<D> HalDelay<D> {
    /// Wraps a HAL delay provider without waiting.
    #[must_use]
    pub const fn new(inner: D) -> Self {
        Self { inner }
    }

    /// Returns the original delay provider without waiting.
    #[must_use]
    pub fn into_inner(self) -> D {
        self.inner
    }
}
impl<D: DelayNs> DelayMs for HalDelay<D> {
    fn delay_ms(&mut self, milliseconds: u16) {
        DelayNs::delay_ms(&mut self.inner, u32::from(milliseconds));
    }
}
