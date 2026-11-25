//! Concrete CONTROL-only bridge to the pinned upstream TrouBLE Host and GATT.
//!
//! The caller owns the actual server, accepted connection, Host runner, monotonic
//! time/critical-section drivers and this bridge. Pin `runner.run()` and
//! `drive_connection(...)` externally, then lend both futures to `TroubleTransport`.
//! Drive it continuously from the same serialized foreground as `Runtime::step`.
//! Logical poll budgets do not establish a radio/display/Flash timing guarantee.
//! Notification admission means FIFO publication, never delivery to the peer.
mod driver;
mod fifo;
mod profile;
mod transport;
pub use driver::drive_connection;
pub use fifo::{Bridge, Queue};
pub use profile::{CONTROL_MAX, ControlProfile, ControlUuids};
pub use transport::TroubleTransport;
