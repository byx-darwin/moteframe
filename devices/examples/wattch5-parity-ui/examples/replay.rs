//! Actual Rust row-streamed output, reconstructed through public model actions.
#[path = "support/scenario.rs"]
mod scenario;
mod support;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("usage:replay REFERENCE_DIRECTORY")?;
    let dir = std::path::Path::new(&dir);
    let text = std::fs::read_to_string(dir.join("cases.csv"))?;
    let mut count = 0;
    for line in text.lines() {
        let (id, m, d) = scenario::replay(line)
            .map_err(|e| format!("case {}:{e}", line.split(',').next().unwrap_or("?")))?;
        let pixels = support::render(&m, &d).map_err(|e| format!("case{id}:{e}"))?;
        let bytes: Vec<_> = pixels.into_iter().flat_map(u16::to_be_bytes).collect();
        std::fs::write(dir.join(format!("{id}.actual.rgb565")), bytes)?;
        count += 1;
    }
    println!("Rendered {count} V2 states through actual Rust row renderer");
    Ok(())
}
