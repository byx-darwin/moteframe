//! Portable retro digital presentation: injected snapshots and explicit action requests only.
#![no_std]
#![forbid(unsafe_code)]
mod types;
pub use types::*;
mod model;
pub use model::Model;
mod font;
mod render;
pub use render::{GRAPHIC_DATA_BYTES, RenderError};
