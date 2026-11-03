// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

#![no_std]
#![forbid(unsafe_code)]
#[cfg(foundation_layout)]
use core::mem::size_of;
#[cfg(foundation_layout)]
use moteframe_device_foundation::{
    ASSET_BYTES, Demo, INPUT_EVENTS, RAW_INPUTS, Services, UI_PIXELS, UiMemory, VALUE_BYTES,
    model::{LinkModel, Sink, SourceModel},
    scene::{Catalog, RenderScratch},
};
#[cfg(foundation_layout)]
use moteframe_ui::{RefreshSession, graphics::pixelcolor::Rgb565};
#[cfg(foundation_layout)]
macro_rules! layout {
    ($name:ident,$size:expr) => {
        pub static $name: [u8; $size] = [0; $size];
    };
}
#[cfg(foundation_layout)]
pub mod sizes {
    use super::*;
    layout!(APP_UI, size_of::<UiMemory>());
    layout!(APP_DEMO, size_of::<Demo<'static, LinkModel>>());
    layout!(APP_SESSION, size_of::<RefreshSession<'static, Rgb565>>());
    layout!(APP_SERVICES, size_of::<Services<LinkModel>>());
    layout!(APP_LINK, size_of::<LinkModel>());
    layout!(APP_CATALOG, size_of::<Catalog>());
    layout!(APP_RENDER, size_of::<RenderScratch>());
    layout!(APP_SOURCE, size_of::<SourceModel>());
    layout!(APP_SINK, size_of::<Sink>());
    layout!(APP_PIXEL_BYTES, UI_PIXELS * 2);
    layout!(APP_RAW_CAPACITY, RAW_INPUTS);
    layout!(APP_EVENT_CAPACITY, INPUT_EVENTS);
    layout!(APP_VALUE_BYTES, VALUE_BYTES);
    layout!(APP_ASSET_BYTES, ASSET_BYTES);
}
/// Zero is an explicit failure sentinel rejected by the report validator.
#[inline(never)]
pub fn exercise(seed: u32) -> u32 {
    let mut sink = moteframe_device_foundation::model::Sink::new();
    match moteframe_device_foundation::probe::exercise(&mut sink, seed) {
        Ok(o)
            if o.samples == 16
                && o.events == 16
                && o.accepted == 16
                && o.received == 16
                && o.recoveries == 3 =>
        {
            o.tx_fingerprint.wrapping_add(1)
        }
        _ => 0,
    }
}
