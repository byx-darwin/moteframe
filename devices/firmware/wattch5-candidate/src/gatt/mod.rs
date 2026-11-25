//! Real bounded CONTROL and DATA queues over upstream TrouBLE.
mod driver;
mod fifo;
mod profile;
mod transport;
pub use driver::drive_connection;
pub use fifo::{Bridge, Queue};
pub use profile::{CONTROL_MAX, ControlProfile, ControlUuids};
pub use transport::TroubleTransport;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_history;
