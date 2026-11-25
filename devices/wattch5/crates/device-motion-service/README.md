# Motion wire services

`no_std`, fixed buffers, no heap or unsafe. Default features are empty. `sport`,
`sleep`, `activity`, and `wear` compile only their corresponding handlers/models.
Registrations must match the supplied, sorted actual module snapshot exactly:
selected backend, business version **1.0**, availability 1, features 0, maxima
128..512. This crate does not register any product module.

Implemented commands (all complete logical requests; RESP includes the common
four-byte status):

| Module | Commands | Actual typed source |
| --- | --- | --- |
| 21 | 01 STATUS, 02 LATEST, 33 UTC-window sleep statistics | `SleepInterval`/`SleepProvisional`, `SleepSummary` |
| 22 | 01 STATUS, 02 LATEST, 30 START, 31 CONTROL, 32 SAVE, 34 CONTEXT | Shared `SportEngine`, real `SportBackend`, `SportSummary` |
| 23 | 01 STATUS, 02 LATEST, 30 CLEAR | `ActivityRecord`; backend atomic clear must recheck scope |
| 24 | 01 STATUS, 02 LATEST with mandatory 2401/2402 selector | `WearInterval`/`PostureInterval` |

There is no 22.C33. Module STATUS is state 0 unavailable / 1 ready / 2 collecting /
3 fault; CONTROL separately returns sport state 1 running / 2 paused / 3 stopped.
START retains the complete sport/environment configuration and full `u64` ID.
Repeated target controls and SAVE actions use the same engine and do not repeat
backend side effects. SAVE query reports actual confirmed writer state; missing
storage is an explicit failure. Caller capacity and complete request fields are
checked before physical/atomic operations. Every physical port callback rechecks
live trusted action scope; integrations must serialize revocation with actual
resource operations rather than use an asynchronous stale authorization cache.
Unknown backend outcomes remain uncertain in `SportEngine`; real recovery is an
integration responsibility, never an automatic reset or fabricated success.

`MotionOwner::new(engine, backend)` is device-owned. Its engine seed must come
from the integration's real boot-appropriate ID policy; it is not a persistent
multi-boot allocator. Device UI work uses `owner.with_sport(|sport| ...)`, sharing the same
engine/backend through a controlled `SportControl`. Its `start(type, environment, now)`,
`control(id, action, now)`, `session(id)`, `active_ms(id, now)` and `save_control(id, action)`
never expose a mutable engine/backend or `Deref` escape. App wire operations use that same
entry with the existing live action guard. `with_backend` is for actual observation/event
ports and inspection; all engine lifecycle transitions must use `SportControl` after
construction, including a device/UI STOP followed by START in one closure. A connection borrows this owner through
`MotionHandler::<B,N>::new(&owner, connection_identity, entries, access, clock)`.
Only one App owner exists; identities must be nonzero and increasing for this
owner's lifetime. `connection_closed()` clears aliases/subscriptions and calls
real `backend.detach(connection)`; it **never stops device-owned sport**. Failed
cleanup can be retried and blocks a replacement App owner until it succeeds.
The runtime must always call this hook on disconnect/CCCD revocation, including
error paths; dropping a Rust borrow is not a backend cleanup operation.

All sources are required ports: `MotionBackend`, `MotionAccess`, `MotionClock`.
There is no production memory backend, physical algorithm, Flash writer, clock,
BLE adapter or automatic zero-valued LATEST. Backends supply immutable nonzero
snapshot/event generations, actual observed timestamps, validated typed records,
selected support and actual runtime status. Invalid UTC must encode as zero,
never read time. `sleep_summary` must query a consistent real stored interval
view, intersect [start,end), clip reliable UTC endpoints and prorate actual
monotonic duration with floor rounding; unsupported stages clear mask bits.
No reliable UTC returns NotFound. `clear_activity` must atomically close the
previous actual interval, clear only scope 1 current-day or scope 2 current-flex,
recheck access at its serialization point, and return a typed post-clear snapshot.
It cannot erase history or alter a sport's counters. GET/SET properties remain
with the generic property service and actual typed property backends. Sport
series/GPS records use the history service; no separate unimplemented RPC is
claimed here. RRI/UV/female services are outside this crate.

Event transport API:

1. `next_event(out)` prepares or retries an immutable `OfferedEvent` body/token.
2. Check `event_valid(token)` before each real fragment admission.
3. Call `event_admitted(token)` only after the **entire** logical event has actually
   been admitted to the ordered control-event queue. This is not phone storage or
   history ACK. Repeated confirmation of the last completed token is harmless;
   wrong/stale tokens cannot advance another event.
4. After any local error or invalid token, the integration must suppress further
   stale fragments and close/clean up as appropriate. All queue fragments must use
   the same connection's nonreused transport message-ID owner.

START realtime waits for the exact START response admission. App STOP summaries
wait for the exact STOP response admission; wrong request/response bytes cannot
release either gate. New realtime while paused/stopped is discarded, never
replaced by synthetic zero values. A terminal event requires actual confirmed STOP evidence: the current engine's
terminal session or an immutable `SportSession` retained by the same controlled owner.
An unknown/replaced ID is never inferred stopped from a later session's state.
`SPORT_PROOF_CAPACITY` explicitly bounds retained proofs at32, matching the maximum
supported handler watch capacity N≤32. While an App is attached, each START first
reserves room for its future actual STOP proof; full retention returns Capacity before
physical start. Existing terminal proofs are never silently evicted. A real STOP remains
executable even when App storage is full: its current actual terminal state is kept by
the engine and a subsequent START cannot overwrite it without retaining that proof and
reserving the next one. Whole E81 admission releases the exact old proof and watch.
Successful App detach clears only App proof/subscription metadata; no App connection
means device work need not retain App-only proofs. Actual summaries/history remain the
real backend's responsibility. Ended or missing current C34 sessions release their alias
storage slots before authorization/allocation, while the alias counter remains consumed;
still-live aliases continue to require actual Context scope. Immutable started event bytes remain pinned on retry.
The handler does not implement transport fragmentation, CCCD policy, message-ID
allocation, queue reservation, deadlines or the new `device-connection::EventPump`
adapter; those must be provided by application integration.

Ordinary event lengths, without RESP prefix or extra WAP1 CRC:

| Event | Exact bytes |
| --- | --- |
| 21.E80 | LATEST15 + final7 / provisional8 = 22 / 23 |
| 22.E80 | SportRealtime26 |
| 22.E81 | end_reason2 + LATEST15 + SportSummary46 = 63 |
| 23.E80/E81 | LATEST15 + full ActivityRecord31 = 46 |
| 24.E80 | record_type2 + UTC8 + time_valid1 + actual value1 = 12 |
| 24.E81 | LATEST15 + closed wear/posture19 = 34 |

`LegacySportSummary` separately preserves frozen **0022/schema1**, exactly 20
bytes: duration/steps/distance/energy `u32`, sport type `u16`, mask `u16`. It
validates type 0..23, mask bits 0..3, canonical absent values and exact length.
`handler.legacy_sport()` reads only `LatestSelector::LegacySport` /
`Observation::LegacySport` returned by the real repository. It never converts a
2201/schema2 result or creates old records. Normal 22.02 remains 2201/schema2;
old phone frames are not accepted. Shared `device-motion` codecs validate the
other frozen records. Debug output omits all query/result/event payloads and
full-width identities.

New-design-choice (local integration policy, no new wire opcode/schema):

- Successful START/CONTROL/SAVE/LATEST/CLEAR arms a bounded connection watch for
  its concrete resource. Event authority is independent: initial absent Event
  scope permits the command while emitting no events; revocation of previously
  retained scope ends the App session and clears pending bytes/aliases. Terminal
  sport event admission frees its watch slot. Producers must keep generations
  unique per subscribed context and filter `pending_event` by the actual module,
  resource and connection principal.
- Increasing local `connection_identity: u64` prevents a detached App actor from
  reopening this owner. It is distinct from the protocol's informational session.
- Monotonic source failure/regression closes the App session rather than inventing
  a timestamp or implicitly stopping device-owned sport. Explicit backend cleanup
  failure remains retryable and prevents a new App owner.

Tests use synthetic typed ports only. Host/ARM builds validate portable dispatch,
codec and state contracts, not sensors, algorithms, durable Flash, BLE ordering,
actual battery/timer behavior or physical hardware acceptance.

Verified 2026-10-29 with Rust1.96: all-features43 service tests; sport-only32,
sleep-only5, activity-only5, wear-only6; empty default has no handler and its
compile-fail documentation test passes. For all six configurations, host
`cargo clippy --all-targets -- -D warnings`, `cargo build --target
thumbv6m-none-eabi`, ARM `cargo clippy --target thumbv6m-none-eabi -- -D warnings`,
and `cargo fmt --check` pass. Real RED failures were observed before implementing
the main dispatcher, legacy codec, concrete cached scope/event permission
separation, reconnect stop events, premature terminal events, capability-query
revocation boundary, Debug redaction and activity-current kind4 validation.

Final independent review regressions are permanently copied into `tests/service.rs`.
`red-final-review.log` records the original three assertion failures before fixes;
`green-final-review.log` records the full passing suite, with separate
`green-resource-capacity.log` and `green-context-reclamation.log` for those fixes.
The old E81 regression changes only the device/UI caller spelling to the controlled API;
the original review files remain unchanged. Additional tests verify device/UI stop+start,
old E81 then new-session realtime, full proof retention without physical START,
unblocked physical STOP, real E81 admission freeing capacity and device work after detach.
Business/static resource Capacity after the response preflight is wire LimitExceeded0007
with exactly4B status. A short real output buffer remains local SessionError::Capacity
before any action. Run `python3 crates/device-motion-service/verify.py` for the complete
feature/Host/ARM/strict-lint matrix; `verification.json` records commands, exits and hashes.

Wire C31 STOP also remains executable when N ordinary watches are full of older
terminal events. It preflights a separate bounded32-entry deferred STOP-watch table,
backed by the same owner reservations established before attached START. A confirmed
STOP returns its real5B Stopped response and retains its own subscription; it does not
perform work then disguise success as0007. Only after exact STOP-response admission
may its E81 run. Free ordinary watch slots promote deferred STOPs in nonreused session
ID order, including when an earlier static queue slot is reused. Both Control and
previously retained Event authority remain live requirements while deferred. Old A.E81
and new B.E81 cannot overwrite one another. The actual proof bound prevents new START
before more unconfirmed STOP metadata could be needed; physical stopping itself is
never rejected for App retention capacity.
`red-stop-full-watch.log` / `green-stop-full-watch.log` records the additional real
wire STOP regression. FIFO slot reuse and deferred-scope revocation have permanent
regressions in the full GREEN suite.

Deferred STOP events can also be offered directly from that bounded table using the
same opaque token and whole-event confirmation, so unrelated ongoing wear/activity
watches cannot permanently prevent a real stopped E81 from being sent. Older subscribed
sport terminal sessions keep priority. `red-deferred-stop-route.log` /
`green-deferred-stop-route.log` records the real regression for a full ordinary table of
unrelated observations; no ordinary source slot is overwritten or silently unsubscribed.

A consumed terminal session cannot create another event watch. New-MID repeat STOP
still returns its genuine idempotent5B Stopped result, while terminal SAVE query and
historical LATEST remain readable; none promises a second E81 or an empty FIFO blocker.
Only an actually active session or a retained, not-yet-admitted real STOP proof arms
those sport subscriptions. The permanent producer fixture now filters every event by
its actual module and concrete resource, rather than handing another session's event
to the first stale watch. `red-delivered-repeat-stop.log` and
`red-delivered-history-watch.log` show the independently reproduced failures;
`green-delivered-watch.log` records STOP/LATEST/SAVE regressions passing. Strict
producer filtering is used across the full suite and single-feature builds.
