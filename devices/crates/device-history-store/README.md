# Durable bounded history and ACK adapter

`HistoryStore<S, V, C, D, R, B, I>` owns one injected `SnapshotStore`, one actual
`HistorySchema` validator, and one trusted `HistoryClock`. Its snapshot partition
must be exclusively owned and independent from the object repository. Runtime
code is `no_std`, heap-free and unsafe-free; it does not bind Flash, a radio,
sensors, a product feature, or a default business schema.

`initialize(snapshot, schema, clock, dataset, domains, lease_s)` requires confirmed
empty storage, an explicitly supplied nonzero dataset, sorted unique non-FF
nonzero-epoch domains, nonzero lease duration, and valid capacities.
`open(snapshot, schema, clock, lease_s)` strictly reopens existing data without
formatting. `append(AppendRecord)` is a trusted producer operation: the caller
supplies actual complete bytes, metadata and parent identity. Both append and
reopen require the injected validator to check the real body schema and parent
relationship. IDs are allocated with checked per-domain high-water increments.
No synthetic measurements, parent relations, dataset IDs or epochs are generated.

D bounds configured domains; R bounds total retained records; B bounds each body;
I bounds the complete serialized image. Image capacity can be reached before R.
Exactly one live lease is supported; a second OPEN returns Busy until close or
expiry. Bodies are never truncated, overwritten or evicted. ACKed records remain
retained; at capacity append returns Capacity. ACK does not erase anything. There
is no physical reclamation or epoch-reset API in this implementation.

## Persistent format

All integers are little endian. `DHIST001` is the exact eight-byte version marker.
The header is marker(8), dataset(16), domain count(u16), record count(u32).
Each of the sorted domain rows stores its canonical `HistoryDomain` bytes(21),
ID high water(u64) and cumulative ACK(u64), for 37 bytes. Each sorted
(domain, record ID) record stores domain(u8), epoch(u32), ID(u64), type(u16),
schema(u16), UTC(i64), flags(u16), parent(u64), body length(u16), WREC CRC(u32),
and exactly that many complete body bytes, for 41 bytes plus body length.

No padding or trailing data is allowed. Reopen validates bounds, nonzero identity,
reserved bits, timestamp rules, sorted uniqueness, domain membership and epochs,
watermarks (every nonzero ACK must name a retained record in its own domain),
exact directory oldest/newest ranges, each existing WREC CRC helper,
and actual injected schema/parent validation. High water may exceed the newest
retained ID and IDs may have gaps; admission uses actual stored IDs, never numeric
adjacency. The format does not authenticate data. Parent metadata and the complete
image are protected by the underlying atomic snapshot payload integrity check;
WREC intentionally follows the unchanged protocol helper's metadata coverage.

## Lease and ACK contract

OPEN freezes actual matching IDs, first/last bounds and count. Time selection
requires timestamp-valid bit0 and uses [start, end); type and parent filtering use
exact canonical filter fields. Plans follow ID order even when UTC runs backward.
Every operation binds the complete opaque lease, including caller-visible snapshot
and opening watermark. Unfiltered leases seed prefix coverage from the existing
durable watermark, so clients may resume directly after it without retransmitting
retained acknowledged records. Valid READ cursors may skip earlier records,
including in filtered read-only views. Final-fragment admission is tracked per
actual eligible ID in a bounded R-byte volatile table; filling earlier holes later
preserves already admitted later IDs. Only the contiguous actual prefix above the
durable watermark grants new ACK coverage. Duplicate admissions are idempotent;
a skipped unacknowledged record prevents ACK advancement, not successful reads.
Filtered leases start without this global ACK coverage and cannot ACK. COUNT
creates no admission evidence.

`HistoryClock` is authoritative. Every live lease operation, including read,
validation, admission and close, reads it. Arguments named `now_ms` must be no
later than actual clock time and must never regress independently of successive
actual clock readings. A nonregressed actual sample remains the clock floor even
when the supplied request timestamp is rejected. This permits one request time to remain stable
while the actual clock advances between operations. Deadlines derive from actual
time with checked addition. Expiry/regression clears retention and proof; close
can release an exact known lease even after expiry and is idempotent for that
exact most-recently released lease. Close never ACKs.

ACK checks live module, dataset and domain Read and Acknowledge scopes, exact
lease/filter/epoch, the complete canonical unfiltered request, actual endpoint
membership and the frozen upper bound, including idempotent paths. When the
endpoint is already durably acknowledged, a fresh authorized matching lease may
return the current watermark without volatile admission proof. This supports
lost-reply/restart retries without retransmitting acknowledged records, performs
no write, and reclaims no data. Only advancing ACKs require the complete actual
no-omission admitted prefix above the durable watermark; their success follows the
durable snapshot commit.
Close, cancel, disconnect, COUNT and queue admission do not advance the watermark.

## Lifecycle and integration limits

All operations and producer changes must be serialized through this owner. There
is no multiwriter coordination or independently shared mutable record table.
Live authorization and validator implementations must follow that same serialized
integration discipline; the owner rechecks live scopes immediately before ACK
commit but cannot make an independently concurrent permission system atomic.

`HistoryHandler` pins and compares whole directory rows, including oldest/newest.
Append updates the actual directory. Consequently producer appends require ending
and rebuilding existing handler sessions. The backend's frozen lease remains a
valid direct-store view until expiry, but it does not disguise changed directory
rows to keep old handlers alive. Product assembly must coordinate producer work
with connection actor lifecycles; this adapter does not provide that assembly.

SnapshotStore pre-mutation Capacity, Conflict and RevisionExhausted failures
preserve readable state, active leases and admission proof. Uncertain/corrupt
snapshot failures, failed recovery or strict reload, and malformed successful
snapshot metadata poison the owner and clear leases/proof. Reads and mutations
then fail until explicit `recover` succeeds. A failed commit can have durably
published the complete new state: recover and query rather than assuming rollback.
In-owner recovery rejects blank or older images below the last confirmed revision;
this detects a bounded backend contract violation, not authenticated rollback
protection after reopening or a hardware power-loss qualification. Handle counters
and clock history survive in-owner recovery. Reopening a new owner
starts a new lifecycle and requires rebuilding every old connection actor; old
leases must never be reused against a new owner.

## Verification and fault boundary

Integration tests use shared logical-media fixtures beneath real `AtomicSnapshot`,
including all three commit sync barriers, torn programmed bytes, restart,
malformed application images with valid outer snapshots, parent/schema/WREC
rejection, numeric ID gaps, forged and filtered ACKs, live authorization, clock
expiry/regression, capacity and malformed successful snapshot metadata.

These tests inherit the snapshot engine's whole-byte logical-media fault contract.
They do not qualify actual Flash bit-level power loss, wear, hardware timing,
BLE delivery or a concrete product. No hardware operations are performed.

## Optional append-only paged backend

`paged::PagedHistoryStore<S,L,V,C,D,R,B,N>` implements the unchanged synchronous
`RecordStore` API while scanning record bodies from `L: Medium`. The existing
`HistoryStore`, `DHIST001`, handler and wire formats are preserved. Here `R` is a
**per-domain** quota, bounded by 255; the original resident store's `R` is global.
For the six source domains choose `D=6,R=255` and the actual complete schema body
bound `B`. ACK retains every body and does not reclaim capacity. No clear, reset,
ID reservation, overwrite, erase-on-append or GC operation exists.

`PagedMedia::new` validates a physical `MediaLayout` and owns the medium once.
`split(&mut self)` grants nonclone metadata/log views with relative offsets zero;
`AtomicSnapshot` therefore uses `Layout { start: 0, bank_bytes }` on its view.
The log view rejects erasure. Both views serialize physical accesses using
`try_borrow_mut`; reentry returns an explicit error. `LogConfig` supplies a
nonzero namespace, the granted absolute `log_start`, and independent
`slots_per_domain >= R`. The caller must supply the same physical grant and
interpretation when reopening. This grants no Wattch 5 Flash partition.

`metadata_bytes::<D>()` is `80+64*D` (464 bytes for six domains), so `N=512`
suffices for those identities, cumulative ACKs and checkpoint rows. The existing
`AtomicSnapshot` owns its N-byte cache. The history owner keeps O(D) bounds,
counts/high/tails, a single lease and 32 admission bytes; it holds no retained-ID
array or full body image. A temporary complete B-byte record, 80-byte header and
at most 256-byte I/O scratch are used while scanning. Actual target frame and
resident measurements must be accounted separately.

Each domain occupies `slots_per_domain * slot_bytes::<B>(geometry)` contiguous
bytes; unused final arena padding must remain FF. The slot stride is
`align_up(80+B, write_size)+write_size`. Failed non-FF, noncommitted slots consume
finite spare slots in that domain; successful quota is not reduced when the
provisioned reserve suffices. `stats()` reports retained counts and consumed
prefixes separately. A failed attempt that settles entirely FF may reuse its
physical slot; only committed record IDs determine the next ID.

The separate new `HPGMET01` payload persists identity, namespace, geometry,
physical configuration, quota, ACK and per-domain checkpoint bounds/high/tail.
Its DSNAP001 outer container provides the existing two-bank atomic commit.
`HPGREC01` slots contain an 80-byte canonical header, complete body, FF padding
and an isolated all-A5 marker unit. The header contains dataset, domain, epoch,
ID, type, schema, UTC, flags, length, actual parent, unchanged WREC CRC, physical
slot ordinal and a CRC of the complete configuration. Slot CRC-32C covers the
`HPG1` separator, header bytes 0..72 and 76..80 and the complete body, including
parent and physical identity. CRC detects accidental corruption; it provides no
authentication or rollback protection across reconstructed owners.

Append validates before mutation, programs preparation in aligned <=256-byte
units, Syncs, programs only the marker, Syncs and validates complete readback
before returning an ID. Any uncertain mutation/readback poisons the owner and
revokes volatile proof. Recovery first Syncs, recovers metadata and scans every
physical slot and padding without formatting or erasing. Exact-marker records
must pass both CRCs, canonical metadata, ordering and actual schema/parent checks.
Nonexact markers never produce records. Confirmed identity, high/count/ACK/tail,
revision, metadata-root and trusted-time/handle floors survive failed recovery;
recovery never resurrects a lease. Explicit initialization requires an empty
snapshot result and completely FF log. A caller may explicitly retry a torn first
metadata initialization; ordinary open never initializes unknown or blank media.

OPEN freezes the actual domain physical boundary; later appends are excluded.
Each admission bit refers to a scanned eligible ordinal, including ID gaps.
Only an authorized canonical unfiltered actual admitted prefix can advance ACK.
Filters, arbitrary legal plan cursors, time/expiry, forged leases, close and live
scope rechecks retain the existing service behavior.

This is a synchronous logical-Medium implementation. Bounded individual I/O does
not bound a full scan's latency, and no polling loop adapts asynchronous NOR.
Real async storage/worker integration, shared-bus scheduling, partition approval,
BLE timing, silicon power-cut/wear behavior and complete product fit remain
unestablished. See `specs/wattch5-paged-history.md` for the offline contract.
