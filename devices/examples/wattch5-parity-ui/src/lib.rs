//! Original-source UI parity with caller-owned snapshots and explicit service requests.
#![no_std]
#![forbid(unsafe_code)]
mod types;
pub use types::*;
mod screen;
pub use screen::Screen;
mod font;
pub use font::{GRAPHIC_DATA_BYTES, Glyph, glyph};
mod font_data;
mod snapshot;
pub use snapshot::*;
mod model;
pub use model::Model;

mod labels;
pub use labels::Label;
mod canvas;
pub use canvas::RenderError;
mod render;
pub use render::Scene;

mod text_layout;

mod damage;

mod background;

mod images;
pub use images::{ImageAssets, ImageError};
mod factory;
pub use factory::FactoryAssets;
