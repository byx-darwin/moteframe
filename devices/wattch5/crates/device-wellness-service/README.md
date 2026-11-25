# Wellness wire services

Independent `no_std` crate with fixed buffers, no heap or unsafe. Defaults are
empty; independent `rri`, `uv`, `womens-health` features forward only the selected
`device-wellness` models. Exact actual registrations use business version **1.0**,
availability 1, features 0, sorted unique module entries, maxima128..512. This
crate does not register product capabilities or install a production backend.

| Module | Implemented wire | Actual typed source |
| --- | --- | --- |
| 2C | 01 STATUS, 02 LATEST, E80 | Device-owned `RriTracker`, summary2C01/schema1, event2C02/schema1 |
| 2D | 01 STATUS only | Actual `WomensBackend` committed configuration and policy state |
| 2E | 01 STATUS, 02 LATEST, C30 CANCEL, E80/E81 | Actual `UvControlPort`, `UvState`/`UvEngine`, `UvReading` |

STATUS is real state0 unavailable /1 ready /2 active collection or policy /3 fault,
plus actual reason. RRI LATEST is status4+LATEST15+summary16=35B. UV LATEST is
status4+LATEST15+reading7=26B. LATEST timestamps come from the actual observed
source; an unknown timestamp stays unknown instead of becoming request/read time.
No actual source/result returns an explicit error. Actual UV index zero remains
valid when its presence bit is set; absent results never turn into successful
zero readings.

The root candidate decision resolves the ambiguous V06 2D interface proposal:
**2D.02 is not registered and returns UnsupportedOpcode**. This handler reads and
validates actual committed configuration before reporting a ready/active policy;
missing config is NotFound, and disabled config cannot claim active policy.
The configuration GET/SET P0001 path stays in the generic property service with
its actual typed storage/authorization backend. There is no prediction,
ovulation, inferred pregnancy status or synthetic LATEST measurement. Existing
91 frozen complete entries and the older model crate are unchanged.

## Actual ownership and ports

`WellnessOwner::new(backend)` owns the same ports used by device tasks.
`owner.with_backend(...)` is a short checked borrow for real task/recovery work.
`WellnessHandler::new(&owner, connection_identity, entries, &Access, &Clock)` binds
one App actor. Identities must be nonzero and increasing for this owner's lifetime.
Only one App actor exists; failed actual detach blocks replacement until retried.
The runtime must call `connection_closed()` on every disconnect, CCCD revocation
and error path, even if the Rust handler borrow is about to be dropped.

The required ports have no default successful implementation:

- `Backend` supplies actual selection, status, sources and App-only detach.
- `Access` separately checks actual module/action/concrete identity and source
  class. RRI algorithm/import and UV sensor/estimate/import are trusted product
  declarations, never claims from a phone payload or new wire flags.
- `Clock` supplies actual monotonic microseconds and independent UTC validity.
- `RriView` borrows an existing device-owned tracker with actual nonzero source
  generation, provider ID/version, origin and measurement timestamp. RRI IDs and
  parent sport IDs remain complete `u64` values. The handler never creates a
  tracker, calls `start_rri`, allocates measurement IDs, restarts acquisition,
  derives intervals as60/bpm or calls stop. Each preparation reads one consistent
  validated tracker view. Persistent measurement high-watermarks, real detector
  validation and acquisition lifecycle remain with the actual device task.
- `UvState` is a read-only facade implemented directly for `UvEngine<N>`; it reads
  actual latest/pending policy results without generating samples. The real
  `UvControlPort` must supply the same authoritative policy/control source,
  explicit provenance, pure cancellation preview and actual atomic cancellation.
- `WomensBackend` is the real committed configuration port; this handler never
  writes or invents a default configuration.

RRI `rri_event_admitted` acknowledges only this App's complete queue admission.
A real adapter must preserve device/history consumers and their independent
storage obligations; it must not prematurely drop a shared candidate. All backend
error/retry retention guarantees are explicit. App detach only removes App
leases/subscribers; it **never stops device-owned RRI/UV acquisition or silences
an alarm**. Device task algorithms, physical drivers, persistent storage and
recovery implementations are not supplied here.

## Response-first UV cancellation

1. Dispatch decodes exact4B nonzero alarm ID and calls **pure**
   `validate_cancel(id)`. It cannot silence anything, mutate the engine, reserve
   a physical operation or create a cancellation tombstone. Its `CancelTicket`
   binds actual source generation, real control-state revision and original ID.
2. The ticket is retained only inside this App actor. `replay_valid` rechecks its
   actual source/state before a queued or cached success is released. The C30
   successful response contains only the four-byte zero status prefix.
3. Only the exact original request identity/body and successful response passed
   to `response_admitted` can call `cancel_atomic(connection,ticket,clock,access)`.
   The actual port must atomically recheck ticket and live Cancel scope, perform
   actual silence, then update the same engine using `request_cancel` and
   `confirm_cancel_response`. A backend returning apparent success without the
   actual resulting known-cancelled state/event is a local contract failure.
4. The new E81 must reference the original ID, AppRequest source and AppCancel
   transition, with actual cancellation time/reading. Repeated admitted receipts
   and requests for genuinely retained known-cancelled IDs create no second E81.
5. Unadmitted disconnect/revocation simply discards the local ticket and detaches
   the App. It leaves the engine, actual alarm and tombstone retention untouched.

Pure preview and atomic effect are required **real-port contracts**. This crate
has no production memory adapter that pretends to implement them. State/scope can
change while a response waits; stale preflight is rejected before release where
possible. If the actual admitted effect fails or has an unknown outcome, the
hook returns a local failure and the App connection must end. No fake E81 or
second status reply is generated. Real uncertain-state recovery must remain
Unavailable until the physical/control layer has established the truth.

UV source generation prevents old-connection tickets and cached successes from
crossing a source reset, but cannot by itself prevent a numeric4B ID collision.
The stronger **product binding** therefore requires
`UvControlPort::identity_policy()` with no default. Only
`UvIdentityPolicy::PersistentNonReuse` admits a UV handler; `Unverified` returns
local `Unavailable` at construction. Subsequent withdrawal also ends the App
actor, even before its first STATUS/LATEST, and invalidates retained replies and
events. A boot-generation value cannot substitute for this guarantee.

The actual port must reserve persistent high-watermarks/ID ranges for **both**
report and alarm IDs, never reuse them across normal reboot, and stop before
exhaustion. The identity ledger is excluded from ordinary user-data clearing and
factory reset. These guarantees belong to the real storage/control implementation;
no production memory backend or pretend persistent adapter is installed here.
The optional wire only requires per-boot uniqueness; this stronger new product
binding changes no frozen bytes. Synthetic tests model a retained high-watermark,
use genuinely different post-reboot engine IDs, and reject the old numeric ID
with NotFound; real persistence and reset retention still require hardware-stage
verification.

## Ordered event integration

`event_route()` inspects typed source routing without encoding, pinning,
consuming, acknowledging or cancelling anything. Use it before the first queue
reservation. After actual reservation, `next_event(out)` prepares/retries a fixed
immutable body and `OfferedEvent` token. Check `event_valid(token)` before each
fragment; call `event_admitted(token)` only after the entire logical event was
actually admitted. The last completed token can be acknowledged repeatedly
without consuming another event. Actual source replacement or scope loss cannot
release stale bytes. Source errors/invalid tokens require the runtime to suppress
stale fragments and close/clean up as appropriate.

| Ordinary event | Exact body length |
| --- | --- |
| 2C.E80 | RRI event25B, full measurement/index/relative time/interval validity |
| 2E.E80 | UV report21B |
| 2E.E81 | UV alarm/cancel/clear22B |

There is **no RESP prefix, LATEST wrapper or extra CRC** on these events. The
runtime supplies nonreused transport message IDs, CCCD policy, fragmentation,
response-first queue ordering and deadlines. `EventPump`/application adapters are
not installed here; the pure-route API supports their first-reservation rule
without depending on `device-connection` or `device-application`.

New-design-choice (local integration policy, no new wire field): successful
LATEST/C30 enables its bounded source watch only when actual Subscribe+Event
scopes already exist; `subscribe(module)` explicitly borrows an existing source
without starting acquisition. Initial absence of event permission still permits
an independently authorized read/cancel. Revocation of retained read, event,
source-class or pending-cancel authority permanently ends this App actor and
clears prepared bytes. A monotonic clock regression also closes the App actor;
cleanup never invents a time or stops device-owned work.

Debug exposes lengths/routing only, excluding timestamps, readings, identities,
cancel tickets and provider text.

Verified 2026-10-29 with Rust1.96: all-features26 tests; rri-only10, uv-only20,
womens-health-only6; empty default exposes no handler and its compile-fail doc
test passes. For all five configurations, host Clippy all-targets with warnings
denied, thumbv6m-none-eabi build and strict Clippy, and format checks pass. Real
RED failures preceded the dispatcher, pure-route and single-view preparation
implementations; real RED also preceded rejection of unverified identity policy
and policy loss before the first read. Tests use synthetic typed ports only; host/ARM checks do not
verify hardware, sensor accuracy, physical cancellation, BLE delivery or durable
Flash behavior.

The final independent UV port-capacity regression is permanently copied into
`tests/service.rs`. `red-final-review.log` records its real assertion failure before
fixing dispatch; `green-resource-capacity.log` records the passing regression.
Actual business port Capacity after the response preflight now returns exact
LimitExceeded0007 `[7,0,0,0]`; it does not apply cancellation or create a success ticket.
A short caller output buffer still returns local SessionError::Capacity before pure
preflight or physical effects. The original independent review directory is unchanged.
From the repository root, run `python3 devices/wattch5/crates/device-wellness-service/verify.py` for the complete feature/Host/ARM/
strict-lint matrix; `verification.json` records commands, exits and source hashes.
