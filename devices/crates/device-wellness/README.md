# device-wellness

Independent zero-dependency `no_std` workspace for proposed optional2C/2D/2E business schemas. Specification: `specs/protocol/v06-contract-proposal.json`, effective layouts/constraints and sharedRRI/UV contracts. Corewire1.2 and the91 complete earlier contracts are untouched. Default features are empty; select `rri`, `womens-health`, or `uv` explicitly. Compile selection is not actual backend availability or runtime registration.

No sensor,physiological algorithm,production simulation,BLE/GATT,authorization authority or durable store is installed. All actual backend implementations in this workspace are test fixtures under tests. Wire integers are explicitly little endian; oldFC/BCD layouts are not copied.

## Implemented concrete codecs

| Candidate entry | Typed body | Bytes | Implemented validation |
| --- | --- | --- | --- |
|2C01/schema1|RriSummary|16|nonzero fullu64 measurement;u32duration/count encoding|
|2C02/schema1 and2C.E80|RriEvent|25|fullmeasurement/index/cumulative-us;strictvalidity;present interval>0;absent interval canonical0;no LATEST wrapper|
|2D.P0001 and2D.02 config|WomensConfig|10|strictboolean;cycle/pregnancy exclusive parameters;1<=duration<=cycle;pregnancy unused duration/cycle0;disabled retains valid config|
|2E01/schema1|UvReading|7|maskbits0index/1intensity;reserved0;canonical absent0;no legacy index0..11 clamp|
|2E.P0001|UvPolicy|8|strictbooleans;report-enabled interval>=1;alarm/report independent|
|2E.C30 request|UvCancelRequest|4|nonzero original alarmID;successful response businessbody remains empty|
|2E.E80/module-schema2|UvReportEvent|21|nonzeroID,source1..3,explicitUTC/timevalid,actualreading mask|
|2E.E81/module-schema2|UvAlarmEvent|22|originalID,transition1..4,consistent cancellation origin,explicitUTC and actualreading|

Every physiological/config Debug implementation reports encoded length or static retention capacities only. It does not print values,dates,IDs,provider names or rawbytes. `None` in a typed field encodes explicitly absent/canonical0,not a backend result. `Some(0)` can be a real UV result. Unknown UTC is UTC0/timevalid0,not1970; valid epoch0 is supported.

`DayNumber` is signed local civil days since1970-01-01 in the configured timezone. Checked proleptic Gregorian conversion validates month/day and leap centuries;fulli32 day-number endpoints roundtrip. No arbitrary medical date window,gestational age,ovulation,prediction or disease algorithm is invented. Timezone changes do not rewrite an anchor civil-day number.

All fixed bodies are at most25B;the application must still use the actual carrier and negotiated direction bound,including status/record/CRC overhead,and fragment with the existing transport. No encoder truncates a real result.

## RRI lifecycle and actual port

`RriTracker` validates nonzero measurement and optional fullu64 parent sport;no32-bit truncation. `start_rri` additionally requires an available real `RriBackend`,actual provider/version and active-parent validation before calling its real start. Persistent no-reuse measurement IDs and actual acquisition provenance belong to the trusted product. A start failure must leave no newly acquired physical resources.

Events start index0 and consume one index per actual candidate,including rejected candidates. Valid interval count increments only for actual valid interval events. Cumulative microseconds strictly increase and cannot be ahead of the caller's actual monotonic clock. A supplied interval must exactly equal the delta from the previous trustworthy heartbeat;no interval is generated as60/bpm. Counters/index/duration fail before wrap.

The candidate wire's valid0 does not distinguish an initial trustworthy baseline from a rejected candidate. `RriObservation::class` is therefore **trusted internal backend metadata**,not an extra wire byte:Baseline establishes an actual heartbeat anchor,Rejected never advances it,Interval requires a measured positive delta. A consumer cannot reconstruct that quality distinction from schema1 wire bytes alone;untrusted-wire ingestion needing it requires a separately specified quality channel/schema. This is an explicit boundary,not a fabricated anchor or a claim of a complete incoming-wire handler.

One actual event remains pending until `confirm`/`confirm_rri` follows downstream admission. Exact retry is immutable and not recounted;changed retry or wrong ID/index fails. `poll_rri` returns no event for missing actual data;Unavailable is explicit. Actual backend data is retained until explicit acknowledgement;ack failure keeps it retryable.

Pause/finish reject undrained pending events. Pause/resume occurs only after the caller confirms the real backend transition;this pure tracker does not prove physical stop. Resume requires a new actual baseline rather than creating a cross-pause interval. Duration is monotonic start-to-end including pauses,not interval sums. Final summaries remain stable on later queries,including times beyond the liveu32-millisecond duration window.

RRI provider/origin metadata is out of band because the25B candidate has no source field. The product must keep its selected provider declaration immutable for the measurement and must not reclassify imported/derived data as sensor truth. `RriOrigin` distinguishes actual algorithm output and actual imported intervals;neither Debug nor wire serialization invents provider evidence.

## Female configuration storage boundary

`read_womens` and `write_womens` use `WomensBackend`. Missing real backend ->Unavailable;missing committed config ->NotFound. Fullvalidation precedes an atomic store;store failure must preserve previous committed configuration. No defaultenabled/anchor/duration is returned as fake stored state. The library stores no actual user configuration itself. Binding,consent,authorization,CAS versions and persistence are runtime/product responsibilities.

## UV bounded policy engine and actual port

`UvEngine<N>` is an offline bounded policy engine,not a complete registered command handler. `UvBackend::peek` supplies actual immutable samples and actual product/provider provenance;no actual sample remainsNotFound. Backend/provider declaration must stay fixed for the engine's lifecycle;hot provider reclassification and proof of physical accuracy are not supplied here. Wire `UvSource` is the event trigger,distinct from `UvValueOrigin` sensor/estimate/import metadata.

The engine compares only actual present UV index:>=threshold triggers once,belowthreshold clears. Missing index never clears an alarm by pretending0. Report `interval_s` limits E80 emissions,not sensor detection or alarm evaluation. Known cancelled cycles remain silent until an actual clearance and a new crossing. No medical hysteresis or sensing algorithm is invented.

Report and alarm slots retain immutable events under backpressure. A new actual sample cannot overwrite pending events. `confirm_report`/`confirm_alarm` follows actual queue admission;`confirm_sample` releases backend data only after generated events are admitted. Actual values are never truncated. Cached latest is a real latest sample,not durable history and not a freshness guarantee.

Cancellation uses the original alarm ID. `request_cancel` returns newly-applied versus already-known cancellation;unknown IDs fail. Cancel E81 remains hidden until `confirm_cancel_response` follows actual C30 response admission. Duplicate cancellation does not create another event or new ID. Every cancellation/clear transition acknowledges the exact(ID,transition),so an old acknowledgement cannot clear a different transition.

Conservative limits,explicitly not hidden production completeness claims:

- There are two event slots and N cancellation tombstones. No cancelled ID is silently evicted or reused. Full retention returnsCapacity before starting another alarm cycle;the product must size/persist a real dedup ledger before deployment. Do not reconstruct this engine mid-boot and lose IDs/tombstones.
- Policy mutation during pending events isBusy. Changing alarm enable/threshold during an active cycle isBusy instead of inventing an unspecified migration/clear event. A production policy-migration design remains separate.
- Configuration here is RAM policy only;2E property CAS/persistent atomic commit is not implemented. UV source declaration validation is structural metadata,not proof of sensor calibration or hardware truth.

## Remaining handler/product integration by entry

Concrete record/property/event bodies and the bounded state/port helpers above are implemented. These are **not** complete generic-property or externally routed handlers:

-2C.STATUS/2C.LATEST routing,permissions,stable dataset allocation,persistent summaries/records and untrusted baseline-quality ingestion are not implemented. Actual RRI acquisition,physical pause/stop and provider evidence remain product work.
-2D.STATUS and exact configuration response routing,consent/profile/CAS and physical durable storage remain product work;the10B body and atomic-port contract do not replace those.
-2E.STATUS/LATEST wrapper,routed C30/common status,credits,authorization,GATT/CRC,policy persistence,capacity advertisement/cancel-ledger persistence and hot policy/provider migration remain product work. The codecs and `UvEngine` do not advertise these as implemented.

No production backend means no usable advertised capability. Authentication/scope and product/source declarations must be checked before calling mutable ports. No disease/ovulation/numerical sampling algorithm or hardware action is performed.

## Verification

See `verification.json` for fresh commands/results/source hashes. Run tests with default,eachfeature andallfeatures;Host+ARM checks/clippy and fmt are independent of the root workspace. The health crate and its prior verification snapshots are not edited. Final independent review remains the coordinator's gate.
