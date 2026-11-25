//! Offline candidate composition, independent of the diagnostic firmware entry.
#![no_std]
pub mod background;
pub mod boot;
pub mod gatt;
pub mod history;
pub mod schema;
pub mod ui;
pub mod v2_ui;
#[cfg(test)]
extern crate std;
#[cfg(test)]
#[path = "test_support/async_support.rs"]
mod async_support;
#[cfg(test)]
#[path = "test_support/history.rs"]
mod history_support;
#[cfg(test)]
#[path = "test_support/support.rs"]
mod support;

#[cfg(test)]
#[path = "test_support/background.rs"]
mod background_tests;
