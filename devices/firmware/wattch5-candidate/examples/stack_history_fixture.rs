//! Host-only synthetic stack-bench image; never a sensor or firmware producer.
#[path = "../src/test_support/history.rs"]
mod medium;
use device_history_store::{
    AppendRecord,
    async_paged::{AsyncPagedHistoryStore, JobResult, JobToken},
    paged::LogConfig,
};
use device_storage::{incremental::IncrementalSnapshot, persistence::Layout};
use medium::{Clock, Control, Mem};
use std::fmt::Write as _;
use std::{error::Error, fs::OpenOptions, io::Write, task::Poll};
use wattch5_candidate::schema::StrictSchema;
type Store = AsyncPagedHistoryStore<Mem, Mem, StrictSchema, Clock, 6, 255, 474, 464>;
const DOMAINS: [u8; 6] = [0, 1, 2, 5, 6, 7];
fn check<T, E: core::fmt::Debug>(r: Result<T, E>) -> Result<T, Box<dyn Error>> {
    r.map_err(|e| format!("bench fixture: {e:?}").into())
}
fn settle(s: &Store, token: JobToken<'_>) -> Result<JobResult, Box<dyn Error>> {
    for _ in 0..2_000_000 {
        if let Poll::Ready(v) = check(s.result(token))? {
            return check(v);
        }
        check(s.step())?;
    }
    Err("fixture job stalled".into())
}
fn identity(s: &str) -> Result<[u8; 16], Box<dyn Error>> {
    if s.len() != 32 {
        return Err("namespace must be16bytes hex".into());
    }
    let mut b = [0; 16];
    for (i, v) in b.iter_mut().enumerate() {
        *v = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)?;
    }
    Ok(b)
}
fn open(image: &[u8], ns: [u8; 16]) -> Result<(Store, Control, Control), Box<dyn Error>> {
    if image.len() != 0x400000 {
        return Err("full4MiB required".into());
    }
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(0x185000, 256, 4096);
    meta.bytes
        .borrow_mut()
        .copy_from_slice(&image[0x200000..0x202000]);
    meta.durable.borrow_mut().clone_from(&meta.bytes.borrow());
    log.bytes
        .borrow_mut()
        .copy_from_slice(&image[0x202000..0x387000]);
    log.durable.borrow_mut().clone_from(&log.bytes.borrow());
    let store = check(Store::new_unrecovered(
        check(IncrementalSnapshot::new_unrecovered(
            meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        ))?,
        log.medium(),
        StrictSchema,
        Clock::new(),
        LogConfig {
            namespace: ns,
            log_start: 0x202000,
            slots_per_domain: 259,
        },
        300,
    ))?;
    if settle(&store, check(store.begin_recover())?)?
        != (JobResult::Recovered { initialized: true })
    {
        return Err("uninitialized fixture input".into());
    }
    Ok((store, meta, log))
}
fn body(
    domain: u8,
    id: u64,
    out: &mut [u8; 474],
) -> Result<(u16, u16, u64, usize), Box<dyn Error>> {
    use device_motion::*;
    let (kind, schema, parent, n) = match domain {
        0 | 5 => (
            0x2301,
            1,
            0,
            check(
                ActivityRecord {
                    kind: if domain == 0 { 1 } else { 2 },
                    start_utc_ms: 0,
                    end_utc_ms: 0,
                    steps: u32::try_from(id)?,
                    distance_mm: 0,
                    energy_j: 0,
                    valid_mask: 4,
                }
                .encode(out),
            )?,
        ),
        1 => (
            0x0021,
            1,
            0,
            check(
                SleepInterval {
                    duration_ms: 60000,
                    stage: 1,
                    quality: 80,
                    flags: 0,
                }
                .encode(out),
            )?,
        ),
        2 => (
            0x0020,
            1,
            0,
            check(
                device_health::HeartRate {
                    bpm: Some(72),
                    quality: 95,
                }
                .encode(out),
            )?,
        ),
        7 => (
            0x2801,
            1,
            0,
            check(
                device_health::Temperature {
                    centi_c: Some(3650),
                    mode: device_health::TemperatureMode::Skin,
                }
                .encode(out),
            )?,
        ),
        6 if id % 2 == 1 => (
            0x2201,
            2,
            0,
            check(
                SportSummary {
                    session: 1,
                    start_utc_ms: 0,
                    end_utc_ms: 0,
                    duration_ms: 60000,
                    sport_type: 1,
                    environment: 0,
                    count_kind: 0,
                    count: 0,
                    distance_mm: 0,
                    energy_j: 0,
                    valid_mask: 4,
                }
                .encode(out),
            )?,
        ),
        6 => (
            0x2208,
            2,
            1,
            check(
                SportGpsSample {
                    session: 1,
                    index: u32::try_from(id)?,
                    time_offset_ms: 0,
                    lat_e7: 0,
                    lon_e7: 0,
                    valid: 0,
                    point_kind: 1,
                    steps: 0,
                    valid_mask: 0,
                }
                .encode(out),
            )?,
        ),
        _ => return Err("unsupported fixture domain".into()),
    };
    Ok((kind, schema, parent, n))
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: stack_history_fixture INPUT OUTPUT NAMESPACE".into());
    }
    let original = std::fs::read(&args[0])?;
    let ns = identity(&args[2])?;
    let (s, meta, log) = open(&original, ns)?;
    if check(s.stats())?.retained != [0; 6] {
        return Err("input must have empty history".into());
    }
    let mut expected = String::new();
    for domain in DOMAINS {
        for id in 1..=255 {
            let mut bytes = [0; 474];
            let (kind, schema, parent, n) = body(domain, id, &mut bytes)?;
            let result = settle(
                &s,
                check(s.begin_append(AppendRecord {
                    domain,
                    record_type: kind,
                    schema,
                    utc_ms: 0,
                    flags: 0,
                    parent_session: parent,
                    payload: &bytes[..n],
                }))?,
            )?;
            if result != (JobResult::Appended { id }) {
                return Err("wrong durable append ID".into());
            }
            let hex: String = bytes[..n]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            writeln!(
                &mut expected,
                "{{\"domain\":{domain},\"id\":{id},\"record_type\":{kind},\"schema\":{schema},\"parent\":{parent},\"body_hex\":\"{hex}\"}}"
            )?;
        }
    }
    drop(s);
    let mut image = original.clone();
    image[0x200000..0x202000].copy_from_slice(&meta.durable.borrow());
    image[0x202000..0x387000].copy_from_slice(&log.durable.borrow());
    let (recovered, _, _) = open(&image, ns)?;
    if check(recovered.stats())?.retained != [255; 6] {
        return Err("fresh owner lost fixture records".into());
    }
    if image[..0x200000] != original[..0x200000] || image[0x387000..] != original[0x387000..] {
        return Err("nonhistory bytes changed".into());
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[1])?
        .write_all(&image)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(format!("{}.records.jsonl", args[1]))?
        .write_all(expected.as_bytes())?;
    println!(
        "{{\"synthetic_bench_only\":true,\"sensor_measurements\":false,\"records_per_domain\":255,\"total_records\":1530,\"largest_selected_payload\":46,\"fresh_recovery\":true,\"nonhistory_bytes_unchanged\":true}}"
    );
    Ok(())
}
