// Feature-gated match tables can have only diverging arms or unused bindings in a minimal build.
#![allow(
    unreachable_code,
    unused_variables,
    unused_mut,
    unused_imports,
    dead_code
)]
use crate::{
    wire::{Reader, Writer, canonical, ensure},
    *,
};
fn d(
    module: u8,
    key: u16,
    max_length: u64,
    access: Access,
    schema_type: SchemaType,
    revision: u16,
) -> SchemaDescriptor {
    SchemaDescriptor {
        module,
        key,
        max_length,
        access,
        schema_type,
        revision,
    }
}
/// Descriptor of a compiled validator. This is not a backend capability claim.
pub fn property_descriptor(module: u8, key: u16) -> Result<SchemaDescriptor, Error> {
    use Access::{ReadOnly as R, ReadWrite as W};
    use SchemaType::*;
    if key == 0xf000 && cfg!(feature = "limits") && compiled_module(module) {
        return Ok(d(module, key, 8, R, Struct, 1));
    }
    let (max, access, kind, rev) = match (module, key) {
        #[cfg(feature = "time")]
        (1, 1) => (65541, W, Struct, 1),
        #[cfg(feature = "device")]
        (2, 1) => (8, R, Struct, 1),
        #[cfg(feature = "device")]
        (2, 2) => (65545, R, Struct, 1),
        #[cfg(feature = "device")]
        (2, 3) => (65539, R, Struct, 1),
        #[cfg(feature = "device")]
        (2, 4) => (131079, R, Struct, 1),
        #[cfg(feature = "device")]
        (2, 5) => (65537, W, Utf8, 1),
        #[cfg(feature = "device")]
        (2, 6) => (15, R, Struct, 1),
        #[cfg(feature = "device")]
        (2, 7) => (8, R, Struct, 1),
        #[cfg(feature = "profile")]
        (3, 1) => (14, W, Struct, 1),
        #[cfg(feature = "profile")]
        (3, 2) => (18, W, Struct, 1),
        #[cfg(feature = "settings")]
        (4, 1 | 5) => (1, W, Boolean, 1),
        #[cfg(feature = "settings")]
        (4, 2) => (2, W, Struct, 1),
        #[cfg(feature = "settings")]
        (4, 3) => (1, W, Struct, 1),
        #[cfg(feature = "settings")]
        (4, 4 | 8) => (6, W, Struct, 1),
        #[cfg(feature = "settings")]
        (4, 6) => (8, W, Struct, 1),
        #[cfg(feature = "settings")]
        (4, 7) => (65537, W, Utf8, 1),
        #[cfg(feature = "settings")]
        (4, 9 | 10) => (2, W, Struct, 1),
        #[cfg(feature = "settings")]
        (4, 11) => (1022, R, Directory, 1),
        #[cfg(feature = "reminders")]
        (5, 1) => (6, W, Struct, 1),
        #[cfg(feature = "find")]
        (6, 1) => (3, W, Struct, 1),
        #[cfg(feature = "telemetry")]
        (9, 1) => (6, W, Struct, 1),
        #[cfg(feature = "extension")]
        (0x11, 1) => (66, W, Extension, 1),
        #[cfg(feature = "heart-rate")]
        (0x20, 1) => (7, W, Struct, 1),
        #[cfg(feature = "heart-rate")]
        (0x20, 2) => (3, W, Struct, 1),
        #[cfg(feature = "heart-rate")]
        (0x20, 3) => (17, W, Struct, 1),
        #[cfg(feature = "sleep")]
        (0x21, 1) => (6, W, Struct, 1),
        #[cfg(feature = "sleep")]
        (0x21, 2) => (18, R, Struct, 1),
        #[cfg(feature = "sport")]
        (0x22, 1) => (8, W, Struct, 2),
        #[cfg(all(feature = "sport", feature = "heart-rate"))]
        (0x22, 2) => (17, W, Struct, 1),
        #[cfg(feature = "activity")]
        (0x23, 1) => (4, W, Struct, 1),
        #[cfg(feature = "blood-pressure")]
        (0x25, 2) => (3, W, Struct, 1),
        #[cfg(feature = "blood-pressure")]
        (0x25, 3) => (5, W, Struct, 1),
        #[cfg(feature = "blood-pressure")]
        (0x25, 0xf0) => (65537, R, Utf8, 1),
        #[cfg(feature = "oxygen")]
        (0x26, 1) => (11, W, Struct, 1),
        #[cfg(feature = "oxygen")]
        (0x26, 2) => (3, W, Struct, 1),
        #[cfg(feature = "glucose")]
        (0x27, 1) => (11, W, Struct, 1),
        #[cfg(feature = "temperature")]
        (0x28, 1) => (6, W, Struct, 2),
        #[cfg(feature = "temperature")]
        (0x28, 2) => (4, W, Struct, 2),
        #[cfg(feature = "temperature")]
        (0x28, 3) => (3, W, Struct, 1),
        #[cfg(feature = "wellbeing")]
        (0x2a, 1) => (1, W, Boolean, 1),
        // Ten unique (type,source) pairs; each has at most255 labels.
        #[cfg(feature = "wellbeing")]
        (0x2a, 2) => (168440351, R, Directory, 1),
        #[cfg(feature = "wellbeing")]
        (0x2a, 3) => (8, W, Struct, 1),
        #[cfg(feature = "womens-health")]
        (0x2d, 1) => (10, W, Struct, 1),
        #[cfg(feature = "uv")]
        (0x2e, 1) => (8, W, Struct, 1),
        #[cfg(feature = "antplus")]
        (0x38, 1) => (1, W, Boolean, 2),
        #[cfg(feature = "antplus")]
        (0x38, 2) => (1, R, Struct, 2),
        #[cfg(feature = "radiation")]
        (0x39, 1) => (2, W, Struct, 1),
        #[cfg(feature = "engineering")]
        (0x72, 1) => (1, W, Boolean, 1),
        #[cfg(feature = "engineering")]
        (0x72, 2) => (2, W, Struct, 1),
        #[cfg(feature = "engineering")]
        (0x72, 3) => (8, W, Struct, 1),
        #[cfg(feature = "engineering")]
        (0x72, 4) => (2, W, Struct, 1),
        _ => return Err(Error::Unknown),
    };
    Ok(d(module, key, max, access, kind, rev))
}
/// Write validation. RO writes are rejected before examining their value.
pub fn validate_property(
    module: u8,
    key: u16,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
) -> Result<SchemaDescriptor, Error> {
    let desc = property_descriptor(module, key)?;
    if desc.access == Access::ReadOnly {
        return Err(Error::Readonly);
    }
    validate_property_value(module, key, bytes, caps)
}
/// Decode and validate actual readback, including read-only DTOs.
pub fn validate_property_value(
    module: u8,
    key: u16,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
) -> Result<SchemaDescriptor, Error> {
    decode_property(module, key, bytes, caps)?;
    property_descriptor(module, key)
}
fn range(value: u32, limits: Option<Range>) -> Result<(), Error> {
    let r = limits.ok_or(Error::Unavailable)?;
    ensure(r.min <= r.max && value >= r.min && value <= r.max)
}
fn interval(module: u8, value: u32, caps: &ProductCapabilities<'_>) -> Result<(), Error> {
    let h = caps
        .health
        .ok_or(Error::Unavailable)?
        .iter()
        .find(|h| h.module == module)
        .ok_or(Error::Unavailable)?;
    ensure(module == 0x20 || value > 0)?;
    range(value, Some(h.interval_s))
}
fn temperature_mode(mode: u8, caps: &ProductCapabilities<'_>) -> Result<(), Error> {
    ensure((1..=4).contains(&mode))?;
    let h = caps
        .health
        .ok_or(Error::Unavailable)?
        .iter()
        .find(|h| h.module == 0x28)
        .ok_or(Error::Unavailable)?;
    if h.temperature_modes & (1 << (mode - 1)) != 0 {
        Ok(())
    } else {
        Err(Error::Unsupported)
    }
}
/// Pure codec entry point. No state is persisted and no action/backend is invoked.
pub fn decode_property<'a>(
    module: u8,
    key: u16,
    bytes: &'a [u8],
    caps: &ProductCapabilities<'_>,
) -> Result<Value<'a>, Error> {
    let desc = property_descriptor(module, key)?;
    ensure(bytes.len() as u64 <= desc.max_length)?;
    let mut r = Reader::with_max_text(bytes, string_max(caps));
    if key == 0xf000 && cfg!(feature = "limits") {
        let v = ModuleLimits {
            max_objects: r.u16()?,
            max_object_bytes: r.u32()?,
            max_string_bytes: r.u16()?,
        };
        r.end()?;
        return Ok(Value::ModuleLimits(v));
    }
    let value = match (module, key) {
        #[cfg(feature = "time")]
        (1, 1) => Value::Timezones(Timezones {
            offset_min: r.i16()?,
            second_offset_min: r.i16()?,
            second_city: r.text()?,
        }),
        #[cfg(feature = "device")]
        (2, 1) => {
            let v = Battery {
                level: r.u8()?,
                charging: r.boolean()?,
                charge_complete: r.boolean()?,
                since_charge_s: r.u32()?,
                valid_mask: r.u8()?,
            };
            crate::dto::validate_battery(v)?;
            Value::Battery(v)
        }
        #[cfg(feature = "device")]
        (2, 2) => Value::Identity(Identity {
            product: r.u32()?,
            subtype: r.u16()?,
            group: r.u16()?,
            serial: r.text()?,
        }),
        #[cfg(feature = "device")]
        (2, 3) => Value::Ui(Ui {
            version: r.text()?,
            diff_supported: r.boolean()?,
            valid: r.boolean()?,
        }),
        #[cfg(feature = "device")]
        (2, 4) => {
            let v = Hardware {
                hardware: r.text()?,
                screen_shape: r.u8()?,
                width: r.u16()?,
                height: r.u16()?,
                api_version: r.text()?,
            };
            ensure(v.screen_shape <= 3)?;
            Value::Hardware(v)
        }
        #[cfg(feature = "device")]
        (2, 5) => Value::Text(r.text()?),
        #[cfg(feature = "device")]
        (2, 6) => {
            let v = Reset {
                reason: r.u16()?,
                count: r.u32()?,
                utc_ms: r.i64()?,
                valid_mask: r.u8()?,
            };
            crate::dto::validate_reset(v)?;
            Value::Reset(v)
        }
        #[cfg(feature = "device")]
        (2, 7) => {
            let v = Address {
                address_type: r.u8()?,
                display_octets: r.array()?,
                valid: r.boolean()?,
            };
            ensure(
                v.address_type <= 3
                    && (v.valid || (v.address_type == 0 && v.display_octets == [0; 6])),
            )?;
            Value::Address(v)
        }
        #[cfg(feature = "profile")]
        (3, 1) => {
            return device_models::UserProfile::decode(bytes)
                .map(Value::UserProfile)
                .map_err(|_| Error::Invalid);
        }
        #[cfg(feature = "profile")]
        (3, 2) => {
            return device_models::ActivityGoals::decode(bytes)
                .map(Value::ActivityGoals)
                .map_err(|_| Error::Invalid);
        }
        #[cfg(feature = "settings")]
        (4, 1 | 5) => Value::Boolean(r.boolean()?),
        #[cfg(feature = "settings")]
        (4, 2) => {
            let distance = r.u8()?;
            let temperature = r.u8()?;
            ensure(distance <= 1 && temperature <= 1)?;
            Value::Units {
                distance,
                temperature,
            }
        }
        #[cfg(feature = "settings")]
        (4, 3) => {
            let v = r.u8()?;
            ensure(v <= 1)?;
            Value::HourFormat(v)
        }
        #[cfg(feature = "settings")]
        (4, 4 | 8) => return window(bytes),
        #[cfg(feature = "settings")]
        (4, 6) => {
            let main_ms = r.u32()?;
            let other_ms = r.u32()?;
            range(main_ms, caps.display_ms)?;
            range(other_ms, caps.display_ms)?;
            Value::DisplayTimeouts { main_ms, other_ms }
        }
        #[cfg(feature = "settings")]
        (4, 7) => {
            let text = r.text()?;
            if !caps.languages.ok_or(Error::Unavailable)?.contains(&text) {
                return Err(Error::Unsupported);
            }
            Value::Text(text)
        }
        #[cfg(feature = "settings")]
        (4, 9 | 10) => {
            let v = r.u16()?;
            ensure(v <= 1000)?;
            let levels = if key == 9 {
                caps.saver_levels
            } else {
                caps.normal_levels
            }
            .ok_or(Error::Unavailable)?;
            ensure(levels.iter().all(|n| *n <= 1000) && levels.windows(2).all(|w| w[0] < w[1]))?;
            if !levels.contains(&v) {
                return Err(Error::Unsupported);
            }
            Value::Brightness(v)
        }
        #[cfg(feature = "settings")]
        (4, 11) => return BrightnessLimits::decode(bytes).map(Value::BrightnessLimits),
        #[cfg(feature = "reminders")]
        (5, 1) => return window(bytes),
        #[cfg(feature = "find")]
        (6, 1) => Value::Find {
            enabled: r.boolean()?,
            delay_s: r.u16()?,
        },
        #[cfg(feature = "telemetry")]
        (9, 1) => {
            let enabled = r.boolean()?;
            let mask = r.u8()?;
            let interval = r.u32()?;
            r.end()?;
            if enabled {
                return Err(Error::Unsupported);
            }
            ensure(mask == 0 && interval == 0)?;
            Value::TelemetryDisabled
        }
        #[cfg(feature = "extension")]
        (0x11, 1) => {
            let n = usize::from(r.u16()?);
            ensure(n <= 64)?;
            Value::Extension(r.take(n)?)
        }
        #[cfg(feature = "heart-rate")]
        (0x20, 1) => {
            let v = HeartPolicy {
                enabled: r.boolean()?,
                mode: r.u8()?,
                interval_s: r.u32()?,
                rmssd_enabled: r.boolean()?,
            };
            ensure((1..=2).contains(&v.mode))?;
            interval(module, v.interval_s, caps)?;
            ensure(!v.rmssd_enabled || v.enabled)?;
            if v.rmssd_enabled && !caps.rmssd_allowed {
                return Err(Error::Unavailable);
            }
            Value::HeartPolicy(v)
        }
        #[cfg(any(feature = "heart-rate", feature = "blood-pressure", feature = "oxygen"))]
        (0x20 | 0x25 | 0x26, 2) => {
            let enabled = r.boolean()?;
            let value = r.u16()?;
            if module == 0x26 {
                ensure(value <= 10000)?;
            }
            Value::Threshold { enabled, value }
        }
        #[cfg(feature = "heart-rate")]
        (0x20, 3) => Value::Zones(zones(&mut r)?),
        #[cfg(feature = "sleep")]
        (0x21, 1) => return window(bytes),
        #[cfg(feature = "sleep")]
        (0x21, 2) => {
            return device_motion::SleepDetection::decode(bytes)
                .map(Value::SleepDetection)
                .map_err(|_| Error::Invalid);
        }
        #[cfg(feature = "sport")]
        (0x22, 1) => {
            let enabled = r.boolean()?;
            let delay_s = r.u32()?;
            let sport_type = r.u16()?;
            let trigger_kind = r.u8()?;
            ensure(sport_type <= 23 && trigger_kind == 1)?;
            if enabled {
                let supported = caps.automatic_sports.ok_or(Error::Unavailable)?;
                if !supported.contains(&sport_type) {
                    return Err(Error::Unsupported);
                }
                range(delay_s, caps.auto_sport_delay_s)?;
            }
            Value::SportAuto {
                enabled,
                delay_s,
                sport_type,
                trigger_kind,
            }
        }
        #[cfg(all(feature = "sport", feature = "heart-rate"))]
        (0x22, 2) => Value::Zones(zones(&mut r)?),
        #[cfg(feature = "activity")]
        (0x23, 1) => {
            let seconds = r.u32()?;
            ensure(seconds > 0)?;
            range(seconds, caps.activity_segment_s)?;
            Value::ActivitySegment(seconds)
        }
        #[cfg(feature = "blood-pressure")]
        (0x25, 3) => Value::BloodPressureReference {
            enabled: r.boolean()?,
            high: r.u16()?,
            low: r.u16()?,
        },
        #[cfg(feature = "blood-pressure")]
        (0x25, 0xf0) => {
            caps.limits.ok_or(Error::Unavailable)?;
            let version = r.text()?;
            ensure(!version.is_empty())?;
            Value::BloodPressureVersion(version)
        }
        #[cfg(feature = "oxygen")]
        (0x26, 1) => {
            let enabled = r.boolean()?;
            let interval_s = r.u32()?;
            interval(module, interval_s, caps)?;
            let window =
                device_models::TimeWindow::decode(r.take(6)?).map_err(|_| Error::Invalid)?;
            Value::OxygenPolicy {
                enabled,
                interval_s,
                window,
            }
        }
        #[cfg(feature = "glucose")]
        (0x27, 1) => {
            let v = Glucose {
                enabled_mask: r.u8()?,
                before_x100: r.u16()?,
                after_x100: r.u16()?,
                breakfast_min: r.u16()?,
                lunch_min: r.u16()?,
                dinner_min: r.u16()?,
            };
            ensure(
                v.enabled_mask & !31 == 0
                    && v.breakfast_min < 1440
                    && v.lunch_min < 1440
                    && v.dinner_min < 1440
                    && (v.enabled_mask & 1 == 0 || v.before_x100 > 0)
                    && (v.enabled_mask & 2 == 0 || v.after_x100 > 0),
            )?;
            canonical(
                v.enabled_mask,
                &[
                    i128::from(v.before_x100),
                    i128::from(v.after_x100),
                    i128::from(v.breakfast_min),
                    i128::from(v.lunch_min),
                    i128::from(v.dinner_min),
                ],
            )?;
            Value::Glucose(v)
        }
        #[cfg(feature = "temperature")]
        (0x28, 1) => {
            let enabled = r.boolean()?;
            let interval_s = r.u32()?;
            let mode = r.u8()?;
            interval(module, interval_s, caps)?;
            temperature_mode(mode, caps)?;
            Value::TemperaturePolicy {
                enabled,
                interval_s,
                mode,
            }
        }
        #[cfg(feature = "temperature")]
        (0x28, 2) => {
            let enabled = r.boolean()?;
            let centi_c = r.i16()?;
            let mode = r.u8()?;
            temperature_mode(mode, caps)?;
            Value::TemperatureAlarm {
                enabled,
                centi_c,
                mode,
            }
        }
        #[cfg(feature = "temperature")]
        (0x28, 3) => Value::TemperatureReference {
            enabled: r.boolean()?,
            centi_c: r.i16()?,
        },
        #[cfg(feature = "wellbeing")]
        (0x2a, 1) => {
            let enabled = r.boolean()?;
            if enabled && !caps.rmssd_allowed {
                return Err(Error::Unavailable);
            }
            Value::Boolean(enabled)
        }
        #[cfg(feature = "wellbeing")]
        (0x2a, 2) => {
            return AlgorithmDirectory::decode_with_string_max(bytes, string_max(caps))
                .map(Value::AlgorithmDirectory);
        }
        #[cfg(feature = "wellbeing")]
        (0x2a, 3) => {
            let v = WellbeingImport {
                status: r.u8()?,
                mood: r.i16()?,
                fatigue: r.i16()?,
                stress: r.i16()?,
                valid_mask: r.u8()?,
            };
            ensure(
                (1..=3).contains(&v.status)
                    && v.valid_mask & !7 == 0
                    && (v.status == 1 || v.valid_mask == 0)
                    && (0..=4).contains(&v.fatigue)
                    && (0..=100).contains(&v.stress),
            )?;
            canonical(
                v.valid_mask,
                &[
                    i128::from(v.mood),
                    i128::from(v.fatigue),
                    i128::from(v.stress),
                ],
            )?;
            if v.valid_mask & 1 != 0
                && !caps
                    .mood_values
                    .ok_or(Error::Unavailable)?
                    .contains(&v.mood)
            {
                return Err(Error::Unsupported);
            }
            Value::WellbeingImport(v)
        }
        #[cfg(feature = "womens-health")]
        (0x2d, 1) => {
            return device_wellness::WomensConfig::decode(bytes)
                .map(Value::Womens)
                .map_err(|_| Error::Invalid);
        }
        #[cfg(feature = "uv")]
        (0x2e, 1) => {
            return device_wellness::UvPolicy::decode(bytes)
                .map(Value::UvPolicy)
                .map_err(|_| Error::Invalid);
        }
        #[cfg(feature = "antplus")]
        (0x38, 1) => {
            let enabled = r.boolean()?;
            if !caps.antplus_backend {
                return Err(Error::Unavailable);
            }
            Value::Boolean(enabled)
        }
        #[cfg(feature = "antplus")]
        (0x38, 2) => {
            let state = r.u8()?;
            ensure(state <= 6)?;
            if !caps.antplus_backend {
                return Err(Error::Unavailable);
            }
            Value::AntState(state)
        }
        #[cfg(feature = "radiation")]
        (0x39, 1) => {
            let n = r.u16()?;
            ensure((1..=1000).contains(&n))?;
            Value::RadiationThreshold(n)
        }
        #[cfg(feature = "engineering")]
        (0x72, 1) => {
            engineering(caps)?;
            Value::Boolean(r.boolean()?)
        }
        #[cfg(feature = "engineering")]
        (0x72, 2) => {
            engineering(caps)?;
            let enabled = r.boolean()?;
            let read_policy = r.u8()?;
            ensure(read_policy <= 3 && (enabled || read_policy == 0))?;
            Value::LogPolicy {
                enabled,
                read_policy,
            }
        }
        #[cfg(feature = "engineering")]
        (0x72, 3) => {
            engineering(caps)?;
            let amplitude = r.u32()?;
            let minimum_interval_ms = r.u32()?;
            range(amplitude, caps.amplitude)?;
            range(minimum_interval_ms, caps.tap_interval_ms)?;
            Value::Tap {
                amplitude,
                minimum_interval_ms,
            }
        }
        #[cfg(feature = "engineering")]
        (0x72, 4) => {
            engineering(caps)?;
            let n = r.u16()?;
            range(u32::from(n), caps.wear_counts)?;
            Value::WearThreshold(n)
        }
        _ => return Err(Error::Unknown),
    };
    r.end()?;
    Ok(value)
}
#[cfg(any(
    feature = "settings",
    feature = "oxygen",
    feature = "reminders",
    feature = "sleep"
))]
fn window(bytes: &[u8]) -> Result<Value<'_>, Error> {
    device_models::TimeWindow::decode(bytes)
        .map(Value::Window)
        .map_err(|_| Error::Invalid)
}
fn zones(r: &mut Reader<'_>) -> Result<Zones, Error> {
    let sport_type = r.u16()?;
    let zone_type = r.u8()?;
    let resting = r.u16()?;
    let maximum = r.u16()?;
    let mut thresholds = [0; 5];
    for n in &mut thresholds {
        *n = r.u16()?;
    }
    ensure(
        sport_type <= 23
            && zone_type == 1
            && resting > 0
            && resting < maximum
            && thresholds[0] > 0
            && thresholds.windows(2).all(|w| w[0] < w[1]),
    )?;
    Ok(Zones {
        sport_type,
        zone_type,
        resting,
        maximum,
        thresholds,
    })
}
fn engineering(caps: &ProductCapabilities<'_>) -> Result<(), Error> {
    if caps.engineering {
        Ok(())
    } else {
        Err(Error::Unavailable)
    }
}
/// Typed encoding. On any error the caller must discard output; encoding is not transactional.
pub fn encode_property(
    module: u8,
    key: u16,
    value: Value<'_>,
    out: &mut [u8],
    caps: &ProductCapabilities<'_>,
) -> Result<usize, Error> {
    property_descriptor(module, key)?;
    let n = encode_value(value, out)?;
    let decoded = decode_property(module, key, &out[..n], caps)?;
    ensure(core::mem::discriminant(&decoded) == core::mem::discriminant(&value))?;
    Ok(n)
}
fn encode_value(value: Value<'_>, out: &mut [u8]) -> Result<usize, Error> {
    let mut w = Writer::new(out);
    match value {
        Value::ModuleLimits(v) => {
            w.u16(v.max_objects)?;
            w.u32(v.max_object_bytes)?;
            w.u16(v.max_string_bytes)?;
        }
        Value::Timezones(v) => {
            w.i16(v.offset_min)?;
            w.i16(v.second_offset_min)?;
            w.text(v.second_city)?;
        }
        Value::Identity(v) => {
            w.u32(v.product)?;
            w.u16(v.subtype)?;
            w.u16(v.group)?;
            w.text(v.serial)?;
        }
        Value::Text(v) => w.text(v)?,
        Value::Boolean(v) => w.boolean(v)?,
        Value::Battery(v) => {
            w.u8(v.level)?;
            w.boolean(v.charging)?;
            w.boolean(v.charge_complete)?;
            w.u32(v.since_charge_s)?;
            w.u8(v.valid_mask)?;
        }
        Value::Ui(v) => {
            w.text(v.version)?;
            w.boolean(v.diff_supported)?;
            w.boolean(v.valid)?;
        }
        Value::Hardware(v) => {
            w.text(v.hardware)?;
            w.u8(v.screen_shape)?;
            w.u16(v.width)?;
            w.u16(v.height)?;
            w.text(v.api_version)?;
        }
        Value::Reset(v) => {
            w.u16(v.reason)?;
            w.u32(v.count)?;
            w.i64(v.utc_ms)?;
            w.u8(v.valid_mask)?;
        }
        Value::Address(v) => {
            w.u8(v.address_type)?;
            w.put(&v.display_octets)?;
            w.boolean(v.valid)?;
        }
        Value::Units {
            distance,
            temperature,
        } => {
            w.u8(distance)?;
            w.u8(temperature)?;
        }
        Value::HourFormat(v) => w.u8(v)?,
        Value::DisplayTimeouts { main_ms, other_ms } => {
            w.u32(main_ms)?;
            w.u32(other_ms)?;
        }
        Value::Brightness(v) => w.u16(v)?,
        Value::BrightnessLimits(v) => return v.encode(out),
        #[cfg(any(
            feature = "settings",
            feature = "oxygen",
            feature = "reminders",
            feature = "sleep"
        ))]
        Value::Window(v) => return v.encode(out).map_err(model_error),
        Value::WindowObject(v) => {
            w.u16(v.id)?;
            w.u16(v.parent)?;
            w.boolean(v.visible)?;
            w.u16(v.order)?;
        }
        Value::Find { enabled, delay_s } => {
            w.boolean(enabled)?;
            w.u16(delay_s)?;
        }
        Value::TelemetryDisabled => w.put(&[0; 6])?,
        Value::HeartPolicy(v) => {
            w.boolean(v.enabled)?;
            w.u8(v.mode)?;
            w.u32(v.interval_s)?;
            w.boolean(v.rmssd_enabled)?;
        }
        Value::Threshold { enabled, value } => {
            w.boolean(enabled)?;
            w.u16(value)?;
        }
        Value::BloodPressureReference { enabled, high, low } => {
            w.boolean(enabled)?;
            w.u16(high)?;
            w.u16(low)?;
        }
        #[cfg(feature = "blood-pressure")]
        Value::BloodPressureVersion(version) => w.text(version)?,
        #[cfg(feature = "oxygen")]
        Value::OxygenPolicy {
            enabled,
            interval_s,
            window,
        } => {
            w.boolean(enabled)?;
            w.u32(interval_s)?;
            w.u8(window.enabled)?;
            w.u8(window.weekday_mask)?;
            w.u16(window.start_min)?;
            w.u16(window.end_min)?;
        }
        Value::Zones(v) => {
            w.u16(v.sport_type)?;
            w.u8(v.zone_type)?;
            w.u16(v.resting)?;
            w.u16(v.maximum)?;
            for n in v.thresholds {
                w.u16(n)?;
            }
        }
        Value::Glucose(v) => {
            w.u8(v.enabled_mask)?;
            w.u16(v.before_x100)?;
            w.u16(v.after_x100)?;
            w.u16(v.breakfast_min)?;
            w.u16(v.lunch_min)?;
            w.u16(v.dinner_min)?;
        }
        Value::TemperaturePolicy {
            enabled,
            interval_s,
            mode,
        } => {
            w.boolean(enabled)?;
            w.u32(interval_s)?;
            w.u8(mode)?;
        }
        Value::TemperatureAlarm {
            enabled,
            centi_c,
            mode,
        } => {
            w.boolean(enabled)?;
            w.i16(centi_c)?;
            w.u8(mode)?;
        }
        Value::TemperatureReference { enabled, centi_c } => {
            w.boolean(enabled)?;
            w.i16(centi_c)?;
        }
        Value::WellbeingImport(v) => {
            w.u8(v.status)?;
            w.i16(v.mood)?;
            w.i16(v.fatigue)?;
            w.i16(v.stress)?;
            w.u8(v.valid_mask)?;
        }
        Value::AlgorithmDirectory(v) => return v.encode(out),
        Value::Hid(v) => return v.encode(out),
        Value::AntState(v) => w.u8(v)?,
        Value::LogPolicy {
            enabled,
            read_policy,
        } => {
            w.boolean(enabled)?;
            w.u8(read_policy)?;
        }
        Value::Tap {
            amplitude,
            minimum_interval_ms,
        } => {
            w.u32(amplitude)?;
            w.u32(minimum_interval_ms)?;
        }
        Value::WearThreshold(v) => w.u16(v)?,
        Value::DiagnosticLog(v) => return v.encode(out),
        #[cfg(feature = "profile")]
        Value::UserProfile(v) => return v.encode(out).map_err(model_error),
        #[cfg(feature = "profile")]
        Value::ActivityGoals(v) => return v.encode(out).map_err(model_error),
        #[cfg(feature = "reminders")]
        Value::Reminder(v) => return v.encode(out, u16::MAX.into()).map_err(model_error),
        #[cfg(feature = "womens-health")]
        Value::Womens(v) => return v.encode(out).map_err(wellness_error),
        #[cfg(feature = "uv")]
        Value::UvPolicy(v) => return v.encode(out).map_err(wellness_error),
        #[cfg(feature = "sleep")]
        Value::SleepDetection(v) => return v.encode(out).map_err(motion_error),
        Value::SportAuto {
            enabled,
            delay_s,
            sport_type,
            trigger_kind,
        } => {
            w.boolean(enabled)?;
            w.u32(delay_s)?;
            w.u16(sport_type)?;
            w.u8(trigger_kind)?;
        }
        Value::ActivitySegment(v) => w.u32(v)?,
        Value::RadiationThreshold(v) => w.u16(v)?,
        Value::Extension(v) => {
            w.u16(u16::try_from(v.len()).map_err(|_| Error::Invalid)?)?;
            w.put(v)?;
        }
    }
    Ok(w.len())
}
pub fn object_descriptor(module: u8, kind: u16) -> Result<SchemaDescriptor, Error> {
    use Access::{ReadOnly as R, ReadWrite as W};
    let (max, access) = match (module, kind) {
        #[cfg(feature = "settings")]
        (4, 1) => (7, W),
        #[cfg(feature = "reminders")]
        (5, 1..=6) => (65569, W),
        #[cfg(feature = "hid")]
        (0x37, 1) => (19, W),
        #[cfg(feature = "engineering")]
        (0x72, 1) => (4296474602, R),
        _ => return Err(Error::Unknown),
    };
    Ok(d(module, kind, max, access, SchemaType::Object, 1))
}
pub fn validate_object(
    module: u8,
    kind: u16,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
) -> Result<SchemaDescriptor, Error> {
    let desc = object_descriptor(module, kind)?;
    if desc.access == Access::ReadOnly {
        return Err(Error::Readonly);
    }
    validate_object_value(module, kind, bytes, caps)
}
pub fn validate_object_value(
    module: u8,
    kind: u16,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
) -> Result<SchemaDescriptor, Error> {
    decode_object(module, kind, bytes, caps)?;
    object_descriptor(module, kind)
}
pub fn decode_object<'a>(
    module: u8,
    kind: u16,
    bytes: &'a [u8],
    caps: &ProductCapabilities<'_>,
) -> Result<Value<'a>, Error> {
    let desc = object_descriptor(module, kind)?;
    ensure(bytes.len() as u64 <= desc.max_length)?;
    if let Some(limits) = caps.limits {
        ensure(bytes.len() as u64 <= u64::from(limits.max_object_bytes))?;
        if limits.max_objects == 0 {
            return Err(Error::Unsupported);
        }
    }
    let mut r = Reader::with_max_text(bytes, string_max(caps));
    let value = match (module, kind) {
        #[cfg(feature = "settings")]
        (4, 1) => {
            let v = WindowObject {
                id: r.u16()?,
                parent: r.u16()?,
                visible: r.boolean()?,
                order: r.u16()?,
            };
            ensure(
                window_id(v.id) && (v.parent == 65535 || window_id(v.parent)) && v.id != v.parent,
            )?;
            if !caps
                .windows
                .ok_or(Error::Unavailable)?
                .contains(&WindowCapability {
                    id: v.id,
                    parent: v.parent,
                })
            {
                return Err(Error::Unsupported);
            }
            Value::WindowObject(v)
        }
        #[cfg(feature = "reminders")]
        (5, 1..=6) => {
            return device_models::Reminder::decode(kind, bytes, string_max(caps))
                .map(Value::Reminder)
                .map_err(|_| Error::Invalid);
        }
        #[cfg(feature = "hid")]
        (0x37, 1) => {
            let v = HidMappings::decode(bytes)?;
            let bindings = caps.hid.ok_or(Error::Unavailable)?;
            for (gesture, action) in v.mappings() {
                if !bindings.contains(&HidBinding {
                    platform: v.platform(),
                    gesture,
                    action,
                }) {
                    return Err(Error::Unsupported);
                }
            }
            return Ok(Value::Hid(v));
        }
        #[cfg(feature = "engineering")]
        (0x72, 1) => {
            engineering(caps)?;
            let v = DiagnosticLog::decode(bytes)?;
            for e in v.entries() {
                let dict = caps.diagnostics.ok_or(Error::Unavailable)?;
                let diagnostic = dict
                    .iter()
                    .find(|d| {
                        d.id == e.diagnostic_id
                            && d.reason == e.reason
                            && usize::from(d.parameter_length) == e.parameters.len()
                    })
                    .ok_or(Error::Unsupported)?;
                ensure(diagnostic.revision > 0 && (diagnostic.validate_parameters)(e.parameters))?;
            }
            return Ok(Value::DiagnosticLog(v));
        }
        _ => return Err(Error::Unknown),
    };
    r.end()?;
    Ok(value)
}
pub fn encode_object(
    module: u8,
    kind: u16,
    value: Value<'_>,
    out: &mut [u8],
    caps: &ProductCapabilities<'_>,
) -> Result<usize, Error> {
    object_descriptor(module, kind)?;
    let n = encode_value(value, out)?;
    let decoded = decode_object(module, kind, &out[..n], caps)?;
    ensure(core::mem::discriminant(&decoded) == core::mem::discriminant(&value))?;
    Ok(n)
}

#[allow(clippy::match_like_matches_macro)] // cfg! values differ for each feature subset.
fn compiled_module(module: u8) -> bool {
    match module {
        1 => cfg!(feature = "time"),
        2 => cfg!(feature = "device"),
        3 => cfg!(feature = "profile"),
        4 => cfg!(feature = "settings"),
        5 => cfg!(feature = "reminders"),
        6 => cfg!(feature = "find"),
        9 => cfg!(feature = "telemetry"),
        0x11 => cfg!(feature = "extension"),
        0x20 => cfg!(feature = "heart-rate"),
        0x25 => cfg!(feature = "blood-pressure"),
        0x26 => cfg!(feature = "oxygen"),
        0x27 => cfg!(feature = "glucose"),
        0x28 => cfg!(feature = "temperature"),
        0x2a => cfg!(feature = "wellbeing"),
        0x21 => cfg!(feature = "sleep"),
        0x22 => cfg!(feature = "sport"),
        0x23 => cfg!(feature = "activity"),
        0x2d => cfg!(feature = "womens-health"),
        0x2e => cfg!(feature = "uv"),
        0x37 => cfg!(feature = "hid"),
        0x38 => cfg!(feature = "antplus"),
        0x39 => cfg!(feature = "radiation"),
        0x72 => cfg!(feature = "engineering"),
        _ => false,
    }
}
fn window_id(id: u16) -> bool {
    id <= 25 || (32768..65535).contains(&id)
}
fn model_error(e: device_protocol::ProtocolError) -> Error {
    match e {
        device_protocol::ProtocolError::Capacity => Error::Capacity,
        _ => Error::Invalid,
    }
}
fn wellness_error(e: device_wellness::Error) -> Error {
    match e {
        device_wellness::Error::Capacity => Error::Capacity,
        _ => Error::Invalid,
    }
}
fn motion_error(e: device_motion::MotionError) -> Error {
    match e {
        device_motion::MotionError::Capacity => Error::Capacity,
        _ => Error::Invalid,
    }
}

fn string_max(caps: &ProductCapabilities<'_>) -> usize {
    caps.limits
        .map_or(usize::from(u16::MAX), |l| usize::from(l.max_string_bytes))
}
