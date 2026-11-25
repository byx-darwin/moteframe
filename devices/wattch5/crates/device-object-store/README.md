# Durable object repository

`DurableObjects<S, V, M, E, B, I>` owns exactly one injected `SnapshotStore`
and an actual injected `ObjectSchema`. It implements the existing `ObjectStore`
and `ObjectRepository` contracts without changing wire1.2 or selected firmware
features. No default medium, schema, resources, heap, unsafe code, or hardware
access is supplied.

Call `open(store, schema)` to load and validate an existing image. Confirmed
blank storage remains uninitialized; explicitly call `initialize(&[module_ids])`
with sorted unique nonzero IDs. Reinitialization fails. `recover()` invokes the
owned backend's recovery and reloads and validates its actual image. Rebuild
connection actors after recovery; an uncertain result does not promise rollback.
Use `revision()` or the stable object ID after recovery to discover the outcome.
In-owner recovery rejects blank or older images below its last confirmed revision,
keeping access blocked instead of allowing identity reuse. This is bounded defense
against an injected backend contract violation, not authenticated rollback
protection across owner recreation or proof about physical Flash.
`into_store()` releases ownership for shutdown or reopening.

`M` limits modules; `E` limits all lifetime creation reservations; `B` limits each
encoded object body (at most4096); `I` limits the encoded image and stack scratch.
The largest image uses `40 + 9*M + E*(75+B)` bytes. Actual images omit unoccupied
slots and nonexisting bodies. Reserve and commit can return Capacity even when
there are no live objects: permanent token/reservation/birth evidence is never
evicted. Deletion and Clear free no ledger slots. Skipped IDs remain consumed.
Clear requires live Clear authorization plus independently granted dangerous
scope and collection revision CAS; it retains every highwater and ledger entry.
Pending reservations remain bound to their original full Begin and cannot be
re-reserved. Abort/disconnect discard engine staging while preserving them.

Creation reservations persist before success, including module, kind, ID,
expected revision, complete length, create flag and nonzero namespace-global
token. Direct commits independently validate complete length, WOBJ CRC, actual
schema and live authorization, even when called without ObjectEngine. Creation
requires an exact pending reservation; edits require kind and revision CAS.
Original Birth fields survive edits, deletion and Clear in permanent evidence.
ID_STATE exposes Birth only for existing objects; unused and consumed states
return canonical zero Birth as required by the service. Collection revision starts
at1 and increments for every successful publication/delete/Clear; reservations
advance only namespace snapshot revision and their module's highwater. No revision
wrap is allowed.

Reads bind the expected object revision and copy actual available range bytes.
Enumeration sorts IDs, checks the entire returned page and first continuation ID
before copying metadata, and returns Capacity for an empty output with pending
entries. Access providers are trusted live sources, not permission snapshots.
The exclusive owner provides single execution-context serialization. It does not
provide ISR/multicore locking or a second concurrent writer to its partition;
integration supplies any synchronization beyond ordinary Rust ownership.

## Image v1

All integers are little endian. Header: `DOBJ0001`, u32 capacities M/E/B, u64
snapshot revision, u32 module count, u32 reservation count. Sorted module entries
contain u8 ID, u32 highwater and u32 collection revision. Reservations are sorted
by `(module, ID)` and retain full32-byte Begin,30-byte Birth, u8 existence flag,
u32 current revision/length/WOBJ CRC, followed by complete bytes only if existing.
The image ends with CRC32C computed with device-protocol's existing helper.
Nonexisting current revision/length/CRC are canonically zero. Original Birth is
zero only for never-committed reservations. The decoder rejects unsupported
format/capacities, trailing bytes, inconsistent counts/order/identities, duplicate
tokens, unsupported highwater, bad bounds/revisions, WOBJ integrity failures and
bodies rejected by the actual injected schema. The envelope revision must match
SnapshotStore metadata exactly. Recovery also checks the exact v1 operation
counter: snapshot revision equals1 plus the permanent reservation count plus the
sum of all `(collection revision - 1)`. Each module's collection revision must
cover1 plus all live current revisions, one creation for every committed
tombstone, and at least one removal when any committed tombstone exists. One
Clear can remove many objects. Pending never-committed reservations add no
collection mutations. All counter accumulation is checked; impossible revision
fixtures are corrupt even when their CRCs and envelope revision agree.

Persistence accepts success only with exactly the next snapshot revision and
encoded length. Unexpected metadata and uncertain/corrupt backend failures poison
the owner and block access until recovery and successful reload. Known backend
prevalidation Capacity, Conflict and RevisionExhausted preserve the current view.

## Verification and limits

Tests use real AtomicSnapshot over test-only whole-byte logical media, with tears
at each mutation operation and all three sync barriers. They cover durable Begin,
reopen, engine abort/disconnect, token reuse across modules, edit/read/delete/Clear
CAS, birth evidence, authorization, schema/CRC, bounded capacity and revision
exhaustion, semantic corrupt images, and unexpected backend metadata/recovery.
The decoder/backend-outcome fixtures are test-only; no memory backend is shipped.
This qualifies the inherited logical fault model, not bit-level Flash behavior.

The initial attempted RED stopped at workspace registration, and implementation
continued while the parent registered packages. Those setup/compiler failures
are not behavioral RED evidence. The later highwater corruption regression has
an observed assertion RED followed by a decoder fix and GREEN. Other initial
contract tests are regression evidence, not a claim of strict test-first history.

Focused offline locked tests, strict host all-target clippy, and strict ARM lib
clippy logs are under `target/object-history-persistence/objects`. Parent-owned
wire integration and whole-workspace acceptance are recorded separately.
