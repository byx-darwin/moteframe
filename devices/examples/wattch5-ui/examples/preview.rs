//! Host replay of the actual `no_std` reference UI; all values are demo inputs.
mod support;
use std::{env, error::Error, fs, path::Path};
use wattch5_ui_demo::{Bundle, Event, Model};
fn step(model: &mut Model, event: Event) -> Result<(), Box<dyn Error>> {
    model.handle(event).map_err(|e| format!("{e:?}"))?;
    Ok(())
}
fn snapshot(name: &str) -> Result<Model, Box<dyn Error>> {
    let mut model = Model::new(10, 50, 3).map_err(|e| format!("{e:?}"))?;
    if name == "dial" {
        return Ok(model);
    }
    step(&mut model, Event::Up)?;
    if name == "menu0" {
        return Ok(model);
    }
    step(&mut model, Event::Up)?;
    if name == "menu1" {
        return Ok(model);
    }
    step(&mut model, Event::Tap { x: 10, y: 20 })?;
    if let Some(index) = name.strip_prefix("settings") {
        let page = index.parse::<u8>()?;
        if page > 2 {
            return Err("invalid page".into());
        }
        for _ in 0..page {
            step(&mut model, Event::Up)?;
        }
        return Ok(model);
    }
    if let Some(index) = name.strip_prefix("brightness") {
        let level = index.parse::<u8>()?;
        if level > 4 {
            return Err("invalid level".into());
        }
        step(&mut model, Event::Tap { x: 90, y: 110 })?;
        while model.level() != level {
            step(&mut model, Event::Up)?;
        }
        return Ok(model);
    }
    Err("unknown snapshot".into())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        return Err("usage:preview bundle.bin output-directory | --serve".into());
    }
    let bytes = fs::read(&args[1])?;
    let assets = Bundle::new(&bytes).map_err(|e| format!("{e:?}"))?;
    if args[2] == "--serve" {
        return support::serve(&assets);
    }
    let output = Path::new(&args[2]);
    fs::create_dir_all(output)?;
    for name in [
        "dial",
        "menu0",
        "menu1",
        "settings0",
        "settings1",
        "settings2",
        "brightness0",
        "brightness1",
        "brightness2",
        "brightness3",
        "brightness4",
    ] {
        let pixels = support::render(snapshot(name)?, &assets)?;
        let raw: Vec<_> = pixels.iter().flat_map(|p| p.to_be_bytes()).collect();
        fs::write(output.join(format!("{name}.actual.rgb565")), raw)?;
    }
    println!("Rendered11snapshots through actual wattch5-ui-demo + device-ui");
    Ok(())
}
