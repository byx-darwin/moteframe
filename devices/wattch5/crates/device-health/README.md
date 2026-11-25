# device-health

Independent `no_std`, zero-dependency workspace for optional offline health service contracts. It contains no sensor driver, physiological algorithm, BLE access, storage, authorization source, or production simulated backend. Core wire1.2 and the91 complete V0.5 contracts remain unchanged. New module/schema/type/enum decisions follow the source-resolution proposal; they are proposed design choices, not individually approved user freezes.

`default=[]`. Selecting a feature permits its typed module/type to be constructed; it does **not** advertise an available backend. Product registration must require the selected feature, a real supported backend and the runtime's authorization/capability policy.

| Feature | Module | Proposed result types |
| --- | --- | --- |
| heart-rate | 20 | 0020 |
| blood-pressure | 25 | 2501 |
| oxygen | 26 | 2601 |
| glucose | 27 | 2701 |
| temperature | 28 | 2801 |
| ecg | 29 | 2901 |
| wellbeing | 2A | 2A01..2A05 |
| microcirculation | 2B | 2B01 |

## Interfaces and admission order

- `HealthStart::decode(module, body)` validates exactly8B, selected module/type, single1/continuous2, non-temperature temp_mode0, temperature modes1..4, and context0 outside HR. `Backend::resources` must additionally reject physically unsupported modes/types; a nonzero HR context must pass the backend's live alias validation. `JobId::{decode,encode}` handles exact4B STOP/QUERY parameters and rejects0/trailing bytes.
- `JobManager<N>::new(first_id)` creates static slots and a nonzero boot-local monotonic sequence. The product owns one such sequence per boot. Preserve `next_id()` before reconstructing; never reset it on reconnect. `None` means exhausted, not permission to restart at1.
- `start(request, owner, source, backend)` checks context, actual availability and product-global resource conflicts before accepting. Backend `Unavailable`/`Unsupported` yields an error and no successful job. A backend start failure burns an attempted ID, never reuses it, and must have rolled back physical resources.
- Queue the START response with the returned job. Call `confirm_start_response` **only after actual transport admission**. Result/end APIs refuse premature emission. The same queue's FIFO ordering must carry response before events; this service cannot prove over-the-air delivery.
- `fetch_result` borrows caller storage and returns only actual backend bytes plus metadata. `None` is no result, never a zero successful value. The backend retains immutable sample identity, metadata and bytes until `confirm_result` follows actual event admission. Retry is allowed; duplicate acknowledgements do not consume/count a sample twice.
- `stop` is idempotent for retained terminal or already-stopping jobs. Unknown/wrong-module/evicted IDs return `NotFound`. Asynchronous stop and stop failure keep the physical resource lease until `poll_completion` confirms definite cleanup. Failed completion polling is not proof of cleanup.
- `pending_end` peeks one immutable terminal event, repeatable on a full FIFO. `confirm_end_event` confirms its admission; thereafter no end is regenerated. A retained terminal slot can be reused only after this admission or muted disconnect cleanup. Completed process state does not prove a valid physiological result exists.
- `disconnect` immediately mutes all application-owned events and tries every active application job even after an error. Device-owned jobs continue. Failed/asynchronous App cleanup retains resource ownership for explicit recovery; stale response acknowledgement cannot unmute it. Authentication and ownership-to-session binding remain the runtime's job.

`ResourceSet` bits share one product-global allocation across all backend modules; they are not wire register addresses. Separate backend objects must use the same resource namespace and one shared manager for conflicting hardware.

## LATEST and provenance

`Latest` encodes only `record_type:u16,schema:u16,utc_ms:i64,time_valid:u8,payload_length:u16,payload`, in little endian, with exact lengths and total business-envelope limit512B. Command responses must separately reserve common status4B; caller transport must enforce its negotiated full message limit and any required CRC. Event envelopes never prepend command status.

`Timestamp::unknown()` is UTC0/time_valid0; it is not a1970 record. Valid epoch0 is supported. The standalone `Latest` type remains a pure envelope. `JobManager::fetch_result` now dispatches to `validate_result_body` before exposing a result: only the selected concrete body/schema is accepted, with exact lengths, canonical absence, enums, reserved masks, and embedded-source consistency. Unknown schema is explicitly `Unsupported`.

Concrete typed body codecs, each compiled only by its corresponding feature:

| Type | Record/schema | Bytes | Constraints |
| --- | --- | --- | --- |
| HeartRate |0020/1|4|bpm presence ->flagsbit0;reservedflags0;quality0..100 or255unknown;frozen bytes unchanged|
| BloodPressure |2501/1|8|maskbits0systolic/1diastolic/2pulse;independent Option fields;absent value0|
| Oxygen |2601/1|6|maskbits0SpO2/1PI;percent*100 range0..10000;absent0|
| Glucose |2701/1|3|mmol/L*100 and strict validityboolean;absent0|
| Temperature |2801/1|4|signedcenti-C;mode1object/2forehead/3body/4skin;strict validity;absent0|
| Ecg |2901/2|21|nonzero measurement;slot0..254;maskbits0ECG/1PPG/2avg/3min/4max;canonical absence;consistent same-window min/avg/max|
| Wellbeing |2A01..05/1|8|typed independent fatigue/stress/mood/RMSSD/MET unit,source,scale and declared score domains|
| FatigueSnapshot |2A01/2|12|original8B fatigue plus independently valid positive resting BPM and trigger0..5; one actual snapshot/provenance; schema1 remains readable|
| Alk |2B01/2|10..497|finite positive1/s when present;device source;actual algorithmID/version;strict UTF-8Str and exact lengths;canonical absentpositivezero|

Every body Debug implementation hides physiological fields. `None` means an explicitly absent/failed field and encodes its required canonicalzero; it never creates a backend sample or a successful measurement. Some numeric0 can be a real encoded value; codec field presence is not proof of clinical accuracy. No unprovided clinical thresholds are invented for HR/BP/glucose/signedtemperature encoding domains.

`ResultBody` is sealed metadata for these concrete schemas. `ProductDeclaration` and `Provenance` separately validate actual algorithm/providerID+version, source, and `AcquisitionKind::{SensorMeasurement,DerivedEstimate,PhoneImport}`. A legacy HR-derived estimate can match a declaration as an estimate but cannot pass `validate_sensor_measurement`; changing only reported provenance toSensorMeasurement fails the trusted declaration match. A present body value is also required for that gate. Declarations are trusted audited product configuration, not peer assertions; these checks cannot establish hardware truth by themselves.

Temperature declarations explicitly list supported low4 mode bits, so a body-only backend cannot silently advertise object/forehead/skin. Mood encoding is a signed product enum, not an invented universal scale: complete validation requires the declaration's unique-value,nonempty-label dictionary (at most255labels,matching the u8 metadata count). ALK's embedded algorithmID/version and any embedded source must match actual declaration/provenance.

### Actual outbound carrier bounds

A body codec's coding ceiling is not permission to send it on every carrier. Use `ResultCarrier::checked_length` or `ResultEvent::check_carrier`/`Latest::check_carrier` with the **actual negotiated outbound logical maximum** before admitting the message and calling `confirm_result`. Capacity failure retains the real result for an explicitly designed alternative; never truncate it or acknowledge an unsent result.

| Carrier | Overhead around body | Max body at512B | Max body at128B |
| --- | --- | --- | --- |
| EVENT LATEST |15B|497B|113B|
| Successful RESP LATEST |4B status+15B|493B|109B|
| History RECORD |38B|474B|90B|

A497B ALK body can therefore fit a512B EVENT LATEST while failing the response and history carriers. History uses its record envelope,not an additional nested LATEST header. The final actual encoder must additionally count any extra business header/CRC needed by its chosen message. This crate currently has no health GATT registration or automatic alternative object-channel design; future adapters must reject an oversized result or use an explicitly specified alternate carrier.

`ResultSource::DeviceAlgorithm/PhoneImport` is actual provenance, distinct from `TriggerSource::App/DeviceUser/Timer/Sport`. Provenance is metadata here, not an extra byte inserted into frozen health payloads. The C paths deriving SpO2/BP from HR or importing glucose do not become true sensor measurements through this service.

## Five heart zones

The `heart-rate` feature exports `ZoneThresholds` and `ZoneCounter`. Five positive strictly increasing inclusive upper bounds define `(0,T0],(T0,T1],...,(T3,T4]`. Every actual nonzero monotonic sample ID is consumed once; retransmission with the same ID never increments twice. Invalid/zero/out-of-range samples do not increment. Counters are samples, not seconds. Overflow returns `Exhausted` without wrapping or advancing the sample cursor.

Thresholds are copied at context creation, so policy changes cannot rewrite an active context. The runtime owns live context alias validation and allocates a new context for another lifecycle. Summary is31B; proposed module-schema2 E82 is context:u32+summary=35B. The new context prefix is not retrofitted into any complete frozen endpoint.

## Additional candidate bodies

`Reason::from_legacy_cuff_error` explicitly converts only cuff appendix7 raw1..7 into module25 reasons0201..0207 (pressure rise7seconds/30mmHg, overpressure295mmHg, no pulse, interference, invalid result, low voltage, EEPROM). Other modules cannot finish jobs using cuff reasons. Query keeps3B and END keeps10B; the original section3.11 dictionary is separate. This defines no physical pressure detection/protection implementation.

`ThresholdEvent` is only a strict E83 body codec:22B for20/25/26,23B for28, without status, source byte or CRC. `validate_observation` binds actual typed target fields, timestamp and declaration/provenance; invalid/missing target fields return NotFound. Imports and estimates retain their classification and never establish independent sensor capability. This does not implement crossing decisions, episode/no-reuse policy, suppression, scheduler or an authorized event source; no E83 runtime registration is added.

## Verification and remaining integration

From the repository root, run `cargo test --manifest-path devices/wattch5/crates/device-health/Cargo.toml --all-features --locked` and the corresponding `--no-default-features --features NAME` commands. See `verification.json` for the exact feature/Host/ARM command matrix and observed results.

Remaining: real backends/algorithms and clinical validity, BLE/runtime routing and authentication, product-declaration gate wiring before sensor-measurement claims, module capability registration, actual event routing/framing (ordinary events have no extra CRC), durable history/LATEST storage, product-global resource mapping, actual product mood labels/algorithm versions/temperature mode support, hardware verification. Commercial raw/self-test feature guards remain the firmware's responsibility. No hardware action or universal functionality-complete claim is made.
