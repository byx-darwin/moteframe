# Caller-attested incremental NOR

`writable::NorChip<S, C>` owns one awake `SpiDevice` and a prompt monotonic
millisecond source. Its sole non-clone `WritableNor` partition handle implements
`device_storage::incremental::AsyncMedium`. `ReadOnlyNor` remains unchanged.
There is no synchronous `Medium` implementation and no conversion of Pending
into success.

`Profile::validate` checks structure and conventional density/geometry only.
`Attestation` is an explicit trusted integration boundary: nonempty evidence
references do **not** independently prove hardware qualification. The caller
must establish the exact part, complete protection/locks/pins, operation and
transfer maxima, awake state, repeated partial-page rules, durable atomic-byte
fault model, and exclusive chip/partition ownership. Parts requiring additional
unimplemented status/lock checks cannot be bound. Observed `5e4016` establishes
only conventional4MiB density; no supplied board profile qualifies its writes.

The board's `storage::writable_storage_binding()` remains unavailable. Its CFG
banks are an unprovisioned new-product proposal, not a reassigned legacy region.
The original C firmware actively uses CFG and DATA. No open/recovery operation
formats, erases defaults, migrates data, unprotects status, resets the chip or
erases the chip/block. The optional writer must never coexist with an ungated
second reader of the same die, including the existing read-only type.

## Bounded command behavior

Admission performs no SPI I/O and copies one exact aligned logical write unit.
Every poll performs at most one finite transaction; each transaction releases CS
and the shared bus before return. Read/program chunks are <=256 bytes and erase
commands affect one4096-byte sector. SPI implementations must themselves have
finite transfer bounds; the Wattch5 HAL has configured finite per-byte polling,
which is not a measured elapsed-time/radio guarantee.

Program uses `05`, preflight `03` to reject0→1, `06`, checked `05` WEL/protection,
`02`, separate `05` busy polls, and exact `03` readback. Erase uses
`05/06/05/20`, separate busy polls, and sixteen256-byte FF readbacks. Reads use
idle/protection `05` and a staged `03`. Sync drains `05` and verifies `9F`
identity after idle/WEL-clear. `01`, `04`, `66/99`, chip/block erase and
suspend/resume are absent. An armed WEL after interrupted WREN may therefore
prevent recovery; the driver never silently clears it through an unqualified
command.

One private256-byte program array and one private256-byte read array enforce
input ownership and untouched external output on Pending/error. Read terminal
replay reuses the program array, so asset reads cannot overwrite its retained
result. Diagnostics expose only fault class/range/effect responsibility, never
payload bytes. Deadline overflow, clock regression, I/O, unexpected status or
readback disagreement shut the public gate and retain physical responsibility.
No uncertain array command is retried. An injected frozen clock cannot establish
elapsed time; continuous monotonic progress is an integration obligation.

Operation and physical-issue deadlines are inclusive and checked both before and
after each successful SPI transaction. Status/data are accepted only after the
post-transaction check; an issued deadline is installed before attempting the
array command. Finishing exactly at a deadline is valid, including a representable
`u64::MAX` bound; finishing later, regressing or wrapping the clock fails closed
before output/completion publication or recovery settlement. Deadline construction
uses checked arithmetic. Asset reads use the same whole-operation bound and
private output guard. An overdue Sync does not clear retained effects or enable
private recovery scan access.

Unit256 substantially reduces command count under the snapshot engine's
one-unit program API; it reserves256 bytes for the header and256 for the marker.
Unit1 has less padding but many partial-page writes and needs appropriate
silicon qualification. Geometry/layout are part of the existing DSNAP001 media
interpretation; reopen with the same values. Marker A5→zero retirement on bank
reuse still requires qualification even with unit256.

## Recovery and public assets

Create `IncrementalSnapshot::new_unrecovered(chip.claim_writer()?, layout)` with
partition-relative layout offsets. The chip is externally owned and outlives
the engine, so its recovery/asset methods remain available while the engine owns
the handle. Check current engine jobs are terminal before beginning a recovery.

1. `chip.begin_recovery()` returns a checked epoch and closes public assets.
   Admit engine/owner recovery before any further storage polling.
2. The engine's recovery Sync drains prior effects and checks idle, WEL,
   protection and identity. Its success enables only the partition handle's
   bounded bank reads. Program/Erase and public assets remain blocked.
3. Keep polling engine **and property owner** recovery until both report success.
   Engine corruption/failure leaves the gate shut; a fresh recovery epoch may be
   attempted. A permanently faulted SharedBus cannot be repaired by this worker.
4. Only then call `chip.attest_recovery_complete(epoch)`. This trusted integration
   attestation checks matching settled epoch, no admitted operation and no
   outstanding effect. It does not itself inspect banks or produce a durable
   settings receipt. Calling it merely after Sync violates its caller contract.

`read_asset` permits a bounded read wholly outside the declared settings
partition only while public idle. Same-die assets remain blocked during active,
armed, failed and recovering jobs. A separate-CS LCD may use SharedBus between
worker polls while NOR internally writes; a real bus/CS failure poisons the
shared bus and therefore LCD too. No recursive bus access or ISR handle is
permitted.

Accepted product commits continue after actor detach/revocation. Moving an
engine into its medium transfers its exact in-flight obligation; it does not
cancel it. Dropping the handle permanently consumes its writer claim and keeps
the chip's operation/gate/effect state. Do not drop a boot-owned active owner and
then construct a new apparently idle writer. Physical operations cannot be
undone by cancellation.

## Offline provisioning proposal

`python3 drivers/spi-nor/tools/provisioning_plan.py --capture <existing-original.bin>
--output <new-proposal.json>` reads a pinned original4MiB capture, verifies its
full and regional SHA256 values, validates complete disjoint aligned ranges,
and creates only a new local JSON file. It refuses changed originals and existing
output paths. No device interface exists. JSON remains explicitly unqualified,
unprovisioned and unavailable; original resources, DATA and full backup remain
preserved. Future actual provisioning requires fresh separate authorization and
qualification evidence. Run validator regression through
`make spi-nor-provisioning-check`, also a dependency of `make check`.

Offline model tests prove software protocol/ownership behavior only. Physical
power loss, timings, supply, endurance, actual same-die behavior and native-radio
coexistence remain unqualified.
