# Exclusive asynchronous partitions

`AsyncPartitions<M, K>` owns one `AsyncMedium` and validates all ranges before
I/O. Geometry must have nonzero capacity/erase/write sizes, a write unit at
most 256 bytes, erase units divisible by write units, and complete erase units
in the medium. Each nonempty range starts/ends on erase boundaries, fits with
checked arithmetic, and is disjoint from every other range. No assumptions
about physical NOR qualification or format validity follow from construction.

`new_unrecovered(medium, [Range { start, length, erase }, ...])` starts closed.
`split(&mut self)` issues exactly one `Coordinator` and a non-clone array of
`Partition` handles. The exclusive borrow prevents moving/replacing the broker
while any handle exists. A second split is rejected even after handles drop.
There is no API to extract, duplicate, or replace the underlying writer.

Each partition implements the existing `AsyncMedium` trait with fixed relative
geometry. Read length is 1..=256, Program is exactly one aligned write unit,
and Erase is one aligned erase unit requiring `Range.erase`. Checked translation
must fit the range before forwarding. Every accepted begin retains its owner
and expected completion through Pending. Other begins return Busy; wrong-owner
polls return Conflict, perform no I/O, and preserve ownership. A short read
output returns Capacity without polling or releasing the command. An uncertain
admission, terminal I/O error, or malformed completion closes the shared gate
and retains recovery responsibility. A Ready result ends physical command
ownership even when erroneous/malformed: recovery attempts a fresh Sync without
requiring terminal replay. A genuinely pending command (including uncertain
admission) still must be drained first. Invalid preflight has no physical effects.

Polls use private 256-byte scratch; caller output changes only for an exact
successful Read completion. Pending, error and malformed results never expose
scratch contents. Program bytes are copied by the underlying medium's existing
begin contract. No RefCell borrow survives a public call. Nested begin returns
Busy, nested poll returns Conflict, and nested coordinator access returns
Reentrant; no borrow panic or unsafe code is used.

Boot/recovery integration:

1. Call `Coordinator::recover_step()` until Ready. Every call performs at most
   one medium begin OR poll and at most 256-byte I/O. Recovery first closes
   access, drains an accepted/abandoned command, then completes a fresh Sync.
2. Recovery success opens Read and Sync only. Recover and validate every actual
   format owner in this mode; Program and Erase remain refused. Owners whose
   recovery requires writes need an explicitly designed separate policy.
3. Call `Coordinator::attest_normal()` only after all actual owners succeeded.
   This trusted assertion does not itself verify their formats/readiness.
   It fails while a command remains active or recovery is incomplete.
4. On shared uncertainty, repeat coordinator recovery and every owner recovery
   before attesting again. Dropping an active handle cannot clear its command;
   only its terminal poll or coordinator drain can end physical ownership, and
   recovery still requires a fresh Sync before reopening access.

The sole coordinator must stay alive for the broker's useful lifetime. Starting
coordinator recovery during normal operation intentionally takes over any
pending partition command; its abandoned result is not delivered afterward.
There is no formatting, range erasure, relocation, or physical-device access
implicit in recovery. Existing synchronous storage APIs and formats are intact.

`tests/partitions.rs` exercises a byte-array NOR `AsyncMedium` that copies input,
applies real one-to-zero program/erase effects, separates visible/durable bytes,
and injects latency, admission uncertainty, errors and malformed completions.
This is offline protocol/ownership evidence, not physical flash validation.
