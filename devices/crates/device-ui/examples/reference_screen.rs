//! Replays an external static scene without changing the runtime UI framework.
#[path = "support/replay.rs"]
mod replay;

use device_core::Rgb565;
use std::{io, path::PathBuf};

fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let manifest = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("usage: reference_screen SCENE.tsv OUTPUT.ppm"))?,
    );
    let destination = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("missing OUTPUT.ppm"))?,
    );
    let scene = replay::Scene::load(&manifest)?;
    let raster = replay::render(&scene, &mut [Rgb565(0); 32])?;
    raster.save_ppm(&destination)?;
    let lit = raster.pixels().iter().filter(|pixel| pixel.0 != 0).count();
    println!(
        "Replayed {} pixels ({lit} nonblack), {} rectangle writes, 32 scratch pixels -> {}",
        raster.pixels().len(),
        raster.writes(),
        destination.display()
    );
    Ok(())
}
