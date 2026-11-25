//! Host validation export through the same checked header and span decoder as the device.
use device_assets::Memory;
use device_core::Rgb565;
use std::{error::Error, fs, io::Write, path::Path};
use wattch5_ui_demo::open_legacy;
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("usage:inspect_resources SOURCE OUTPUT HEX_ADDRESS...".into());
    }
    let bytes = fs::read(&args[0])?;
    let mut source = Memory::new(&bytes).map_err(|e| format!("{e:?}"))?;
    let output = Path::new(&args[1]);
    fs::create_dir_all(output)?;
    for text in &args[2..] {
        let address = u32::from_str_radix(text.trim_start_matches("0x"), 16)?;
        let image =
            open_legacy(&mut source, address).map_err(|e| format!("{address:08x}:{e:?}"))?;
        let mut file = fs::File::create(output.join(format!("{address:08x}.rgb565")))?;
        let mut pixels = [Rgb565(0); 128];
        let width = usize::from(image.width());
        for y in 0..image.height() {
            image
                .read_span(&mut source, 0, y, &mut pixels[..width])
                .map_err(|e| format!("{e:?}"))?;
            for pixel in &pixels[..width] {
                file.write_all(&pixel.0.to_be_bytes())?;
            }
        }
        println!("{address:08x},{},{}", image.width(), image.height());
    }
    Ok(())
}
