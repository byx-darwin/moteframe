# Incremental DSNAP001 persistence

`device_storage::incremental` adds `AsyncMedium` and
`IncrementalSnapshot<M, N>` without changing `Medium`, `SnapshotStore`,
`AtomicSnapshot`, or their format. The shared encoder, committed-header validator,
CRC and layout validator remain in `persistence` to prevent format drift.

Construct with `new_unrecovered`; run `begin_recover` and repeatedly call
`poll_recover` from the main loop. Construction and acceptance perform no medium
I/O. A poll performs at most one `begin` **or** one `poll` on the medium; it does
not drain immediately completed operations. CPU CRC work consumes at most 256
bytes per slice. Candidate admission and `load_committed` each copy up to N bytes;
choose N to match the application's admission and RAM budget.

`begin_commit(expected, bytes)` checks healthy/idle state, CAS revision, payload
capacity, revision overflow and token overflow, then copies the candidate into
owned storage. Keep the engine alive and poll its returned `CommitId` independently
of the requesting connection or UI actor. `Pending` never means durable success.
`load_committed` returns the old confirmed image throughout a healthy pending
commit. No candidate becomes visible until the final durability barrier and exact
marker/header/payload readback, with CRC validation, have completed.

`inspect_committed` performs the same health check and invokes a scoped read-only
callback with the exact confirmed `Option<Snapshot>` and payload slice, without
an N-byte copy or medium I/O. Confirmed blank storage passes `None` and an empty
slice. The callback never runs before successful recovery, after uncertainty,
or during recovery. Its slice cannot escape as part of the result, and it grants
no mutable access. Healthy pending commits still expose only the old confirmed
slot. This additive API leaves the copying `load_committed` contract unchanged.
Callers with their own schema/floor checks must still apply them on every read;
inspection alone establishes storage confirmation, not application validity.

`is_busy()` is a read-only, no-I/O query covering the whole accepted commit or
recovery, including CPU preparation and intervals between medium commands.
Retained terminal receipts/errors are idle. An idle engine can still require
recovery; `is_busy() == false` does not replace checking `load_committed`.
Composition code receiving an engine must reject an active job unless it already
owns the corresponding progress responsibility/token.

The commit ordering is: retire stale marker → Sync → erase individual units →
program padded header and payload units → Sync → program A5 marker → Sync →
exact readback → swap cache slot. Reads/programs are at most 256 bytes; each program
is one aligned geometry write unit, and each erase is one geometry erase unit.
Header fields, little-endian encoding, FF padding, CRC coverage and final marker
are byte-for-byte compatible with the synchronous engine, including non-power-of-
two write units.

Recovery starts with Sync to settle pending buffered effects. It validates both
banks using bounded marker/header/payload reads, including the lower revision
bank. An exact marker with invalid metadata/payload is corrupt even if the other
bank is valid; equal valid revisions are ambiguous. Only after both banks validate
does recovery reread the winner into the inactive slot, recheck CRC, and promote.
Confirmed blank is represented by `None` and does not format or write defaults.

Every admitted medium failure or malformed completion poisons access. A failed
final Sync or final readback can leave a complete new image on storage; the caller
still receives an error and must recover. Successful recovery may then confirm
that new image. Wrong/stale tokens and wrong poll kinds perform no I/O and do not
consume a result. A terminal success/error is replayable until the next
**successful admission** of commit or recovery; failed preflight preserves it.
Commit tokens are checked u64 counters local to an engine instance, and are never
reused within that lifetime. Do not transfer tokens between engine instances.

An `AsyncMedium` must own copied program bytes before admission returns, maintain
exclusive ownership until terminal completion, and return promptly without
spinning or invoking a blocking erase. It must not write read output on Pending
or error, must report the exact kind/length on completion, and must permit a new
recovery Sync after any terminal error. Busy admission means no effects and is
retried in a later engine slice. Io admission may have partial effects and poisons
the engine. Reads are stable under exclusive ownership; there is no post-readback
mutation, so no extra final barrier is required. The legacy README's atomic-byte
fault model and durability requirements apply. These are adapter contracts, not
proof that a physical device satisfies them.

`into_medium` transfers exclusive responsibility even if an operation is in
flight; it never cancels/settles the operation and never proves durability.

Storage consists of two N-byte slots, one 256-byte scratch and metadata, with no
heap, unsafe code or additional dependency. A physical adapter may need its own
additional owned program buffer. Host layout measurements and target integration
budgets belong in the delivery report; they are not aggregate stack guarantees.
