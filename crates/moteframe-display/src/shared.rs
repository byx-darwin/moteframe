// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Shared bus adapter: DC changes are inside the same exclusive CS transaction.
use embedded_hal::{delay::DelayNs, spi::SpiBus};
use moteframe_core::{
    DisplayBus, OutputPin,
    spi::{Device, Error},
};
/// Panel device on a foreground shared SPI owner, with one independent DC pin.
#[derive(Debug)]
pub struct SharedPanelBus<'a, B, D, C, P> {
    device: Device<'a, B, D, C>,
    dc: P,
}
impl<'a, B, D, C, P> SharedPanelBus<'a, B, D, C, P> {
    /// Wraps an idle-high device. Both devices sharing the bus must use its configured mode.
    #[must_use]
    pub const fn new(device: Device<'a, B, D, C>, dc: P) -> Self {
        Self { device, dc }
    }
}
impl<B: SpiBus, D: DelayNs, C: OutputPin, P: OutputPin<Error = C::Error>> DisplayBus
    for SharedPanelBus<'_, B, D, C, P>
{
    type Error = Error<B::Error, C::Error>;
    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error> {
        let dc = &mut self.dc;
        self.device.transaction_with(|bus, _| {
            dc.set_level(false).map_err(Error::Pin)?;
            bus.write(&[command]).map_err(Error::Spi)?;
            if !data.is_empty() {
                // HAL bus writes may buffer; the command must finish before DC changes.
                bus.flush().map_err(Error::Spi)?;
                dc.set_level(true).map_err(Error::Pin)?;
                bus.write(data).map_err(Error::Spi)?;
            }
            Ok(())
        })
    }
    fn pixels(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let dc = &mut self.dc;
        self.device.transaction_with(|bus, _| {
            dc.set_level(true).map_err(Error::Pin)?;
            bus.write(data).map_err(Error::Spi)
        })
    }
}
