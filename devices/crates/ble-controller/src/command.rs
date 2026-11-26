//! Finite standard HCI command dispatch; unsupported capabilities fail explicitly.
use crate::hci::{HciError, HciState};
use crate::{
    Advertisement, AdvertisingData, AdvertisingParameters, ControllerError, LinkBackend,
    RandomAddress,
};
use bt_hci::{cmd, param::Error as Status};

/// Owned HCI return parameters without the status byte.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CommandReply {
    bytes: [u8; 16],
    len: usize,
}
impl core::fmt::Debug for CommandReply {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommandReply")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
impl CommandReply {
    fn new(data: &[u8]) -> Self {
        let mut bytes = [0; 16];
        bytes[..data.len()].copy_from_slice(data);
        Self {
            bytes,
            len: data.len(),
        }
    }
    /// Valid returned bytes, excluding status.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
fn length(data: &[u8], n: usize) -> Result<(), Status> {
    if data.len() == n {
        Ok(())
    } else {
        Err(Status::INVALID_HCI_PARAMETERS)
    }
}
fn word(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}
impl<B: LinkBackend> HciState<B> {
    fn editable(&self) -> Result<(), Status> {
        if self.core.is_advertising() || self.core.connection().is_some() {
            Err(Status::CMD_DISALLOWED)
        } else {
            Ok(())
        }
    }
    fn core_result(
        result: Result<(), ControllerError<B::Error>>,
    ) -> Result<(), cmd::Error<HciError<B::Error>>> {
        result.map_err(|e| match e {
            ControllerError::State => cmd::Error::Hci(Status::CMD_DISALLOWED),
            _ => cmd::Error::Io(HciError::Controller(e)),
        })
    }
    fn config_command(&mut self, opcode: u16, p: &[u8]) -> Result<(), Status> {
        self.editable()?;
        match opcode {
            0x2005 => {
                length(p, 6)?;
                self.address = Some(
                    RandomAddress::new([p[0], p[1], p[2], p[3], p[4], p[5]])
                        .map_err(|_| Status::INVALID_HCI_PARAMETERS)?,
                );
            }
            0x2006 => {
                length(p, 15)?;
                if p[4] != 0 || p[5] != 1 || p[6] > 1 || p[14] != 0 {
                    return Err(Status::UNSUPPORTED);
                }
                self.parameters = Some(
                    AdvertisingParameters::new(word(p, 0), word(p, 2), p[13])
                        .map_err(|_| Status::INVALID_HCI_PARAMETERS)?,
                );
            }
            0x2008 | 0x2009 => {
                length(p, 32)?;
                if p[0] > 31 {
                    return Err(Status::INVALID_HCI_PARAMETERS);
                }
                let data = AdvertisingData::new(&p[1..=usize::from(p[0])])
                    .map_err(|_| Status::INVALID_HCI_PARAMETERS)?;
                if opcode == 0x2008 {
                    self.data = Some(data);
                } else {
                    self.scan = Some(data);
                }
            }
            _ => return Err(Status::UNKNOWN_CMD),
        }
        Ok(())
    }
    #[cfg(feature = "data-length")]
    fn data_length_command(
        &mut self,
        opcode: u16,
        p: &[u8],
    ) -> Result<CommandReply, cmd::Error<HciError<B::Error>>> {
        use crate::DataLength;
        match opcode {
            0x2022 => {
                length(p, 6)?;
                let handle = word(p, 0);
                if self
                    .core
                    .connection()
                    .is_none_or(|id| id.handle() != handle)
                {
                    return Err(Status::UNKNOWN_CONN_IDENTIFIER.into());
                }
                let (octets, time) = (word(p, 2), word(p, 4));
                if !DataLength::valid_pair(octets, time) {
                    return Err(Status::INVALID_HCI_PARAMETERS.into());
                }
                Self::core_result(self.core.set_tx_length(octets, time))?;
                Ok(CommandReply::new(&handle.to_le_bytes()))
            }
            0x2023 => {
                length(p, 0)?;
                let suggested = self.core.suggested_length.unwrap_or(DataLength::LEGACY);
                let [octets_lo, octets_hi] = suggested.tx_octets.to_le_bytes();
                let [time_lo, time_hi] = suggested.tx_time.to_le_bytes();
                Ok(CommandReply::new(&[octets_lo, octets_hi, time_lo, time_hi]))
            }
            0x2024 => {
                length(p, 4)?;
                let (octets, time) = (word(p, 0), word(p, 2));
                if !DataLength::valid_pair(octets, time) {
                    return Err(Status::INVALID_HCI_PARAMETERS.into());
                }
                self.core.suggested_length = Some(DataLength {
                    tx_octets: octets,
                    tx_time: time,
                    ..DataLength::MAX
                });
                Ok(CommandReply::new(&[]))
            }
            _ => {
                length(p, 0)?;
                // The supported TX and RX maxima are symmetric.
                Ok(CommandReply::new(&DataLength::MAX.control(0)[1..]))
            }
        }
    }
    #[allow(
        clippy::inline_always,
        reason = "Specialize fixed HCI opcodes without retaining the universal command frame"
    )]
    #[inline(always)]
    pub(crate) fn command(
        &mut self,
        opcode: u16,
        p: &[u8],
    ) -> Result<CommandReply, cmd::Error<HciError<B::Error>>> {
        if opcode != 0x0c03 {
            Self::core_result(self.core.check())?;
        }
        let empty = CommandReply::new(&[]);
        match opcode {
            0x0c03 => {
                length(p, 0)?;
                Self::core_result(self.core.reset())?;
                self.address = None;
                self.parameters = None;
                self.data = None;
                self.scan = None;
                self.event_mask = 0x0000_1fff_ffff_ffff;
                self.le_mask = 1;
            }
            0x0c01 | 0x2001 => {
                length(p, 8)?;
                let mask = u64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]]);
                if opcode == 0x0c01 {
                    self.event_mask = mask;
                } else {
                    self.le_mask = mask;
                }
            }
            0x2002 => {
                length(p, 0)?;
                let length = if self.core.supports_data_length() {
                    251
                } else {
                    27
                };
                return Ok(CommandReply::new(&[length, 0, 4]));
            }
            0x2003 => {
                length(p, 0)?;
                return Ok(CommandReply::new(&self.core.le_features()));
            }
            #[cfg(feature = "data-length")]
            0x2022..=0x2024 | 0x202f if self.core.supports_data_length() => {
                return self.data_length_command(opcode, p);
            }
            0x200f => {
                length(p, 0)?;
                return Ok(CommandReply::new(&[0]));
            } // Filter accept list capacity is zero.
            0x1009 => {
                length(p, 0)?;
                return Ok(CommandReply::new(&[0; 6]));
            } // LE-only controller has no public identity.
            0x2005 | 0x2006 | 0x2008 | 0x2009 => self.config_command(opcode, p)?,
            0x200a => {
                length(p, 1)?;
                match p[0] {
                    0 => {
                        if self.core.is_advertising() {
                            Self::core_result(self.core.stop_advertising())?;
                        }
                    }
                    1 => {
                        let address = self.address.ok_or(Status::CMD_DISALLOWED)?;
                        let parameters = self.parameters.ok_or(Status::CMD_DISALLOWED)?;
                        let data = self.data.unwrap_or(
                            AdvertisingData::new(&[])
                                .map_err(|_| Status::INVALID_HCI_PARAMETERS)?,
                        );
                        let scan = self.scan.unwrap_or(
                            AdvertisingData::new(&[])
                                .map_err(|_| Status::INVALID_HCI_PARAMETERS)?,
                        );
                        Self::core_result(
                            self.core
                                .advertise(Advertisement::new(address, parameters, data, scan)),
                        )?;
                    }
                    _ => return Err(Status::INVALID_HCI_PARAMETERS.into()),
                }
            }
            0x0406 => {
                length(p, 3)?;
                let id = self
                    .core
                    .connection()
                    .filter(|id| id.handle() == word(p, 0))
                    .ok_or(Status::UNKNOWN_CONN_IDENTIFIER)?;
                if !matches!(p[2], 0x05 | 0x13..=0x15 | 0x1a | 0x29 | 0x3b) {
                    return Err(Status::INVALID_HCI_PARAMETERS.into());
                }
                Self::core_result(self.core.disconnect(id, p[2]))?;
            }
            _ => return Err(Status::UNKNOWN_CMD.into()),
        }
        Ok(empty)
    }
}
