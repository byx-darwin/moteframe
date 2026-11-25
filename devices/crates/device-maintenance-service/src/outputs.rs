//! Typed named-job output bodies, beneath the C42 envelope and actual product-policy checks.
use crate::Error;

/// A complete validated output. It carries no command status or schema prefix inside the body.
/// Parsing establishes structure only; actual observation/readback and original policy still come
/// from the durable engine and product `OperationSchema`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NamedOutput<'a> {
    body: &'a [u8],
    value: NamedOutputValue<'a>,
}
impl core::fmt::Debug for NamedOutput<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NamedOutput")
            .field("length", &self.body.len())
            .finish_non_exhaustive()
    }
}
/// Exact decoded fields for the selected named operation; none proves a physical PASS.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NamedOutputValue<'a> {
    ///72.OP01/02: actual selected sensor/touch identifier and product-defined driver status.
    #[cfg(feature = "engineering")]
    Driver {
        /// Actual selected sensor or touch identifier.
        identifier: u16,
        /// Driver-defined status under the actual product policy.
        status: u16,
    },
    ///72.OP03: manifest reference CRC and actual observed CRC/checked extent.
    #[cfg(feature = "engineering")]
    ResourceCheck {
        /// Expected CRC from the trusted resource manifest.
        expected_crc: u32,
        /// CRC of the actual observed resource bytes.
        actual_crc: u32,
        /// Actual extent covered by this check.
        bytes_checked: u32,
    },
    ///72.OP04: actual interrupts observed; zero is data, not automatic PASS.
    #[cfg(feature = "engineering")]
    InterruptCount(u32),
    ///72.OP05: actual ADC samples with explicit unit/scale from the original product policy.
    #[cfg(feature = "engineering")]
    AdcSamples(AdcSamples<'a>),
    ///72.OP06: actual ADC comparison fields. Criterion validation remains product-specific.
    #[cfg(feature = "engineering")]
    AdcCheck {
        /// Actual signed raw observation.
        raw: i32,
        /// Product-declared comparison reference.
        reference: i32,
        /// Actual signed comparison error.
        error: i32,
        /// Product-declared unit identifier.
        unit: u8,
        /// Decimal scale exponent for the declared unit.
        scale_exp: i8,
    },
    ///72.OP07/08: actual committed/restored calibration revision, never request echo proof.
    #[cfg(feature = "engineering")]
    CalibrationRevision(u32),
    ///72.OP09: bounded actual gesture observations.
    #[cfg(feature = "engineering")]
    GestureSamples(GestureSamples<'a>),
    ///72.OP10: actual duration and failures; elapsed duration alone never implies PASS.
    #[cfg(feature = "engineering")]
    Ageing {
        /// Actual measured duration in seconds.
        elapsed_s: u32,
        /// Actual observed failures in the product-defined bit allocation.
        failure_mask: u32,
    },
    ///72.OP11: actual measured off duration, never inferred solely from disconnect.
    #[cfg(feature = "engineering")]
    OffDurationMs(u32),
    ///`72.EXIT_TEST` declares no additional output; terminal status remains C41.
    #[cfg(feature = "engineering")]
    Empty,
    ///73.OP01: verified actual product ID readback.
    #[cfg(feature = "factory-config")]
    ProductId(u16),
    ///73.OP02: verified actual decimal serial; leading zeroes are retained.
    #[cfg(feature = "factory-config")]
    Serial(&'a str),
    ///73.OP03: verified actual selected subtype.
    #[cfg(feature = "factory-config")]
    Subtype(u16),
    ///73.OP04/05: actual enable state readback.
    #[cfg(feature = "factory-config")]
    Enabled(bool),
}
impl core::fmt::Debug for NamedOutputValue<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NamedOutputValue").finish_non_exhaustive()
    }
}
impl<'a> NamedOutput<'a> {
    /// Decode the exact operation's schema1 output, after the real engine's original-policy checks.
    /// # Errors
    /// Unsupported module/operation/schema, malformed fields or a body beyond the C42 hard carrier.
    pub fn decode(module: u8, operation: u16, schema: u16, body: &'a [u8]) -> Result<Self, Error> {
        if schema != 1 || body.len() > 502 {
            return Err(Error::Invalid);
        }
        let mut reader = Reader::new(body);
        let value = match module {
            #[cfg(feature = "engineering")]
            0x72 => engineering(operation, &mut reader)?,
            #[cfg(feature = "factory-config")]
            0x73 => factory(operation, &mut reader)?,
            _ => return Err(Error::Unsupported),
        };
        reader.end()?;
        Ok(Self { body, value })
    }
    /// Inspect typed fields without logging private sample/identity bytes.
    #[must_use]
    pub const fn value(&self) -> NamedOutputValue<'a> {
        self.value
    }
    /// Copy the same complete validated bytes. No partial output is written on short capacity.
    /// # Errors
    /// Insufficient caller capacity; no real storage or hardware effects occur here.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
        let destination = out.get_mut(..self.body.len()).ok_or(Error::Capacity)?;
        destination.copy_from_slice(self.body);
        Ok(self.body.len())
    }
}
#[cfg(feature = "engineering")]
fn engineering<'a>(operation: u16, r: &mut Reader<'a>) -> Result<NamedOutputValue<'a>, Error> {
    Ok(match operation {
        1 | 2 => NamedOutputValue::Driver {
            identifier: r.u16()?,
            status: r.u16()?,
        },
        3 => NamedOutputValue::ResourceCheck {
            expected_crc: r.u32()?,
            actual_crc: r.u32()?,
            bytes_checked: r.u32()?,
        },
        4 => NamedOutputValue::InterruptCount(r.u32()?),
        5 => {
            let channel = r.u16()?;
            let count = usize::from(r.u16()?);
            let bytes = r.take(count.checked_mul(4).ok_or(Error::Invalid)?)?;
            let unit = r.byte()?;
            let scale_exp = i8::from_ne_bytes([r.byte()?]);
            NamedOutputValue::AdcSamples(AdcSamples {
                channel,
                bytes,
                unit,
                scale_exp,
            })
        }
        6 => NamedOutputValue::AdcCheck {
            raw: r.i32()?,
            reference: r.i32()?,
            error: r.i32()?,
            unit: r.byte()?,
            scale_exp: i8::from_ne_bytes([r.byte()?]),
        },
        7 | 8 => NamedOutputValue::CalibrationRevision(r.u32()?),
        9 => {
            let count = r.u16()?;
            let start = r.position;
            for _ in 0..count {
                gesture_sample(r)?;
            }
            let bytes = r.body.get(start..r.position).ok_or(Error::Invalid)?;
            NamedOutputValue::GestureSamples(GestureSamples { count, bytes })
        }
        10 => NamedOutputValue::Ageing {
            elapsed_s: r.u32()?,
            failure_mask: r.u32()?,
        },
        11 => NamedOutputValue::OffDurationMs(r.u32()?),
        12 => NamedOutputValue::Empty,
        _ => return Err(Error::Unsupported),
    })
}
#[cfg(feature = "factory-config")]
fn factory<'a>(operation: u16, r: &mut Reader<'a>) -> Result<NamedOutputValue<'a>, Error> {
    Ok(match operation {
        1 => NamedOutputValue::ProductId(r.u16()?),
        2 => {
            let length = usize::from(r.u16()?);
            if !(1..=32).contains(&length) {
                return Err(Error::Invalid);
            }
            let bytes = r.take(length)?;
            if !bytes.iter().all(u8::is_ascii_digit) {
                return Err(Error::Invalid);
            }
            NamedOutputValue::Serial(core::str::from_utf8(bytes).map_err(|_| Error::Invalid)?)
        }
        3 => NamedOutputValue::Subtype(r.u16()?),
        4 | 5 => NamedOutputValue::Enabled(match r.byte()? {
            0 => false,
            1 => true,
            _ => return Err(Error::Invalid),
        }),
        _ => return Err(Error::Unsupported),
    })
}
#[cfg(feature = "engineering")]
fn scalar_samples(bytes: &[u8]) -> impl Iterator<Item = i32> {
    bytes
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
/// Validated ADC sample array; product policy determines actual unit/scale/calibration support.
#[cfg(feature = "engineering")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AdcSamples<'a> {
    channel: u16,
    bytes: &'a [u8],
    unit: u8,
    scale_exp: i8,
}
#[cfg(feature = "engineering")]
impl core::fmt::Debug for AdcSamples<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AdcSamples")
            .field("count", &(self.bytes.len() / 4))
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "engineering")]
impl AdcSamples<'_> {
    /// Actual channel identifier from the real original job.
    #[must_use]
    pub const fn channel(self) -> u16 {
        self.channel
    }
    /// Product-declared unit code. Structural decoding alone does not establish calibrated volts.
    #[must_use]
    pub const fn unit(self) -> u8 {
        self.unit
    }
    /// Decimal scale exponent, interpreted only with the real unit declaration.
    #[must_use]
    pub const fn scale_exp(self) -> i8 {
        self.scale_exp
    }
    /// Actual retained signed samples in wire order; no allocation or guessed conversion.
    pub fn samples(self) -> impl Iterator<Item = i32> {
        scalar_samples(self.bytes)
    }
}
/// One actual gesture observation, including all raw product-defined parameters.
#[cfg(feature = "engineering")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GestureSample<'a> {
    /// Actual monotonic observation time.
    pub monotonic_us: u64,
    /// Actual declared gesture identifier.
    pub gesture_id: u16,
    parameters: &'a [u8],
}
#[cfg(feature = "engineering")]
impl core::fmt::Debug for GestureSample<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GestureSample").finish_non_exhaustive()
    }
}
#[cfg(feature = "engineering")]
impl GestureSample<'_> {
    /// Raw typed signed parameters; actual units remain defined by the immutable job policy.
    pub fn parameters(self) -> impl Iterator<Item = i32> {
        scalar_samples(self.parameters)
    }
}
/// Complete bounded immutable gesture output.
#[cfg(feature = "engineering")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GestureSamples<'a> {
    count: u16,
    bytes: &'a [u8],
}
#[cfg(feature = "engineering")]
impl core::fmt::Debug for GestureSamples<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GestureSamples")
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "engineering")]
impl<'a> GestureSamples<'a> {
    /// Iterate every already-validated observation. An internal parse failure remains explicit.
    #[must_use]
    pub fn samples(self) -> GestureIter<'a> {
        GestureIter {
            remaining: self.count,
            reader: Reader::new(self.bytes),
        }
    }
}
/// Fallible, fixed-memory iterator over complete gesture observations.
#[cfg(feature = "engineering")]
pub struct GestureIter<'a> {
    remaining: u16,
    reader: Reader<'a>,
}
#[cfg(feature = "engineering")]
impl core::fmt::Debug for GestureIter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GestureIter")
            .field("remaining", &self.remaining)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "engineering")]
impl<'a> Iterator for GestureIter<'a> {
    type Item = Result<GestureSample<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let result = gesture_sample(&mut self.reader);
        if result.is_err() {
            self.remaining = 0;
        }
        Some(result)
    }
}
#[cfg(feature = "engineering")]
fn gesture_sample<'a>(r: &mut Reader<'a>) -> Result<GestureSample<'a>, Error> {
    let monotonic_us = r.u64()?;
    let gesture_id = r.u16()?;
    let count = usize::from(r.byte()?);
    let parameters = r.take(count.checked_mul(4).ok_or(Error::Invalid)?)?;
    Ok(GestureSample {
        monotonic_us,
        gesture_id,
        parameters,
    })
}
struct Reader<'a> {
    body: &'a [u8],
    position: usize,
}
impl<'a> Reader<'a> {
    const fn new(body: &'a [u8]) -> Self {
        Self { body, position: 0 }
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], Error> {
        let end = self.position.checked_add(length).ok_or(Error::Invalid)?;
        let bytes = self.body.get(self.position..end).ok_or(Error::Invalid)?;
        self.position = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().map_err(|_| Error::Invalid)?,
        ))
    }
    #[cfg(feature = "engineering")]
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| Error::Invalid)?,
        ))
    }
    #[cfg(feature = "engineering")]
    fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| Error::Invalid)?,
        ))
    }
    #[cfg(feature = "engineering")]
    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| Error::Invalid)?,
        ))
    }
    fn end(&self) -> Result<(), Error> {
        if self.position == self.body.len() {
            Ok(())
        } else {
            Err(Error::Invalid)
        }
    }
}
