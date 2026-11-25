# Portable durable properties

`PropertyStore<S, C, N>` owns one injected `SnapshotStore`, trusted `PropertySchema`,
immutable product-selected catalogues, and one N-byte serialized RAM cache. It is
`no_std`, heap-free and forbids unsafe code. It has no medium, Flash layout,
platform ABI, application dependency, product default or permission provider.

Supply sorted unique `ModuleCatalogue` declarations and actual schema facts to
`open`. Confirmed blank storage stays `Uninitialized`; `initialize` requires
exactly one sorted complete `DefaultValue` for every selected property. Each
property and the image start at revision one. Initialization never formats an
existing or corrupt image. `adapter(module)` implements unchanged
`device_services::PropertyBackend`; separate module handles share one owner.

Each declaration must be a plain readable/writable/persistent property
(access 0x0b), with actual compiled schema identity and a maximum no larger than
its trusted validator's maximum and 512 bytes. Core/reserved modules, F000,
objects, known read-only metadata and live status/result routes are rejected.
Existing shared configuration aliases 20.P1,20.P3,22.P2,2A.P1 remain exclusively
owned by `device-config-store`. Timezone configuration 01.P1 belongs here;
UTC synchronization and continuity belong to `device-time-store` in a separate
logical partition. No cross-partition transaction is implied.

`PropertySchema` is an integration trust boundary: descriptors must be immutable
and validation must use complete actual schemas and product facts, never peer
metadata or length checks alone. Optional `CodecSchemas` calls
`device-configuration` and `device-content` directly. Bind sorted
`ModuleCapabilities` and, when needed, an actual content `Manifest` using
`with_content`. Find 06.P1 validates both its configuration shape and actual
content delay bounds. Optional leaf features forward only their actual
validators; default features pull neither codec crate. Empty `codecs` and
`content-codecs` builds also compile. The bridge rejects unknown/uncompiled
routes, including writable phone status/location and imported wellbeing results.
It does not register runtime availability or execute physical effects.

SET validates the complete value, stages a complete image and checks live module
and key permission at the serialized commit boundary. Nonzero expected revision
requires exact property CAS. Expected zero explicitly permits a blind write.
Counters use checked increments. RAM is published only after an exact durable
snapshot revision/length acknowledgement. Known pre-mutation Capacity, Conflict
and RevisionExhausted preserve confirmed state. Uncertain/corrupt outcomes or
malformed success acknowledgements block successful access across all handles.
Stored-value validation failure returns IntegrityFailure, clears caller output,
and also blocks the owner. All failed reads clear caller output.

`recover_and_reload` first blocks access, recovers the real store, then validates
the entire image again. Recovery cannot go below the last confirmed image
revision within that owner, including returning blank. Failures leave access
blocked. Rebuild connection actors after recovery and query stable revisions
before retrying uncertain writes. A new owner cannot prove an authenticated
anti-rollback floor. CRC is accidental corruption detection, not authentication.
Checked `RefCell` borrowing returns Busy on recursive access; this is one
execution context, not an ISR/multicore locking scheme.

The private little-endian image is exactly 24 + sum(14 + value_length) bytes:

| Offset | Content |
| --- | --- |
| 0..8 | DPROP001 magic and version |
| 8..16 | mirrored snapshot revision u64 |
| 16..18 | property count u16 |
| 18..24 | six zero reserved bytes |
| each record | module u8, access u8, key u16, schema u16, value_max u16, property_revision u32, value_length u16, complete value |

The record sequence must exactly match the immutable catalogue. Missing, extra,
reordered, duplicate, schema-drifted, zero-revision, truncated, reserved or trailing
data is rejected. Format-v1 also requires image_revision = 1 + sum(property_revision - 1):
initialization sets all counters to one, and every SET advances exactly one property
and the image once. Checked accumulation rejects impossible counter histories.
Every loaded value is checked with its actual codec. N bounds
the whole image, not each property's allocated buffer; a commit uses one temporary
N-byte staged image, so integration must budget stack separately.

The existing wire envelope remains unchanged: logical control frames max at 512
bytes; GET uses 12 response bytes before the value and SET uses 8 request bytes.
Thus a 512-byte stored value is not transportable as a complete property GET/SET.
Choose actual catalogue maxima to fit the negotiated transport (GET <= logical_max
minus 12, SET <= logical_max minus 8); no truncation or invented success occurs.

Offline tests cover the actual 28 wattch5 selected configuration routes with
explicit test-only defaults/manifests, unchanged PropertyService GET/SET replies,
restart, invalid values, live scope/revocation, recursion, capacity, catalogue
exclusions, schema/corruption, revision floors, malformed store acknowledgements,
and every sync/byte tear cut on initialization, a first update and reused bank.
The fake medium exists only under tests. These tests make no hardware, BLE,
physical power-loss, calibration, wear or runtime product availability claim.

## Incremental shared owner

`AsyncPropertyStore<M,C,N>` is an additive boot-owned path over
`device_storage::incremental::IncrementalSnapshot`. Supply an explicitly recovered,
idle engine. `open` validates the same DPROP001 bytes, selected catalogue and actual
schema. It never initializes blank or reformats corrupt storage.

`begin_initialize(defaults)` and `begin_set(module,key,expected,value,access)`
validate and copy a complete candidate into the engine without medium I/O.
`begin_set` checks live module/key authority while holding the serialized owner
borrow, immediately before engine admission. Successful admission is the accepted
commit boundary. Later disconnect, revocation or delivery detachment cannot cancel
that accepted work. Until exact final sync/readback and image validation succeed,
reads and revisions still expose the old confirmed image. Uncertainty poisons all
successful access until explicit `begin_recover`/`poll_recover` revalidates storage
and the owner's revision floor.

Drive `step()` independently of connection actors, including after a connection
ends. Each step calls at most one engine poll; each engine poll performs at most
one bounded medium admission or completion poll. `result(token)` inspects progress
without driving it. `take_result(token)` consumes one terminal domain result;
its outer error reports invalid/pending/recursively borrowed delivery state.
`MutationReceipt` has exact whole-image metadata and an optional property change
(initialization has none). Retained results reserve the one mutation slot until
consumed or detached. `detach(token)` records a borrow-free Cell marker, including
during schema callbacks; background/next admission releases detached terminal
delivery while retaining the durable confirmed image. Tokens are checked, private
and owner-local; do not carry them into another owner lifecycle.

The owner has no third persistent image array. Reads, validation and in-place
candidate construction use one N-byte local image. N and trusted schema CPU cost
remain integration budgets; selecting N=128 does not establish a universal stack
bound. The medium/engine and full call stack require separate accounting.

`AsyncPropertyHandler<...,N,R=128>` is persistent for one actual connection. Bind
its constructor to the effective logical and output limits used by Session;
`required_features` rejects a mismatched declared output maximum before readiness.
R holds the complete exact pending request, must cover the logical maximum and is
at most 512. `response_ready` validates/stages SET once, polls only its retained
token on repeats, and returns false until terminal durability. `dispatch` consumes
that result once and emits the unchanged eight-byte success or four-byte failure.
Session retains historical response replay. GET/DESCRIBE use the existing
PropertyService over a confirmed-read adapter whose synchronous `commit_atomic`
always fails with StorageFailure. There is no wire Pending or successful queued
write. `connection_closed`, scope loss and Drop detach response delivery while
accepted work continues through the stable owner. Never place this actor inside
a factory callback that reconstructs a temporary handler for each invocation.

Offline tests exercise the real Session and ControlConnection against the actual
incremental engine with a test-only pending NOR model. They verify confirmed
visibility, CAS/contention, exact response/replay, response capacity, FIFO retries,
disconnect/timeout/revocation, recursive Drop detachment, and explicit recovery.
