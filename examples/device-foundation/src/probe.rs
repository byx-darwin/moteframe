// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! One executed composition for replay and combined cost accounting.
use crate::{
    Catalog, Demo, Sample, Services, UiMemory,
    model::{Error, LinkModel, SourceModel},
    services::ServiceReport,
};
use moteframe_core::Display;
use moteframe_input::{ButtonConfig, TouchConfig};
use moteframe_ui::{RefreshSettings, graphics::pixelcolor::Rgb565};
/// Failure of the fixed offline workload; never reports unexpected work as success.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// Construction/configuration failed.
    Configuration,
    /// Unexpected input/notification/source fixture admission failure.
    Admission,
    /// Unexpected render/service failure, including a missing expected fault.
    Execution,
    /// Fixed workload did not drain completely.
    Incomplete,
}
/// Logical observations that must be independent of tile/queue capacities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Observation {
    /// Processed normalized samples.
    pub samples: u32,
    /// Consumed events.
    pub events: u32,
    /// Queue admissions (not peer ACK).
    pub accepted: u32,
    /// Complete writes received.
    pub received: u32,
    /// Original error paths deliberately encountered then recovered.
    pub recoveries: u32,
    /// Final generic swatch.
    pub selection: u8,
    /// Trace of actual admitted notification contents.
    pub tx_fingerprint: u32,
}
fn service_ok(r: &ServiceReport<Error>) -> bool {
    r.poll_error.is_none() && r.receive_error.is_none() && r.input_error.is_none()
}
/// Execute a fixed synthetic trace, faults and recovery through the real assembly.
/// The display can retain pixels on host or be a framebuffer-free ARM sink.
/// # Errors
/// Any unexpected error/acceptance outcome aborts with a specific workload stage.
#[inline(never)]
pub fn exercise<D: Display<Color = Rgb565, Error = Error>>(
    display: &mut D,
    seed: u32,
) -> Result<Observation, ProbeError> {
    let catalog = Catalog::new(0).map_err(|_| ProbeError::Configuration)?;
    let mut memory = UiMemory::new(&catalog).map_err(|_| ProbeError::Configuration)?;
    let services = Services::new(
        core::num::NonZeroU64::MIN,
        LinkModel::new(),
        ButtonConfig::new(0, Some(100)).map_err(|_| ProbeError::Configuration)?,
        TouchConfig::new(32, 16, 1, 4, 100).map_err(|_| ProbeError::Configuration)?,
    );
    let mut demo = Demo::new(&mut memory, services, catalog, RefreshSettings::new())
        .map_err(|_| ProbeError::Configuration)?;
    let mut source = SourceModel::new();
    let initial = demo.step(display, 1, &mut source, 0);
    if initial.draw.is_err() || !service_ok(&initial.services) {
        return Err(ProbeError::Execution);
    }
    let mut received = 0;
    for n in 0u8..16 {
        let value = (seed.wrapping_add(u32::from(n))).to_le_bytes()[0];
        demo.services_mut()
            .enqueue_input(Sample::Button {
                pressed: n.is_multiple_of(2),
                at_us: u64::from(n),
            })
            .map_err(|_| ProbeError::Admission)?;
        let target = demo.services().link().target;
        demo.services_mut()
            .link_mut()
            .inject(target, &[value])
            .map_err(|_| ProbeError::Admission)?;
        demo.services_mut()
            .enqueue_notification(&[value, n])
            .map_err(|_| ProbeError::Admission)?;
        if n.is_multiple_of(4) {
            demo.services_mut().link_mut().blocks = 1;
        }
        let report = demo.step(display, 1, &mut source, u64::from(n) + 1);
        if report.draw.is_err() || !service_ok(&report.services) {
            return Err(ProbeError::Execution);
        }
        if report.services.notify_error.is_some() && !n.is_multiple_of(4) {
            return Err(ProbeError::Execution);
        }
        received += report.services.received;
    }
    // Source error retains full redraw, then a new revision identifies repaired content.
    demo.update_resources(1)
        .map_err(|_| ProbeError::Configuration)?;
    source.fail_at = Some(source.reads + 1);
    let fault = demo.step(display, 1, &mut source, 20);
    if fault.draw.is_ok() || !service_ok(&fault.services) {
        return Err(ProbeError::Execution);
    }
    source.fail_at = None;
    demo.update_resources(2)
        .map_err(|_| ProbeError::Configuration)?;
    // A new generation is not silently accepted; explicit rebind restores drawing.
    let fault = demo.step(display, 2, &mut source, 21);
    if !matches!(fault.draw, Err(moteframe_ui::SessionError::NeedsRebind))
        || !service_ok(&fault.services)
    {
        return Err(ProbeError::Execution);
    }
    demo.rebind(display, 2)
        .map_err(|_| ProbeError::Configuration)?;
    demo.services_mut().link_mut().poll_faults = 2;
    let repair = demo.step(display, 2, &mut source, 22);
    if repair.draw.is_err()
        || repair.services.poll_error != Some(Error::Poll)
        || repair.services.receive_error.is_some()
        || repair.services.input_error.is_some()
    {
        return Err(ProbeError::Execution);
    }
    let final_frame = demo.step(display, 2, &mut source, 23);
    if final_frame.draw.is_err()
        || !service_ok(&final_frame.services)
        || demo.pending().is_some()
        || demo.services().queued_inputs() != 0
        || demo.services().pending_token().is_some()
    {
        return Err(ProbeError::Incomplete);
    }
    Ok(Observation {
        samples: demo.services().processed_samples(),
        events: demo.services().processed_events(),
        accepted: demo.services().link().accepted,
        received,
        recoveries: 3,
        selection: demo.services().snapshot().selection,
        tx_fingerprint: demo.services().link().fingerprint,
    })
}
