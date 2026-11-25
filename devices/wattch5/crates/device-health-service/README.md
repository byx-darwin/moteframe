# Optional health wire service

Independent `no_std`, allocation-free workspace; unsafe code forbidden. `default=[]`. Features `heart-rate`, `blood-pressure`, `oxygen`, `glucose`, `temperature`, `ecg`, `wellbeing`, and `microcirculation` forward only their matching `device-health` codec feature. No production simulated backend, sensor driver, algorithm, default LATEST, storage implementation, or authorization authority is installed.

This crate implements executable `device_services::ServiceHandler` routing for modules20/25/26/27/28/29/2A/2B. Health module metadata is major1/minor0; body schema revisions and core wire1.2 are independent. It uses the V0.6 proposal's effective health-job/zone layouts and preserves frozen wire1.2 LATEST/HR bytes. New proposed numbers remain new design choices, not historical device behavior.

| Opcode | Request | Successful response, including common4B status |
| --- | --- | --- |
|01 STATUS|empty|actual module state:u8,reason:u16 =7B total|
|30 START|mode:u8,type:u16,context:u32,temp_mode:u8 =8B|job:u32 =8B total|
|31 STOP|job:u32 =4B|4B only; stop acceptance, not a valid measurement|
|32 QUERY|job:u32 =4B|state:u8,reason:u16 =7B total|
|02 LATEST|empty for single-type modules;2A type:u16|LATEST envelope15B+actual body+status4B|
|20.33 ZONES|context:u32, nonzero|mask+5×(threshold:u16,count:u32) =35B total|

ZONES/20.E82 are selected only when an explicit real `ZonePort` is supplied. No context or sample counter is synthesized. `ZoneSummary` encodes a complete actual five-zone snapshot with mask1F and five positive increasing bounds; it rejects partial/unknown tables. The real port must validate live context aliases and hold a fixed threshold snapshot. STATUS01 uses the required `HealthSource::module_run_status` port (HealthProduct extends HealthSource), with the frozen0unavailable/1ready/2acquiring/3fault dictionary. It never infers a ready device from empty JobManager slots. Generic property schemas remain with their respective owners.

## Shared owner and real ports

Create exactly one `SharedHealthOwner<B,N>` per boot, from the actual boot-global `JobManager<N>` and one real multiplexing `Backend`. The same owner arbitrates PPG/cuff/ECG/etc resources across every module and device-owned job. Do not construct one manager per module or reconnect. `with_manager` uses checked `RefCell` borrowing and returns Busy rather than panicking. It allows actual device-owned work and explicit cleanup recovery; do not manually disconnect the underlying manager while an open handler owns its events.

One `HealthHandler` handles all selected health entries for one physical connection. `new` requires a nonzero monotonically increasing connection identity, the exact immutable module entries, and these actual ports:

- `HealthAccess`: module and specific job/context/type authorization bound to this connection's trusted owner/station. It must reject old/foreign connection jobs. Permission is checked before mutation, immediately after product declaration/binding, at the actual physical Backend.start boundary after context/resource callbacks, after execution, on cached-response release, and on every event prepare/confirmation. The start boundary checks the same live authority for both module Start(None) and the allocated Start(Some(jobID)); denial prevents Backend.start and cannot return an8B success. Recorded active job START scopes are rechecked by `must_end_session`, including before an unrelated core PING. A denied new request remains an ordinary access-denied response; loss of an existing job scope permanently closes/cleans this handler.
- `HealthClock`: actual monotonic milliseconds; reversal or clock failure closes owned work. Peer timestamps never become the clock. The transport executor remains responsible for response/fragment deadlines.
- `HealthSource`: actual selected source/module runtime status; missing source or missing current status returns an explicit error, never an invented ready state.
- `HealthProduct`: immutable audited `ProductDeclaration`, actual `Provenance`, and trusted `(Owner,TriggerSource)` policy for START. Ordinary App starts choose Application/App. Device ownership requires independent product policy. Returned source must equal actual backend metadata; declarations must match the exact module/type. Temperature mode and product mood/algorithm fields use the existing typed body validators. Imports/derived estimates cannot be reclassified through the wire handler.
- `LatestStore`: actual committed snapshots and nonreusing nonzero generation; missing values return NotFound. Successful actual RESULT publication is atomic and idempotent by(job,sample). It is performed only after full body/declaration/carrier checks. Storage failure does not acknowledge the backend. `current` validates a cached LATEST generation before release. Publication stores an actual observation; it does not claim the phone received it.
- Optional `CuffPort` selected via `with_cuff`: real25.E82 pressure/battery retained until confirmed. `running_cuff_job` must bind the same actual physical cuff job; manager state must be Running after START-response admission. PPG BP estimates cannot produce inflation pressure. No CuffPort means no cuff sender. Pressure remains centi-mmHg; battery is cuff0..100 percent/255unknown, never watch battery.
- Optional `ZonePort`: actual live-context snapshots and immutable versioned events, with confirmation and freshness checks. No port means no zone route.

Only one App connection can own this boot-global health owner at a time. Closing the old handler frees its connection binding while retaining physical leases for failed/asynchronous App cleanup. A new connection cannot implicitly release or preempt those resources. Preserve the boot owner; explicit recovery uses its same manager/backend. Device-owned jobs continue across disconnect. Never drop an active handler without checking `disconnect`/`connection_closed`; cleanup failures are explicit and must be recovered through the owner.

Actual module entries are the trusted product's capability declaration, not a probe that proves hardware availability. Register entries only after selecting real ports and audited product declarations. `Backend::resources/start` still reject unsupported/unavailable requests rather than returning fake jobs.

## Response and event admission

Use this handler through the single owning `Session`/`HandlerTable` and `ControlConnection`; Session supplies identity/replay caching. The handler does not create a second request cache. `response_admitted` is wired to actual final response-fragment queue admission. START can physically accept/start work before its response, but only after the final live module and allocated-job Start checks adjacent to the actual backend call. Callback-time revocation is rejected before any physical start; the manager may burn an allocated ID without publishing a job. START returns the manager's actual accepted job but cannot emit RESULT/END until that callback confirms the START response. Exact Session retries do not call Backend.start again.

`response_ready` defers STOP while a prepared result for that job is awaiting real admission. The application executor can drain the immutable event and poll the same deferred command. A successful STOP gates END until the STOP response is actually admitted, then definite backend cleanup governs END. STOP failure never reports released resources. QUERY reports retained manager state; unknown/wrong-module/evicted IDs are NotFound.

`prepare_result`, `prepare_end`, `prepare_cuff`, and `prepare_zones` produce complete logical event bytes and an opaque `EventToken`. Frame using Kind::Event and token.module/opcode. Health events contain exactly their specified business body: no command status and no application CRC are appended. Tokens/buffers remain immutable on FIFO retry. Call `confirm_event` only after all fragments were admitted in order to the same actual FIFO; it acknowledges the real backend exactly once. A full FIFO must neither confirm nor discard. Before every physical queue attempt repeat prepare/scope validation under the adapter's serialized admission boundary. Returned bytes are not permission to send after revocation.

| Complete logical event | Exact length | Actual maximum body at512/128 |
| --- | --- | --- |
|E80 RESULT|15B LATEST+payload|497B /113B|
|E81 END|10B business|10B fixed|
|25.E82 CUFF|job4B+pressure4B+cuff battery1B|9B fixed|
|20.E82 ZONES|context4B+summary31B|35B fixed|

These limits use `device-health` ResultCarrier EventLatest15B and ResponseLatest19B overheads. CRC profile1 applies to objects/history/raw as explicitly specified, not these health events. All APIs take the actual negotiated outgoing logical maximum (128..512). No result is truncated or acknowledged on capacity failure. Oversized ALK results require a separately designed alternate carrier; this crate does not invent one.

E81 maps Completed to existing OK, Cancelled to existing InvalidState plus the actual module cancellation reason, and Failed to existing HardwareFailure plus its actual module reason. Its source is the actual trigger, distinct from result provenance. Process completion is not evidence a physiological result exists. Poll actual lifecycle using `poll_job`; drain retained results before END. Response/END confirmation does not prove peer reception or persistent save.

## Verification and remaining integration

From the repository root, run `python3 devices/wattch5/crates/device-health-service/verify.py`. `verification.json` records the exact default/all/eight single-feature Host tests, ARM checks, and strict clippy commands, plus source hashes and separate logs. Exact-body correction is recorded RED→GREEN in red-exact-event-bodies/red-exact-control-events and green-exact-event-bodies: the initial implementation incorrectly appended a history/raw CRC to health events; the final implementation removes it rather than inventing a new field. RED evidence: red-start.log (unsupported wire response4B vs required actual job8B), red-lifecycle.log (job revocation not visible before core activity), red-zones.log/red-provenance.log (expired zone context/source reclassification). Additional real RED→GREEN logs red-status/green-status and red-cuff/green-cuff prove actual STATUS routing and cuff response-first admission. Corresponding GREEN logs and final matrix are retained. Final-review regressions are retained in tests/start_authorization.rs: red-start-authorization.log proves product binding/resource/context revocation and allocated-job scope denial previously reached physical start; green-start-authorization.log proves zero physical starts for all four cases. The original independent review probes remain unchanged. ECG29.E80 preserves every field of2901/schema2 including full64-bit measurement and all process statistics in a LATEST envelope. Wellbeing2A.E80 preserves its selected2A01..05 type, schema1 unit/exponent/source/value; typed declarations run before storage or emission. Test backends are offline fixtures only under tests, never selected in production.

Remaining real integration: backend/hardware and physiological validity, actual durable LATEST/zone stores, immutable product/provider evidence, security/session ownership, event FIFO/GATT sender and its fragment deadlines, capability registration and product build composition. No device/Git action was performed; compilation and offline tests do not claim hardware readiness or clinical accuracy.
