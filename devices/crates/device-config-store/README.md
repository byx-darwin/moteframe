# Shared durable configuration

This `no_std`, heap-free, unsafe-free crate owns one injected
`device_storage::persistence::SnapshotStore`. It does not select a flash chip,
address, sensor, authorization policy or product profile. The old
`device-configuration` crate remains the codec/schema dependency.

`ConfigStore::open(store, capabilities)` validates the immutable trusted product
facts and loads the existing complete image. Confirmed empty storage creates an
uninitialized owner; `initialize(Defaults { heart, zones })` is an explicit
product decision and persists before making defaults visible. Invalid stored
schemas and uncertain storage failures never select defaults. `into_store()`
consumes the owner for shutdown/reopening without introducing a second owner.

For this owner, `capabilities.rmssd_allowed` declares trusted, selected RMSSD
backend availability. Do not freeze the initial HR-enabled state into that flag:
the owner checks the current and incoming HR dependency on every update. This
differs from a per-call codec capability snapshot that already combines both
facts. Replacing the backend requires a new owner lifecycle and fresh facts.

Select features independently:

| Features | Available adapters |
| --- | --- |
| default (empty), sport alone, wellbeing alone | None |
| heart-rate | 20.P1 |
| heart-rate + sport | 20.P1, 20.P3, 22.P2 |
| heart-rate + wellbeing | 20.P1, 2A.P1 |
| all three | All above |

`adapter(0x20, None)` binds the HR policy path. With both HR and sport,
`adapter(0x20, Some(sport_type))` also exposes HR zones, and
`adapter(0x22, Some(sport_type))` exposes the sport alias. The GET wire format has
no selector, so integration binds a selector in 0..23 before constructing the
property service. A SET carrying a different selector is rejected. Initialize
the configured zone selectors explicitly; absent selectors are NotFound.
`adapter(0x2a, None)` exposes the RMSSD boolean alias when selected.

Each adapter implements the existing `device_services::PropertyBackend` and is
usable directly with `PropertyService::new`. The catalogue is immutable.
20.P1/2A.P1 read and update the same policy revision; 20.P3/22.P2 read and update
the same sport-specific revision. Revision zero explicitly requests a blind
property SET, while storage commits always use the exact full-image revision.
Both property u32 and image u64 revisions advance by exactly one without wrapping.
A read remains valid after a property revision is exhausted.

The checked owner borrow encloses incoming schema/dependency validation, CAS,
live module and key authorization, full replacement persistence, and RAM
publication. Recursive callbacks receive Busy. Actual HR interval bounds and
`ProductCapabilities::rmssd_allowed` must come from trusted product integration;
RMSSD additionally requires the incoming/current HR policy to be enabled.
There is no authorization fallback or backend inferred from a feature name.

An Io, NeedsRecovery, Corrupt or InvalidLayout result poisons the owner, including reads. A failed final
barrier can leave a complete old or new durable version; it cannot produce a
success response. `recover_and_reload()` explicitly recovers the owned storage
and revalidates its complete image before restoring access. Recovery failure
leaves the owner unavailable. Board integration must provide any synchronization
needed across interrupts/cores: RefCell only serializes this execution context.

With HR+sport, `capture_sport(context_id, sport_type)` returns an immutable copy
of the full configuration revision, policy/zones revisions, context, resting and
maximum heart rates, and five thresholds. Later changes affect subsequent
captures. It does not manufacture sensor samples or run a zone algorithm.

## Persisted format

The deterministic image is bounded by `MAX_ENCODED_LENGTH` (523 bytes). The
snapshot storage layer supplies full-image CRC, atomic replacement and durable
revision. Configuration bytes use little endian fields:

- Eight-byte header: `DCFG`, format version 1, layout flags (bit0 HR, bit1
  HR+sport), zone count (0..24), reserved zero.
- HR when selected: nonzero u32 property revision and the existing seven-byte
  `HeartPolicy` codec payload.
- HR+sport when selected: zone records sorted by sport selector, each containing
  nonzero u32 property revision and the existing seventeen-byte `Zones` payload.

The count and exact total length must agree. Duplicates, trailing data, unknown
format/layout flags, incompatible build layouts, invalid policy dependencies,
and invalid product bounds are rejected. Wellbeing adds an alias and does not
change the persisted layout. No implicit migration or destructive reformat is
provided; firmware integrations must decide any future migration explicitly.

Offline tests cover shared aliases/CAS, selector isolation, frozen snapshots,
real property dispatch over AtomicSnapshot, durable failures/reopen, live denial,
recursion, output capacity, invalid schemas, and independent feature selections.
No physical device or flash evidence is claimed. Object/history/ACK/job/UV
ledgers and product algorithm/backend integration remain outside this crate.

## Source-level memory bounds

With HR+sport, the owner retains 24 optional zone entries and the HR policy.
Updates use copied candidate State values plus a 523-byte encoding buffer;
reload uses a 523-byte input buffer and a decoded State. The storage engine has
its own N-byte cache and two temporary N-byte recovery buffers, so nested call
frames must be considered together. These are source bounds, not a measured
peak stack/RAM claim. Firmware must choose N, inspect its actual build and
qualify the resulting stack budget before hardware integration.
