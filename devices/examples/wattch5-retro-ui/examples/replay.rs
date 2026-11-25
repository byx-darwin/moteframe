//! Host-only snapshot export from the actual Rust model and row renderer.
#[path = "support/scenario.rs"]
mod scenario;
mod support;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 {
        return Err("usage:replay REFERENCE_DIRECTORY".into());
    }
    let dir = std::path::Path::new(&args[0]);
    let cases = std::fs::read_to_string(dir.join("cases.csv"))?;
    let mut count = 0;
    for line in cases.lines() {
        let (id, model) = scenario::replay(line)?;
        let pixels = support::render(model)?;
        let bytes: Vec<_> = pixels.into_iter().flat_map(u16::to_be_bytes).collect();
        std::fs::write(dir.join(format!("{id}.actual.rgb565")), bytes)?;
        count += 1;
    }
    println!("Rendered{count}states through real Rust model + device-ui row streaming");
    Ok(())
}
