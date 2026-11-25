//! Hardware-independent WAP wire protocol.
#![no_std]
mod error;
mod frame;
pub use error::ProtocolError;
pub use frame::{Frame, HEADER_LENGTH, Header, Kind, LOGICAL_MAX};
mod fragment;
pub use fragment::Fragmenter;
mod reassembly;
pub use reassembly::{Progress, Reassembler};
mod core;
mod crc;
pub use core::{HelloRequest, HelloResponse, ModuleEntry, ProtocolLimits};
pub use crc::{Crc32c, event_crc, object_crc};
mod bytes;
mod object;
pub use object::{ObjectBegin, ObjectRequest, PropertyRequest};

mod history;
pub use history::{
    HistoryDomain, HistoryDone, HistoryFilter, HistoryPage, HistoryRead, HistoryRecord,
    HistoryRequest, HistoryResponse, HistorySnapshot,
};
mod lane;
pub use lane::{ConnectionDataLane, DataKind, DataLease, LaneError, SharedDataLane};
#[cfg(feature = "raw-data")]
mod raw;
#[cfg(feature = "raw-data")]
pub use raw::RawBatch;
