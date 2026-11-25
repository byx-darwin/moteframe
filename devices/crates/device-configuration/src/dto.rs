use crate::{
    Error,
    wire::{Reader, Writer, ensure},
};
/// Partial battery fields; validity is independent of the numeric value zero.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub level: u8,
    pub charging: bool,
    pub charge_complete: bool,
    pub since_charge_s: u32,
    pub valid_mask: u8,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ui<'a> {
    pub version: &'a str,
    pub diff_supported: bool,
    pub valid: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Hardware<'a> {
    pub hardware: &'a str,
    pub screen_shape: u8,
    pub width: u16,
    pub height: u16,
    pub api_version: &'a str,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Reset {
    pub reason: u16,
    pub count: u32,
    pub utc_ms: i64,
    pub valid_mask: u8,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Address {
    pub address_type: u8,
    pub display_octets: [u8; 6],
    pub valid: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WindowObject {
    pub id: u16,
    pub parent: u16,
    pub visible: bool,
    pub order: u16,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeartPolicy {
    pub enabled: bool,
    pub mode: u8,
    pub interval_s: u32,
    pub rmssd_enabled: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Zones {
    pub sport_type: u16,
    pub zone_type: u8,
    pub resting: u16,
    pub maximum: u16,
    pub thresholds: [u16; 5],
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Glucose {
    pub enabled_mask: u8,
    pub before_x100: u16,
    pub after_x100: u16,
    pub breakfast_min: u16,
    pub lunch_min: u16,
    pub dinner_min: u16,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WellbeingImport {
    pub status: u8,
    pub mood: i16,
    pub fatigue: i16,
    pub stress: i16,
    pub valid_mask: u8,
}
/// Validated borrowed directory. The byte representation is private; items expose typed fields.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BrightnessLimits<'a> {
    bytes: &'a [u8],
}
impl<'a> BrightnessLimits<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut r = Reader::new(bytes);
        for _ in 0..2 {
            let count = r.u8()?;
            let mut prev = None;
            for _ in 0..count {
                let n = r.u16()?;
                ensure(n <= 1000 && prev.is_none_or(|p| p < n))?;
                prev = Some(n);
            }
        }
        r.end()?;
        Ok(Self { bytes })
    }
    pub fn saver_levels(self) -> impl Iterator<Item = u16> + 'a {
        let n = usize::from(self.bytes[0]);
        self.bytes[1..1 + n * 2]
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
    }
    pub fn normal_levels(self) -> impl Iterator<Item = u16> + 'a {
        let pos = 1 + usize::from(self.bytes[0]) * 2;
        self.bytes[pos + 1..]
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
    }
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        w.put(self.bytes)?;
        Ok(w.len())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HidMappings<'a> {
    bytes: &'a [u8],
}
impl<'a> HidMappings<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut r = Reader::new(bytes);
        r.boolean()?;
        ensure((1..=3).contains(&r.u8()?))?;
        let count = r.u8()?;
        let mut seen = [false; 256];
        for _ in 0..count {
            let g = r.u8()?;
            let action = r.u8()?;
            ensure(g <= 7 && action <= 10)?;
            ensure(!seen[usize::from(g)])?;
            seen[usize::from(g)] = true;
        }
        r.end()?;
        Ok(Self { bytes })
    }
    pub const fn enabled(self) -> bool {
        self.bytes[0] == 1
    }
    pub const fn platform(self) -> u8 {
        self.bytes[1]
    }
    pub fn mappings(self) -> impl Iterator<Item = (u8, u8)> + 'a {
        self.bytes[3..].chunks_exact(2).map(|b| (b[0], b[1]))
    }
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        w.put(self.bytes)?;
        Ok(w.len())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AlgorithmDirectory<'a> {
    bytes: &'a [u8],
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AlgorithmEntry<'a> {
    pub record_type: u16,
    pub source: u8,
    pub algorithm_id: &'a str,
    pub version: &'a str,
    pub unit: u8,
    pub scale_exp: i8,
    pub labels: Labels<'a>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Labels<'a> {
    bytes: &'a [u8],
    count: u8,
}
impl<'a> Labels<'a> {
    pub fn entries(self) -> impl Iterator<Item = (i32, &'a str)> {
        let mut r = Reader::new(self.bytes);
        (0..self.count).filter_map(move |_| {
            let n = r.i32().ok()?;
            let s = r.text().ok()?;
            Some((n, s))
        })
    }
}
fn algorithm_entry<'a>(r: &mut Reader<'a>) -> Result<AlgorithmEntry<'a>, Error> {
    let record_type = r.u16()?;
    let source = r.u8()?;
    let algorithm_id = r.text()?;
    let version = r.text()?;
    let unit = r.u8()?;
    let scale_exp = r.u8()? as i8;
    let count = r.u8()?;
    ensure(
        (0x2a01..=0x2a05).contains(&record_type)
            && (1..=2).contains(&source)
            && !algorithm_id.is_empty()
            && !version.is_empty(),
    )?;
    let start = r.position();
    for _ in 0..count {
        let prior_end = r.position();
        let n = r.i32()?;
        let text = r.text()?;
        ensure(!text.is_empty())?;
        let mut previous = Reader::new(r.slice(start, prior_end)?);
        while previous.position() < prior_end - start {
            ensure(previous.i32()? != n)?;
            let len = usize::from(previous.u16()?);
            previous.take(len)?;
        }
    }
    // Unit/exponent are explicit schema, not an algorithm implementation.
    ensure(match record_type {
        0x2a01 => unit == 2 && scale_exp == 0,
        0x2a02 => unit == 1 && scale_exp == 0,
        0x2a03 => unit == 3 && scale_exp == 0 && count > 0,
        0x2a04 => unit == 4 && (-3..=0).contains(&scale_exp),
        0x2a05 => unit == 5 && scale_exp == -2,
        _ => false,
    })?;
    let end = r.position();
    let bytes = r_source_slice(r, start, end)?;
    Ok(AlgorithmEntry {
        record_type,
        source,
        algorithm_id,
        version,
        unit,
        scale_exp,
        labels: Labels { bytes, count },
    })
}
fn r_source_slice<'a>(r: &Reader<'a>, start: usize, end: usize) -> Result<&'a [u8], Error> {
    r.slice(start, end)
}
impl<'a> AlgorithmDirectory<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        Self::decode_with_string_max(bytes, usize::from(u16::MAX))
    }
    pub fn decode_with_string_max(bytes: &'a [u8], max_text: usize) -> Result<Self, Error> {
        let mut r = Reader::with_max_text(bytes, max_text);
        let count = r.u8()?;
        let mut seen = 0u16;
        for _ in 0..count {
            let e = algorithm_entry(&mut r)?;
            let bit = 1u16 << ((e.record_type - 0x2a01) * 2 + u16::from(e.source - 1));
            ensure(seen & bit == 0)?;
            seen |= bit;
        }
        r.end()?;
        Ok(Self { bytes })
    }
    pub fn entries(self) -> impl Iterator<Item = AlgorithmEntry<'a>> {
        let mut r = Reader::new(&self.bytes[1..]);
        (0..self.bytes[0]).filter_map(move |_| algorithm_entry(&mut r).ok())
    }
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        w.put(self.bytes)?;
        Ok(w.len())
    }
}
/// Diagnostic parameter bytes are allowed only with a product-declared ID/reason/length dictionary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticEntry<'a> {
    pub mono_us: u64,
    pub utc_ms: i64,
    pub time_valid: bool,
    pub diagnostic_id: u16,
    pub reason: u16,
    pub parameters: &'a [u8],
    pub end_marker: u8,
    pub error_state: u8,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticLog<'a> {
    bytes: &'a [u8],
}
fn diagnostic_entry<'a>(r: &mut Reader<'a>) -> Result<DiagnosticEntry<'a>, Error> {
    let mono_us = r.u64()?;
    let utc_ms = r.i64()?;
    let time_valid = r.boolean()?;
    let diagnostic_id = r.u16()?;
    let reason = r.u16()?;
    let n = usize::from(r.u16()?);
    let parameters = r.take(n)?;
    let end_marker = r.u8()?;
    let error_state = r.u8()?;
    ensure(
        end_marker <= 2
            && error_state <= 2
            && (time_valid || utc_ms == 0)
            && (end_marker != 1 || error_state == 0)
            && (end_marker != 2 || error_state != 0),
    )?;
    Ok(DiagnosticEntry {
        mono_us,
        utc_ms,
        time_valid,
        diagnostic_id,
        reason,
        parameters,
        end_marker,
        error_state,
    })
}
impl<'a> DiagnosticLog<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut r = Reader::new(bytes);
        let count = r.u16()?;
        let mut previous = None;
        let mut ended = false;
        for _ in 0..count {
            ensure(!ended)?;
            let e = diagnostic_entry(&mut r)?;
            ensure(previous.is_none_or(|p| p <= e.mono_us))?;
            previous = Some(e.mono_us);
            ended = e.end_marker != 0;
        }
        r.end()?;
        Ok(Self { bytes })
    }
    pub fn entries(self) -> impl Iterator<Item = DiagnosticEntry<'a>> {
        let count = u16::from_le_bytes([self.bytes[0], self.bytes[1]]);
        let mut r = Reader::new(&self.bytes[2..]);
        (0..count).filter_map(move |_| diagnostic_entry(&mut r).ok())
    }
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        w.put(self.bytes)?;
        Ok(w.len())
    }
}
#[cfg(feature = "device")]
pub(crate) fn validate_battery(v: Battery) -> Result<(), Error> {
    ensure(v.valid_mask & !15 == 0 && v.level <= 100)?;
    ensure(v.valid_mask & 8 == 0 || v.valid_mask & 2 != 0)?;
    ensure(v.valid_mask & 0x0a != 0x0a || !v.charging || v.since_charge_s == 0)?;
    crate::wire::canonical(
        v.valid_mask,
        &[
            i128::from(v.level),
            i128::from(v.charging),
            i128::from(v.charge_complete),
            i128::from(v.since_charge_s),
        ],
    )
}
#[cfg(feature = "device")]
pub(crate) fn validate_reset(v: Reset) -> Result<(), Error> {
    ensure(v.valid_mask & !7 == 0 && v.reason <= 6)?;
    ensure(v.valid_mask & 1 == 0 || v.reason != 0)?;
    crate::wire::canonical(
        v.valid_mask,
        &[
            i128::from(v.reason),
            i128::from(v.count),
            i128::from(v.utc_ms),
        ],
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ModuleLimits {
    pub max_objects: u16,
    pub max_object_bytes: u32,
    pub max_string_bytes: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Label<'a> {
    pub value: i32,
    pub text: &'a str,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AlgorithmDefinition<'a> {
    pub record_type: u16,
    pub source: u8,
    pub algorithm_id: &'a str,
    pub version: &'a str,
    pub unit: u8,
    pub scale_exp: i8,
    pub labels: &'a [Label<'a>],
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub gesture: u8,
    pub action: u8,
}
impl AlgorithmDirectory<'_> {
    /// Encode typed definitions; discard output on error. This declares metadata, not an algorithm backend.
    pub fn encode_entries(
        entries: &[AlgorithmDefinition<'_>],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        w.u8(u8::try_from(entries.len()).map_err(|_| Error::Invalid)?)?;
        for e in entries {
            w.u16(e.record_type)?;
            w.u8(e.source)?;
            w.text(e.algorithm_id)?;
            w.text(e.version)?;
            w.u8(e.unit)?;
            w.u8(e.scale_exp as u8)?;
            w.u8(u8::try_from(e.labels.len()).map_err(|_| Error::Invalid)?)?;
            for label in e.labels {
                w.put(&label.value.to_le_bytes())?;
                w.text(label.text)?;
            }
        }
        let n = w.len();
        AlgorithmDirectory::decode(&out[..n])?;
        Ok(n)
    }
}
impl BrightnessLimits<'_> {
    pub fn encode_levels(saver: &[u16], normal: &[u16], out: &mut [u8]) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        for levels in [saver, normal] {
            w.u8(u8::try_from(levels.len()).map_err(|_| Error::Invalid)?)?;
            for level in levels {
                w.u16(*level)?;
            }
        }
        let n = w.len();
        BrightnessLimits::decode(&out[..n])?;
        Ok(n)
    }
}
impl HidMappings<'_> {
    pub fn encode_mappings(
        enabled: bool,
        platform: u8,
        mappings: &[Mapping],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        w.boolean(enabled)?;
        w.u8(platform)?;
        w.u8(u8::try_from(mappings.len()).map_err(|_| Error::Invalid)?)?;
        for m in mappings {
            w.u8(m.gesture)?;
            w.u8(m.action)?;
        }
        let n = w.len();
        HidMappings::decode(&out[..n])?;
        Ok(n)
    }
}
impl DiagnosticLog<'_> {
    pub fn encode_entries(entries: &[DiagnosticEntry<'_>], out: &mut [u8]) -> Result<usize, Error> {
        let mut w = Writer::new(out);
        w.u16(u16::try_from(entries.len()).map_err(|_| Error::Invalid)?)?;
        for e in entries {
            w.put(&e.mono_us.to_le_bytes())?;
            w.i64(e.utc_ms)?;
            w.boolean(e.time_valid)?;
            w.u16(e.diagnostic_id)?;
            w.u16(e.reason)?;
            w.u16(u16::try_from(e.parameters.len()).map_err(|_| Error::Invalid)?)?;
            w.put(e.parameters)?;
            w.u8(e.end_marker)?;
            w.u8(e.error_state)?;
        }
        let n = w.len();
        DiagnosticLog::decode(&out[..n])?;
        Ok(n)
    }
}
