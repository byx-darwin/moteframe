// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Complete offline framework consumer, not a device or a business application.
#![no_std]
#![forbid(unsafe_code)]

/// Synthetic offline ports, never hardware or radio implementations.
pub mod model;
/// Bounded scheduling and retained queue state.
pub mod services;
pub use services::{EnqueueError, Link, Resolution, Sample, Services, Snapshot, TxState, TxToken};

/// Real UI pixel array length owned by this consumer.
pub const UI_PIXELS: usize = if cfg!(feature = "small-memory") {
    8
} else {
    32
};
/// Real raw input queue capacity.
pub const RAW_INPUTS: usize = if cfg!(feature = "small-memory") { 2 } else { 8 };
/// Real capacity of each recognized input queue.
pub const INPUT_EVENTS: usize = if cfg!(feature = "small-memory") { 1 } else { 4 };
/// Actual byte storage in the single pending notification and RX slot.
pub const VALUE_BYTES: usize = if cfg!(feature = "small-memory") {
    8
} else {
    16
};
/// Real caller-owned encoded asset scratch.
pub const ASSET_BYTES: usize = if cfg!(feature = "small-memory") {
    2
} else {
    16
};

/// Long-lived application-owned assembly.
pub mod runtime;
/// Synthetic scene and resource descriptors.
pub mod scene;
pub use runtime::{Demo, StepReport, UiMemory};
pub use scene::Catalog;

/// Seeded combined offline workload, also used by the measured ARM consumer.
pub mod probe;
