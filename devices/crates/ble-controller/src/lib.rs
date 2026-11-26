//! Portable, bounded legacy BLE Controller logic and chip backend contracts.
#![no_std]
#![forbid(unsafe_code)]

mod packet;
mod parameters;
mod queue;
pub use packet::{FrameKind, InvalidFrame, LinkFrame};
pub use parameters::{
    AdvertisingData, AdvertisingParameters, ConnectionParameters, InvalidParameters, RandomAddress,
};
pub use queue::{OwnedQueue, QueueFull};

mod data_length;
#[cfg(feature = "data-length")]
mod dle_queues;
pub use data_length::DataLength;
mod backend;
mod controller;
#[cfg(feature = "data-length")]
pub use backend::LengthResponseStatus;
pub use backend::{Advertisement, LinkBackend, LinkEvent, Peer, TxAdmission};
pub use controller::{ConnectionId, Controller, ControllerError, ControllerEvent, Version};
mod command;
mod event;
mod hci;
pub use command::CommandReply;
pub use event::HciBuffer;
#[cfg(any(feature = "external-wake", feature = "data-length"))]
pub use hci::HciControllerRef;
pub use hci::{HciController, HciError};
