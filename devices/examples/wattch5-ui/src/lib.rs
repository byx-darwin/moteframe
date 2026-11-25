//! wattch 5 reference presentation only: injected data and emitted UI actions.
#![no_std]
#![forbid(unsafe_code)]
mod assets;
mod state;
pub use assets::{AssetError, AssetId, Bundle};
pub use state::{Action, Destination, Event, InputError, Model, Page};
mod render;
pub use render::RenderError;
mod legacy;
pub use legacy::{LegacyError, open_legacy};
