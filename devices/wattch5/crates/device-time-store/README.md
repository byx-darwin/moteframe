# Durable UTC mapping

`device-time-store` is an optional `no_std`, allocation-free, unsafe-free owner of
one injected `SnapshotStore` and one trusted clock provider. It does not select a
medium, hardware clock, authorization identity, calibration, product default or
Flash address. Existing `SoftwareClock`, `TimeHandler` and wire fields are unchanged.

`TrustedClock::reading()` supplies `ClockReading { source_id, epoch, mono_us }`.
The nonzero source/epoch pair attests the same continuous calibrated timebase and
must never be reused after discontinuity, including power loss. A retained-clock
adapter may eventually provide this proof; this crate provides no hardware adapter.
Without actual continuity proof, old synchronization evidence cannot establish UTC.

`TimeStore::open(store, source, max_drift_ppm)` owns both inputs and loads strict
existing evidence without writing. Blank media has revision zero; a real GET
returns UTC zero, uncertainty zero, actual monotonic microseconds and `valid=false`.
The first authorized `synchronize(utc_ms, uncertainty_ms, access)` durably creates
revision one. UTC zero and negative UTC are valid synchronized values.

`read(access)` requires live read scope; synchronization requires live set scope.
`evidence()` and `revision()` are trusted internal diagnostics, with no wire endpoint.
`into_parts()` releases the actual store and source. `recover_and_reload()` explicitly
recovers the whole partition and rejects evidence older than the last confirmed
revision; old connection actors must be reconstructed after recovery.

Successful UTC requires an actual sample with the stored source/epoch, monotonic
time at or after the anchor, and no locally latched discontinuity. Source fault,
zero source/epoch, identity changes and backwards samples latch unavailable UTC.
A later plausible sample cannot restore validity; successful explicit durable
synchronization clears the latch. Temporary `Unavailable` errors do not latch.
The latest actual floor and latch survive in-owner recovery. Identity changes permit
a new monotonic coordinate. Trusted providers remain responsible for never reusing
a discontinuous source/epoch across owner reconstruction.

Progression reuses `SoftwareClock` checked arithmetic with the larger of the recorded
and current configured drift bounds, including drift ceiling and submillisecond
rounding uncertainty. Overflow returns an error. Zero drift is appropriate only for
an exact integration-attested source. No elapsed time or reset timestamp is guessed.

One checked `RefCell` borrow spans source, access and storage callbacks, including
handler authorization callbacks; recursive operations return `Busy`. This is a
single-context owner, not interrupt or multicore synchronization. Actual samples
remain observed even if later authorization or commit checks fail.

Synchronization stages a whole image and publishes only after exact durable
revision/length acknowledgement. Known pre-mutation `Capacity`, `Conflict` and
`RevisionExhausted` failures preserve confirmed evidence. Other failures or malformed
successful receipts poison reads/writes until recovery. Revocation after commit
suppresses the response and can leave complete new evidence; it cannot promise
rollback. Recovery preserves observed clock floor/latch and refuses revision rollback
or changed evidence at the same confirmed revision.

`DurableTimeHandler::new(metadata, &owner, access)` implements the existing
`ServiceHandler`: module01 GET01 has no input, SET02 has exactly twelve bytes
(`i64 UTC ms`, `u32 uncertainty ms`, little-endian), and success remains four status
bytes plus the existing twenty-one-byte mapping. Capacity, frame and metadata checks
precede mutation. Clock failure maps to hardware failure, storage to storage failure,
conflict to conflict, and integrity to integrity failure. Revocation wipes output.

The private exact 64-byte image is little-endian:

| Offset | Bytes | Value |
| --- | --- | --- |
| 0 | 8 | `DTIME001` versioned magic |
| 8 | 8 | Nonzero mirrored snapshot revision |
| 16 | 8 | Nonzero source identity |
| 24 | 8 | Nonzero continuity epoch |
| 32 | 8 | Actual anchor microseconds |
| 40 | 8 | Signed UTC milliseconds |
| 48 | 4 | Base uncertainty milliseconds |
| 52 | 4 | Anchor drift bound ppm |
| 56 | 8 | Reserved zero bytes |

The snapshot engine supplies CRC integrity and atomic replacement. These do not
authenticate data or protect against rollback across owner lifecycles. No migration,
automatic formatting or permissive decoding is provided.

Timezone property01.P1 belongs to `device-property-store` independently. No
cross-partition transaction or automatic product registration is implied.

Host tests use real `AtomicSnapshot` over a test-only whole-byte medium, exercising
restart, every commit sync barrier, byte tears, strict image/revision checks, clock
faults and arithmetic, authorization, recursive callbacks and actual module01 wire.
These establish the logical injected-medium contract, not physical power-loss,
Flash wear, RTC calibration, BLE or device behavior.
