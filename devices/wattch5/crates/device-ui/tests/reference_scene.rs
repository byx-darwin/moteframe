//! Generic host replay contracts using hand-authored tiny pixel fixtures.
#[path = "../examples/support/replay.rs"]
mod replay;

use device_core::Rgb565;
use replay::{Scene, render};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture(manifest: &str, pixels: &[u8]) -> std::io::Result<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/ui-migration/fixtures")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&path)?;
    fs::write(path.join("scene.tsv"), manifest)?;
    fs::write(path.join("sprite.rgb565"), pixels)?;
    Ok(path)
}

#[test]
fn test_should_replay_clipped_sprites_in_order_and_export_actual_rgb_pixels() -> std::io::Result<()>
{
    let path = fixture(
        "scene\t3\t2\n-1\t0\t2\t2\tsprite.rgb565\n0\t1\t1\t1\toverlay.rgb565\n",
        &[0xf8, 0, 0x07, 0xe0, 0, 0x1f, 0xff, 0xff],
    )?;
    fs::write(path.join("overlay.rgb565"), [0, 0x1f])?;
    let scene = Scene::load(&path.join("scene.tsv"))?;
    let raster = render(&scene, &mut [Rgb565(0); 1])?;
    assert_eq!(
        raster.pixels(),
        &[
            Rgb565(0x07e0),
            Rgb565(0),
            Rgb565(0),
            Rgb565(0x001f),
            Rgb565(0),
            Rgb565(0)
        ]
    );
    assert_eq!(raster.writes(), 6);
    let ppm = path.join("actual.ppm");
    raster.save_ppm(&ppm)?;
    assert_eq!(
        fs::read(ppm)?,
        [
            b"P6\n3 2\n255\n".as_slice(),
            &[0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0]
        ]
        .concat()
    );
    Ok(())
}

#[test]
fn test_should_reject_truncated_assets_and_malformed_geometry() -> std::io::Result<()> {
    for manifest in [
        "scene\t0\t2\n",
        "scene\t65535\t65535\n",
        "scene\t2\t2\n0\t0\t0\t2\tsprite.rgb565\n",
        "scene\t2\t2\n0\t0\t2\t2\tsprite.rgb565\n",
        "scene\t2\t2\n0\t0\t1\t1\n",
    ] {
        let path = fixture(manifest, &[0, 0])?;
        assert!(
            Scene::load(&path.join("scene.tsv")).is_err(),
            "accepted malformed fixture: {manifest}"
        );
    }
    Ok(())
}

#[test]
fn test_should_report_missing_asset_and_empty_scratch() -> std::io::Result<()> {
    let path = fixture("scene\t2\t1\n0\t0\t1\t1\tmissing.rgb565\n", &[0, 0])?;
    assert_eq!(
        Scene::load(&path.join("scene.tsv"))
            .err()
            .map(|error| error.kind()),
        Some(std::io::ErrorKind::NotFound)
    );
    fs::write(
        path.join("scene.tsv"),
        "scene\t2\t1\n0\t0\t1\t1\tsprite.rgb565\n",
    )?;
    let scene = Scene::load(&path.join("scene.tsv"))?;
    assert!(render(&scene, &mut []).is_err());
    Ok(())
}
