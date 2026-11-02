// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Foreground-only shared SPI; a select is held through the entire synchronous transaction.
use crate::OutputPin;
use core::{cell::RefCell, fmt::Debug};
use embedded_hal::{
    delay::DelayNs,
    spi::{ErrorType, Operation, SpiBus, SpiDevice},
};
/// Shared bus, pin or ownership failure. The first failure is preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<S, P> {
    /// Controller failed.
    Spi(S),
    /// CS or another transaction control pin failed.
    Pin(P),
    /// Another transaction holds the foreground borrow.
    Busy,
    /// A previous transaction failed; hardware recovery is required.
    Faulted,
}
impl<S: embedded_hal::spi::Error, P: Debug> embedded_hal::spi::Error for Error<S, P> {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        match self {
            Self::Spi(e) => e.kind(),
            _ => embedded_hal::spi::ErrorKind::Other,
        }
    }
}
#[derive(Debug)]
struct State<B, D> {
    bus: B,
    delay: D,
    faulted: bool,
}
/// Owns one configured bus and delay. Devices must use the same mode and frequency.
/// `RefCell` deliberately prevents Sync; no interrupt handler may use these handles.
/// Underlying operations, flushes and delays must have finite platform-defined waits.
#[derive(Debug)]
pub struct SharedBus<B, D> {
    state: RefCell<State<B, D>>,
}
impl<B, D> SharedBus<B, D> {
    /// Takes exclusive bus ownership. All unrelated slave selects must already be high.
    #[must_use]
    pub const fn new(bus: B, delay: D) -> Self {
        Self {
            state: RefCell::new(State {
                bus,
                delay,
                faulted: false,
            }),
        }
    }

    /// Returns the controller and delay without I/O or hardware recovery.
    ///
    /// Reclaim all devices first. Pins or the controller may still be in an
    /// uncertain physical state after failure; repair them explicitly before
    /// building a new owner. This does not clear a fault on a live owner.
    #[must_use]
    pub fn into_parts(self) -> (B, D) {
        let State { bus, delay, .. } = self.state.into_inner();
        (bus, delay)
    }
}
impl<B: SpiBus, D: DelayNs> SharedBus<B, D> {
    /// Establishes an idle-high slave select and returns its uniquely owned device.
    /// # Errors
    /// Rejects a busy/faulted bus and poisons the bus if the initial CS write fails.
    /// This legacy entry consumes the pin on error. Use `try_device` to retain it.
    pub fn device<C: OutputPin>(&self, mut select: C) -> DeviceResult<'_, B, D, C> {
        let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        if s.faulted {
            return Err(Error::Faulted);
        }
        if let Err(e) = select.set_level(true) {
            s.faulted = true;
            return Err(Error::Pin(e));
        }
        Ok(Device {
            shared: self,
            select,
        })
    }

    /// Establishes an idle-high device, returning the original pin on every error.
    ///
    /// Busy and faulted owners perform no I/O. Initial CS failure poisons the
    /// owner; the returned pin is not necessarily physically high. No retry or
    /// hardware recovery is attempted.
    /// # Errors
    /// Returns the first failure and original pin in `DeviceCreateError`.
    pub fn try_device<C: OutputPin>(&self, mut select: C) -> TryDeviceResult<'_, B, D, C> {
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return Err(DeviceCreateError {
                error: Error::Busy,
                select,
            });
        };
        if state.faulted {
            return Err(DeviceCreateError {
                error: Error::Faulted,
                select,
            });
        }
        if let Err(error) = select.set_level(true) {
            state.faulted = true;
            return Err(DeviceCreateError {
                error: Error::Pin(error),
                select,
            });
        }
        Ok(Device {
            shared: self,
            select,
        })
    }
}

/// A failed device creation retaining both the first error and the original pin.
/// Extracting these values performs no I/O and does not repair the fault.
pub struct DeviceCreateError<S, C: OutputPin> {
    error: Error<S, C::Error>,
    select: C,
}
impl<S, C: OutputPin> DeviceCreateError<S, C> {
    /// Borrows the original failure without surrendering the retained pin.
    #[must_use]
    pub const fn error(&self) -> &Error<S, C::Error> {
        &self.error
    }

    /// Returns the first failure and original pin without changing hardware.
    #[must_use]
    pub fn into_parts(self) -> (Error<S, C::Error>, C) {
        (self.error, self.select)
    }
}
impl<S: Debug, C: OutputPin + Debug> Debug for DeviceCreateError<S, C>
where
    C::Error: Debug,
{
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("DeviceCreateError")
            .field("error", &self.error)
            .field("select", &self.select)
            .finish()
    }
}
/// Shared synchronous device with exclusive ownership of one slave select.
#[derive(Debug)]
pub struct Device<'a, B, D, C> {
    shared: &'a SharedBus<B, D>,
    select: C,
}
impl<B, D, C> Device<'_, B, D, C> {
    /// Returns the original select pin without I/O, flush or recovery.
    /// The physical level may be uncertain following an error.
    #[must_use]
    pub fn into_select(self) -> C {
        self.select
    }
}
impl<B: SpiBus, D: DelayNs, C: OutputPin> Device<'_, B, D, C> {
    /// Runs command/data operations under one bus borrow and CS assertion.
    /// Flush and CS release are attempted on every selected path. Any error poisons the bus.
    /// Do not yield to a scheduler or recursively use the bus inside the closure.
    /// Cleanup covers `Result` paths; unwinding panics are not recovered.
    /// # Errors
    /// Reports busy/faulted state or the first operation/flush/pin error.
    pub fn transaction_with<T>(
        &mut self,
        operation: impl FnOnce(&mut B, &mut D) -> Result<T, Error<B::Error, C::Error>>,
    ) -> Result<T, Error<B::Error, C::Error>> {
        let mut s = self
            .shared
            .state
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?;
        if s.faulted {
            return Err(Error::Faulted);
        }
        let selected = self.select.set_level(false).map_err(Error::Pin);
        let result = selected.and_then(|()| {
            let State { bus, delay, .. } = &mut *s;
            operation(bus, delay)
        });
        let flushed = s.bus.flush().map_err(Error::Spi);
        let released = self.select.set_level(true).map_err(Error::Pin);
        if result.is_err() || flushed.is_err() || released.is_err() {
            s.faulted = true;
        }
        result.and_then(|value| flushed.and(released).map(|()| value))
    }
}
impl<B: SpiBus, D: DelayNs, C: OutputPin> ErrorType for Device<'_, B, D, C>
where
    C::Error: Debug,
{
    type Error = Error<B::Error, C::Error>;
}
impl<B: SpiBus, D: DelayNs, C: OutputPin> SpiDevice for Device<'_, B, D, C>
where
    C::Error: Debug,
{
    fn transaction(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.transaction_with(|bus, delay| {
            for op in ops {
                match op {
                    Operation::Read(out) => bus.read(out).map_err(Error::Spi)?,
                    Operation::Write(data) => bus.write(data).map_err(Error::Spi)?,
                    Operation::Transfer(out, data) => {
                        bus.transfer(out, data).map_err(Error::Spi)?;
                    }
                    Operation::TransferInPlace(data) => {
                        bus.transfer_in_place(data).map_err(Error::Spi)?;
                    }
                    Operation::DelayNs(ns) => {
                        bus.flush().map_err(Error::Spi)?;
                        delay.delay_ns(*ns);
                    }
                }
            }
            Ok(())
        })
    }
}

/// Device construction result with controller and select errors preserved.
pub type DeviceResult<'a, B, D, C> =
    Result<Device<'a, B, D, C>, Error<<B as ErrorType>::Error, <C as OutputPin>::Error>>;

/// Device creation result retaining pin ownership on failure.
pub type TryDeviceResult<'a, B, D, C> =
    Result<Device<'a, B, D, C>, DeviceCreateError<<B as ErrorType>::Error, C>>;
