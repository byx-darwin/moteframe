# Optional configuration schema registry

Independent `no_std` workspace; default features are empty. Production has no allocator, unsafe code, mutable store, algorithm, backend implementation, hardware action or GATT registration. V0.5's complete entries retain their exact value bytes. New allocations follow the V0.6 candidate; implementation is not evidence of individual product freeze.

`catalog()` records all original **62 property +15 object** ownership routes, including the single generic F000 entry. `coverage.json` lists every route and remaining external dependency. All features provide validators for52 original property routes (including generic F000) and9 object routes, plus proposed04.P000B and38.P0002. The10 remaining properties and6 objects belong to Content and require the explicit provider hook. Reserved40 has no route.

## Entry points

- `validate_property(module,key,bytes,&capabilities)` / `validate_object(...)` validate writes and return a descriptor. Read-only writes return `Readonly` before examining their payload.
- `validate_property_value(...)` / `validate_object_value(...)` validate complete readback, including real read-only fields. `decode_property` / `decode_object` return typed `Value`; encoding uses those typed variants, checks the selected schema again, and rejects mismatched variants. On any encode error discard the caller output.
- `property_descriptor` / `object_descriptor` describe a **compiled validator**, with wire shape, module/body revision, theoretical coding maximum and access. Absence is `Unknown`. No descriptor advertises backend availability. Optional-module body revisions do not change wire1.2.
- `SchemaProvider` and four `*_with_provider` entry points allow only `external-content` routes to delegate to a trusted concrete codec. Provider absence is `Unsupported`; provider descriptor module/key/revision and body maximum are checked. Built-in validation failures never fall through to a provider. Provider implementations must validate all fields and product constraints; the default methods reject.

`Invalid` means malformed length, enum, reserved bit, UTF-8/NUL, canonical field or numeric range. `Unavailable` means a required actual product declaration is missing. `Unsupported` means a valid selection is not supported, including enabled unencrypted09 telemetry. `Capacity` means caller output is too small. These are portable codec errors, not protocol status packets or successful business responses.

## Product declarations and limits

`ProductCapabilities::NONE` permits only format-level validation where the wire contract fully defines the value. It does not represent an operational product. Brightness levels, languages, display timeouts, health intervals/temperature modes, HID actions, ANT backend, automatic sport classifier, activity segment range, RMSSD dependency and engineering native ranges require explicit real facts when their schemas need them.

`limits: Some(ModuleLimits)` supplies the queried module's actual F000 snapshot for this call: each Str is bounded by `max_string_bytes`, objects by `max_object_bytes`, and zero `max_objects` rejects an object. Without this snapshot codecs enforce only the wire's u16 string/coding bound; services must supply actual limits before advertising or accepting operational product configuration. The registry cannot enforce a collection's object count or storage quota; the owning atomic store must do so.

Descriptors use **u64** coding maxima, including log count×entry combinations exceeding a32-bit address space. They do not allocate that amount or promise transfer availability. The real object's F000 maximum, staging capacity, property command envelope and negotiated outgoing frame size remain separate bounds. A large object requires the object channel. No codec truncates data to fit a frame.

`engineering=true` is a caller assertion that firmware already performed its profile/authorization guard and selected a real engineering backend. This crate does not authenticate it. Native count scales/ranges remain product-specific. Every diagnostic ID/reason must have a nonzero dictionary revision, exact parameter length and a product-supplied parameter validator; unknown parameter structures cannot pass by length alone.

## Concrete bodies and units

| Route | Typed value and exact body |
|---|---|
|01.P1|`Timezones`:i16 local UTC-offset minutes,i16 second offset,Str second city; frozen; no new validity field|
|02.P1|`Battery`:u8 percent,bool charging,bool complete,u32 seconds since observed charge exit,u8 mask0..3; absent fields0; percent≤100; valid elapsed requires known charging; known charging+valid elapsed requires seconds0|
|02.P2|`Identity`:u32 product,u16 subtype,u16 group,Str serial; frozen|
|02.P3|`Ui`:Str version,bool diff support,bool valid; frozen; validity does not canonicalize frozen text|
|02.P4|`Hardware`:Str hardware,u8 shape0..3,u16 logical width/height,Str API version|
|02.P5|Str name; frozen; empty is not implicit restore-default|
|02.P6|`Reset`:u16 reason0unknown(bit0clear)/1power-on/2software/3watchdog/4brownout/5external/6fault,u32 count,i64 UTC ms,u8 mask0..2; absent fields0|
|02.P7|`Address`:u8 type0..3,6 display-order octets,bool valid; invalid all0; no integer byte reversal|
|03.P1/P2|Delegated `device_models::UserProfile`14B / `ActivityGoals`18B|
|04.P1/P5|bool vibration / double tap; frozen|
|04.P2/P3|distance0metric/1imperial,temp0C/1F;hour0=24h/1=12h|
|04.P4/P8;05.P1;21.P1|Delegated frozen6B `TimeWindow`; bool,weekday bit7=0,minutes<1440; disabled preserves legal fields; weekday0 is wire-valid|
|04.P6|u32 main/other milliseconds; actual product range|
|04.P7|Str supported language tag; actual packaged-language list|
|04.P9/PA|u16 normalized brightness0..1000; actual strictly increasing discrete lists;0 means minimum usable brightness|
|04.PB (new)|`BrightnessLimits`:u8 saver count,u16 levels[],u8 normal count,u16 levels[]; strictly increasing each|
|04.O1|`WindowObject`:u16 semantic ID,u16 parent,bool visible,u16 sibling order; frozen7B; dictionary1 IDs0..25 and documented product IDs32768..65534; FFFF parent sentinel; actual registered ID/parent pair|
|05.O1..O6|Delegated `device_models::Reminder`; repeat/kind/work-window canonical rules stay in its owner; all title limits enforced|
|06.P1|bool enabled,u16 delay seconds; frozen3B; disabled does not clear the delay|
|09.P1|Exactly6 zero bytes; enabled1 is Unsupported pending an encrypted telemetry contract|
|11.P1|Frozen ByteString:u16 length +≤64 raw extension bytes, exact tail; intentionally opaque App-versioned marker, not a history ACK|
|20.P1|`HeartPolicy`:bool,u8 policy mode1timed/2continuous,u32 interval seconds,bool RMSSD; frozen7B; product interval/RMSSD dependency|
|20.P2;25.P2;26.P2|bool,u16 threshold, respectively bpm/mmHg/percent×100; oxygen≤10000; enable is not a validity mask|
|20.P3/22.P2|`Zones`:u16 sport0..23,u8 explicit type1,u16 resting/maximum,5 strictly increasing u16 bpm bounds;17B;1≤resting<maximum|
|21.P2|Delegated `device_motion::SleepDetection`18B, read-only real state/wear/timers|
|22.P1|Schema2,bool,u32 continuous recognized delay seconds,u16 sport0..23,u8 trigger1; selected real classifier/sport/delay bounds required for enable|
|23.P1|u32 segment seconds>0 within actual product range; no segmentation algorithm here|
|25.P3|bool,u16 reference systolic/diastolic mmHg; frozen5B; disabled retains reference, no hardware calibration authority|
|25.P00F0 (new)|Read-only actual BP algorithm/module version Str; nonempty and bounded by actual F000 string capacity. No firmware/host version fallback or automatic runtime registration.|
|26.P1|bool enabled,u32 seconds,6B window;11B; actual interval bounds; no synthetic scheduled measurements|
|27.P1|`Glucose`:mask bits0before/1after reference,2breakfast/3lunch/4dinner;u16 references mmol/L×100 and3 local-minute u16 values;11B; enabled references>0,minutes<1440,absent fields0|
|28.P1/P2|Schema2:bool,u32 seconds,u8 mode / bool,i16 centi-C,u8 mode;6B/4B;mode1object/2forehead/3body/4skin, actual selected modes|
|28.P3|bool,i16 centi-C reference; frozen3B; no added mode/source|
|2A.P1|bool alias of20.P1 RMSSD; enabling needs current HR policy+selected RMSSD backend assertion|
|2A.P2|`AlgorithmDirectory`:count + typed entries(type2A01..05,source1device/2phone,algorithm/version Str,unit,exponent,typed labels); unique(type,source), unique label values, nonempty identity; type-specific unit/exponent; no algorithm installed by parsing this directory|
|2A.P3|`WellbeingImport`:status1normal/2fetch failed/3network failed,3 i16 values,u8 mask0mood/1fatigue/2stress;8B; failures mask0; absent0;fatigue0..4,stress0..100,mood actual dictionary; always phone-import, no new source byte|
|2D.P1/2E.P1|Delegated `device_wellness::WomensConfig`10B / `UvPolicy`8B; no date/medical/UV algorithm duplicated|
|37.O1|`HidMappings`:bool,u8 platform1..3,u8 count≤8,{gesture0..7,action0..10}[]; unique gestures; actual product mappings; no OS authorization claim|
|38.P1/P2(new)|Schema2:bool enabled / read-only u8 actual state0..6; require real selected ANT backend; never fabricate connected|
|39.P1|u16 threshold1..1000 V/m; frozen2B; no radiation measurement|
|72.P1..P4|bool debug;bool logging,u8 policy0..3(disabled→0);u32 native amplitude,u32 minimum interval ms;u16 native wear threshold|
|72.O1|Schema1:`DiagnosticLog`:u16 count,entries{u64 mono_us,i64 UTC ms,bool time_valid,u16 ID,u16 reason,u16 parameter length,typed dictionary bytes,u8 end0..2,u8 error0..2}; invalid UTC0; product dictionary mandatory|
|*.PF000|Read-only frozen8B:u16 max objects,u32 max object bytes,u16 max string bytes; only compiled modules, no new wire module|

Variable directories expose typed iterators and typed writers. Their internal encoded views are private and can only be created by full field validation. Diagnostic batches require nondecreasing monotonic timestamps, no entries after a terminal marker, normal end/error0, and error end/nonzero error. These stream-coherence rules are explicit implementation choices for the proposed logging schema; they add no frozen fields.

## Remaining integration work

`coverage.json` identifies all16 external Content routes individually:07.P1..P5;08.P1;30.P1..P3/O1;31.P1/O1;32.O1;34.O1;35.O1;36.O1. No provider is installed by default. Root must connect the concrete Content APIs after their independent acceptance. SchemaProvider is not evidence these16 validators are finished here.

This crate does not persist aliases or policies, enforce cross-object hierarchy/quotas, schedule work, register hardware capability, produce read-only product/physiological data, select algorithms, execute HID/ANT, authorize engineering, or expose any GATT service. HR/RMSSD aliases and20/22 zone aliases require the owner's atomic state/version; `rmssd_allowed` must be evaluated from that same transaction. Firmware must bind schemas to real ports and recheck authentication/product scope. Encoding a DTO is not a successful setting operation.

Run `python3 verify.py`. `verification.json` and `verification.log` record the author's Host/ARM/default/all/each-feature snapshot. Tests are offline codec acceptance only; root arranges a fresh final review. Once delivered this crate is frozen pending that review.


R2 additional content route: `candidate_catalog()` separately lists `30.P0004` cross-platform source-app switches and `25.P00F0` read-only blood-pressure algorithm version. `catalog()` retains the original62 properties+15 objects. Only the explicit `external-content` provider hook may validate30.P0004; missing notification codec/provider fails closed.25.P00F0 remains a read-only compiled blood-pressure registry route. ContentSchemas enforces schema1, actual string/count limits and the whole-value property capacity. This addition does not retroactively expand the prior111-entry implementation map.
