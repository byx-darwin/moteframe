//! Private bridges preserve product contracts while executing pinned public `MoteFrame`.
#![no_std]
#![forbid(unsafe_code)]
use device_core::{Dimensions, Display as Legacy, Rect, Rgb565};
use embedded_graphics::pixelcolor::{Rgb565 as Native, raw::RawU16};
use moteframe_core::Display;
/// Explicit GPIO adapter, forwarding the original failure.
#[derive(Debug)]
pub struct Pin<P>(pub P);
impl<P: device_core::OutputPin> moteframe_core::OutputPin for Pin<P> {
    type Error = P::Error;
    fn set_level(&mut self, v: bool) -> Result<(), Self::Error> {
        self.0.set_level(v)
    }
}
/// Owned resource source; the original read failure is retained.
pub struct ReadRef<R>(R);
impl<R> core::fmt::Debug for ReadRef<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReadRef").finish_non_exhaustive()
    }
}
impl<R> ReadRef<R> {
    /// Wrap the source without any I/O.
    #[must_use]
    pub const fn new(source: R) -> Self {
        Self(source)
    }
    /// Return the identical source.
    pub fn into_inner(self) -> R {
        self.0
    }
}
impl<R: device_core::ReadAt> moteframe_core::ReadAt for ReadRef<R> {
    type Error = R::Error;
    fn capacity(&self) -> u32 {
        self.0.capacity()
    }
    fn read_exact(&mut self, o: u32, b: &mut [u8]) -> Result<(), Self::Error> {
        self.0.read_exact(o, b)
    }
}
/// Borrow a legacy product display for the public UI renderer.
pub struct DisplayRef<'a, D: ?Sized>(&'a mut D);
impl<D: ?Sized> core::fmt::Debug for DisplayRef<'_, D> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DisplayRef").finish_non_exhaustive()
    }
}
impl<'a, D: ?Sized> DisplayRef<'a, D> {
    /// Borrow without submitting pixels.
    pub fn new(d: &'a mut D) -> Self {
        Self(d)
    }
}
impl<D: Legacy + ?Sized> Display for DisplayRef<'_, D> {
    type Color = Native;
    type Error = device_core::NativeWriteError<D::Error>;
    fn dimensions(&self) -> Dimensions {
        self.0.dimensions()
    }
    fn write(&mut self, r: Rect, p: &[Native]) -> Result<(), Self::Error> {
        self.0.write_rgb565(r, p)
    }
}
/// Public native-color display, retaining the original product Display API.
#[derive(Debug)]
pub struct NativeDisplay<D>(D);
impl<D> NativeDisplay<D> {
    /// Wrap a native display without I/O.
    #[must_use]
    pub const fn new(d: D) -> Self {
        Self(d)
    }
    /// Borrow the same display for initialization or board-controlled recovery.
    /// The mutable borrow prevents concurrent UI writes.
    pub fn inner_mut(&mut self) -> &mut D {
        &mut self.0
    }
    /// Release the original native display.
    pub fn into_inner(self) -> D {
        self.0
    }
}
impl<D: Display<Color = Native>> Legacy for NativeDisplay<D> {
    type Error = device_core::NativeWriteError<D::Error>;
    fn dimensions(&self) -> Dimensions {
        self.0.dimensions()
    }
    fn write(&mut self, r: Rect, p: &[Rgb565]) -> Result<(), Self::Error> {
        if !r.fits(Legacy::dimensions(self)) || u32::try_from(p.len()).ok() != Some(r.area()) {
            return Err(device_core::NativeWriteError::InvalidInput);
        }
        let mut native = [Native::from(RawU16::new(0)); 8];
        for (dy, row) in p.chunks(usize::from(r.width)).enumerate() {
            for (dx, chunk) in row.chunks(8).enumerate() {
                for (out, pixel) in native.iter_mut().zip(chunk) {
                    *out = Native::from(RawU16::new(pixel.0));
                }
                let dx = u16::try_from(dx * 8)
                    .map_err(|_| device_core::NativeWriteError::InvalidInput)?;
                let dy =
                    u16::try_from(dy).map_err(|_| device_core::NativeWriteError::InvalidInput)?;
                let w = u16::try_from(chunk.len())
                    .map_err(|_| device_core::NativeWriteError::InvalidInput)?;
                self.0
                    .write(
                        Rect::new(r.x + dx, r.y + dy, w, 1),
                        &native[..chunk.len()],
                    )
                    .map_err(device_core::NativeWriteError::Display)?;
            }
        }
        Ok(())
    }
    fn write_rgb565(
        &mut self,
        r: Rect,
        p: &[Native],
    ) -> Result<(), device_core::NativeWriteError<Self::Error>> {
        // Rebuilding the rectangle here emits a smaller routine with Cortex-M0 release LTO.
        self.0
            .write(moteframe_core::Rect::new(r.x, r.y, r.width, r.height), p)
            .map_err(|e| {
                device_core::NativeWriteError::Display(device_core::NativeWriteError::Display(e))
            })
    }
}
/// Verified private panel commands copied into the public driver unchanged.
#[must_use]
pub const fn initialization<const N: usize>(
    s: [device_display::InitStep<'static>; N],
) -> [moteframe_display::InitStep<'static>; N] {
    let mut out = [moteframe_display::InitStep::DelayMs(0); N];
    let mut i = 0;
    while i < N {
        out[i] = match s[i] {
            device_display::InitStep::Command(c, d) => moteframe_display::InitStep::Command(c, d),
            device_display::InitStep::DelayMs(ms) => moteframe_display::InitStep::DelayMs(ms),
        };
        i += 1;
    }
    out
}
/// Forward the original delay to the public profile initializer.
#[derive(Debug)]
pub struct Delay<D>(pub D);
impl<D: device_core::DelayMs> moteframe_core::DelayMs for Delay<D> {
    fn delay_ms(&mut self, n: u16) {
        self.0.delay_ms(n);
    }
}
