# device-motion

Portable no_std, zero-dependency protocol models and sport state. Default features are empty; `sport`, `sleep`, `activity`, `wear` are independent selections.

`SportEngine` confirms real backend transitions, keeps monotonic active duration separate from wallclock timestamps, handles idempotent pause/resume/stop, and separates detail-saving from sport state. Session IDs remain u64. `ContextAliases` creates connection-scoped u32 aliases and `resolve_active` rechecks session lifetime; it never truncates a session ID. A terminal session is retained until the next start. ID allocation is boot-local and is not a persistent multi-boot identity scheme.

`SportBackend` must provide actual capability/transition/detail-writer results. An uncertain transition blocks subsequent controls. There is no production fake backend. Disconnect does not implicitly stop a device-owned sport. Authorization and connection event routing belong to the service integration layer and are not supplied by these pure objects.

Fifteen concrete body models cover final/provisional sleep, sleep detection, activity, wear/posture intervals, sport summary/realtime, six sample series and GPS. History envelopes and CRC belong to device-protocol; storage integrity belongs to device-storage. Sleep stage support, wear detection, counts, distance, energy and physiological algorithms must come from selected real backends. A struct codec does not create or validate a physiological measurement.

```sh
cargo test -p device-motion --all-features --locked
cargo clippy -p device-motion --all-features --all-targets --locked -- -D warnings
cargo check -p device-motion --all-features --target thumbv6m-none-eabi --locked
```

New masks and schema2 layouts follow specs/protocol/v06-contract-proposal.json. Existing sleep schema1 remains7B; provisional schema2 is8B. Respiration series uses hundredths per minute; GPS degrees use E7. No Flash, BLE, sensor or UI hardware is accessed here.
