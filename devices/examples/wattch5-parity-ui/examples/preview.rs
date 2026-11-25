//! Loopback preview of the actual Rust V2 model and row renderer.
#[path = "support/app.rs"]
mod app;
#[path = "../../wattch5-ui/examples/support/http.rs"]
mod http;
mod support;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::serve()
}
