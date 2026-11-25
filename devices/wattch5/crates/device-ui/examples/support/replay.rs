//! Host-only replay helpers. No reference page or business logic belongs here.

use std::{
    fmt, fs,
    io::{self, Write},
    path::Path,
};

use device_core::{Dimensions, Display, Rect, Rgb565 as Packed};
use device_ui::{DirtyRegion, graphics, repaint};
use graphics::{
    image::{Image, ImageRaw},
    pixelcolor::Rgb565,
    prelude::*,
};

const MAX_PIXELS: usize = 1_048_576;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn extent(width: &str, height: &str) -> io::Result<Dimensions> {
    let width = width.parse::<u16>().map_err(|_| invalid("invalid width"))?;
    let height = height
        .parse::<u16>()
        .map_err(|_| invalid("invalid height"))?;
    let count = usize::from(width) * usize::from(height);
    if count == 0 || count > MAX_PIXELS {
        return Err(invalid("empty or excessive image extent"));
    }
    Ok(Dimensions::new(width, height))
}

struct Sprite {
    position: Point,
    size: Dimensions,
    bytes: Vec<u8>,
}

impl fmt::Debug for Sprite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Sprite")
            .field("position", &self.position)
            .field("size", &self.size)
            .field("byte_count", &self.bytes.len())
            .finish()
    }
}

/// A host fixture of positioned, borrowed-at-draw-time RGB565 images.
#[derive(Debug)]
pub struct Scene {
    dimensions: Dimensions,
    sprites: Vec<Sprite>,
}

impl Scene {
    /// Reads a TSV manifest and exactly sized, big-endian RGB565 pixel assets.
    /// # Errors
    /// Rejects malformed geometry/lengths and propagates filesystem errors.
    pub fn load(manifest: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(manifest)?;
        let mut lines = contents.lines();
        let header: Vec<_> = lines
            .next()
            .ok_or_else(|| invalid("missing scene header"))?
            .split('\t')
            .collect();
        if header.len() != 3 || header[0] != "scene" {
            return Err(invalid("invalid scene header"));
        }
        let dimensions = extent(header[1], header[2])?;
        let directory = manifest
            .parent()
            .ok_or_else(|| invalid("missing manifest directory"))?;
        let mut sprites = Vec::new();
        for line in lines {
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 5 {
                return Err(invalid("invalid sprite record"));
            }
            let x = fields[0]
                .parse::<i32>()
                .map_err(|_| invalid("invalid x coordinate"))?;
            let y = fields[1]
                .parse::<i32>()
                .map_err(|_| invalid("invalid y coordinate"))?;
            let size = extent(fields[2], fields[3])?;
            let bytes = fs::read(directory.join(fields[4]))?;
            if bytes.len() != usize::from(size.width) * usize::from(size.height) * 2 {
                return Err(invalid("asset pixel count does not match sprite extent"));
            }
            sprites.push(Sprite {
                position: Point::new(x, y),
                size,
                bytes,
            });
        }
        Ok(Self {
            dimensions,
            sprites,
        })
    }

    fn draw<T: DrawTarget<Color = Rgb565>>(&self, target: &mut T) -> Result<(), T::Error> {
        for sprite in &self.sprites {
            let raw = ImageRaw::<Rgb565>::new(&sprite.bytes, u32::from(sprite.size.width));
            Image::new(&raw, sprite.position).draw(target)?;
        }
        Ok(())
    }
}

/// Host memory display and PPM exporter; never linked into runtime device-ui.
#[derive(Debug)]
pub struct Raster {
    dimensions: Dimensions,
    pixels: Vec<Packed>,
    writes: usize,
}

impl Display for Raster {
    type Error = &'static str;
    fn dimensions(&self) -> Dimensions {
        self.dimensions
    }
    fn write(&mut self, area: Rect, pixels: &[Packed]) -> Result<(), Self::Error> {
        if !area.fits(self.dimensions) || usize::try_from(area.area()).ok() != Some(pixels.len()) {
            return Err("invalid raster write");
        }
        for (row, colors) in pixels.chunks(usize::from(area.width)).enumerate() {
            let start = (usize::from(area.y) + row) * usize::from(self.dimensions.width)
                + usize::from(area.x);
            self.pixels[start..start + colors.len()].copy_from_slice(colors);
        }
        self.writes += 1;
        Ok(())
    }
}

impl Raster {
    /// Pixels in row-major order, useful for offline comparison.
    pub fn pixels(&self) -> &[Packed] {
        &self.pixels
    }

    /// Number of real Display writes performed during replay.
    pub const fn writes(&self) -> usize {
        self.writes
    }

    /// Exports the rendered RGB565 pixels as an RGB PPM.
    /// # Errors
    /// Propagates output errors rather than accepting partial output.
    pub fn save_ppm(&self, destination: &Path) -> io::Result<()> {
        let mut output = io::BufWriter::new(fs::File::create(destination)?);
        write!(
            output,
            "P6\n{} {}\n255\n",
            self.dimensions.width, self.dimensions.height
        )?;
        for Packed(pixel) in &self.pixels {
            let red = u8::try_from((pixel >> 11) * 255 / 31)
                .map_err(|_| invalid("invalid red channel"))?;
            let green = u8::try_from(((pixel >> 5) & 63) * 255 / 63)
                .map_err(|_| invalid("invalid green channel"))?;
            let blue = u8::try_from((pixel & 31) * 255 / 31)
                .map_err(|_| invalid("invalid blue channel"))?;
            output.write_all(&[red, green, blue])?;
        }
        output.flush()
    }
}

/// Replays a stable scene through the real dirty/tile pipeline.
/// # Errors
/// Reports the original display or repaint failure.
pub fn render(scene: &Scene, scratch: &mut [Packed]) -> io::Result<Raster> {
    let dimensions = scene.dimensions;
    let mut raster = Raster {
        dimensions,
        pixels: vec![Packed(0); usize::from(dimensions.width) * usize::from(dimensions.height)],
        writes: 0,
    };
    let mut dirty = DirtyRegion::new(dimensions);
    dirty.invalidate_all();
    repaint(&mut raster, &mut dirty, scratch, Rgb565::BLACK, |tile| {
        scene.draw(tile)
    })
    .map_err(|error| io::Error::other(format!("{error:?}")))?;
    Ok(raster)
}
