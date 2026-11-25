# Portable commercial job ledger

`device-job-store` implements the existing `DeviceJobLedger` for module 02
`DATA_CLEAR` and `FACTORY_RESET`, over an exclusively owned `SnapshotStore`.
It preserves the existing C40/C41 bytes and provides no physical clear/reset
executor, hardware adapter, module registration or successful default provider.

`JobStore<S, B, P, I, R = 32, N = 4096>` requires real trusted adapters for:

- `TrustedBoot`: a nonzero monotonic actual boot epoch, stable during that boot.
- `TrustedJobPolicy`: current support/revision and explicit authorization of the
  original captured policy revision for a stable principal and operation.
- `ResetIsolation`: actual clear/reset partition policy preserving the ledger.

`open` reads existing canonical v1 bytes or confirmed blank storage. It never
formats unknown bytes. Blank storage remains unavailable until explicit
`initialize` commits all `64 + 64*R` image bytes. That initial full image proves
the supplied snapshot supports the declared lifetime capacity. `R` must be
32 through 65535 and `N` must hold the complete image. Runtime code is `no_std`,
uses bounded arrays, has no heap or RTOS, and forbids unsafe code. Size these
arrays and the snapshot adapter for the actual target's memory and stack.

Every identity remains permanently retained: no ACK, eviction or deletion is
invented. Capacity full rejects new identities while exact retries and queries
remain usable. Deduplication uses `(stable principal, operation_id)` and binds
the complete commercial operation and schema/parameter bytes `[01, 00]`.
Different principals can use the same operation ID independently. Legacy
id-only execution lookup/poll returns `Conflict` when ambiguous; trusted runtime
callers use `execution_record_bound` / `DeviceJobsOwner::poll_bound`.

Acceptance and Running each require a durable snapshot before success. Physical
execution remains in the existing service's response-admission boundary.
Terminal outcomes come only from the real executor and are persisted before
its finalization callback. Completion after unbinding does not require current
App authorization or an enabled current operation policy. Actual boot and reset
isolation still apply. No storage recovery executes or polls physical work.

A newer actual boot durably changes all previous Accepted/Running records to
Interrupted with existing result `InvalidState` (0004) and reason
`JobBindingMismatch` (FF06), preserving the physical outcome as unknown.
Same-boot I/O recovery retains unresolved states. Uncertain commits or malformed
success receipts poison access until `recover_and_reload`; a new image may have
persisted despite an error. After dropping connection handlers, use
`DeviceJobsOwner::into_parts`, explicitly recover the ledger, then rebuild the
owner with the same actual executor. Query the durable identity before deciding
on further operator action.

The canonical fixed image, state flags, admission boots and aggregate revision
are covered by the [persistence tests](tests).
Recovery compares confirmed in-owner history and rejects rollback, identity loss
or replacement, terminal changes and regressing execution. The format/CRC does
not authenticate media or provide antirollback across owner recreation. The
injected medium still needs target-specific power-loss, wear, partition/reset
isolation and synchronization validation; host tests establish only the defined
logical byte/sync fault model.

Offline checks:

```sh
cargo test --offline --locked -p device-job-store
cargo clippy --offline --locked -p device-job-store --all-targets -- -D warnings
cargo clippy --offline --locked -p device-job-store --lib --target thumbv6m-none-eabi -- -D warnings
```
