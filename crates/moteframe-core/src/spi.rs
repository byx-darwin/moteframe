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
}
impl<B: SpiBus, D: DelayNs> SharedBus<B, D> {
    /// Establishes an idle-high slave select and returns its uniquely owned device.
    /// # Errors
    /// Rejects a busy/faulted bus and poisons the bus if the initial CS write fails.
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
}
/// Shared synchronous device with exclusive ownership of one slave select.
#[derive(Debug)]
pub struct Device<'a, B, D, C> {
    shared: &'a SharedBus<B, D>,
    select: C,
}
impl<B: SpiBus, D: DelayNs, C: OutputPin> Device<'_, B, D, C> {
    /// Runs command/data operations under one bus borrow and CS assertion.
    /// Flush and CS release are attempted on every selected path. Any error poisons the bus.
    /// Do not yield to a scheduler or recursively use the bus inside the closure.
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
