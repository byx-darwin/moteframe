# device-actions

Optional typed action bodies; no device I/O, heap or runtime dependencies beyond device-protocol. Default features empty. `system`, `find`, `phone`, `call`, `media`, `notification-actions` enable only their command variants and parsing.

Fourteen bodies cover reboot/poweroff/name restore, find/stop/result, heartbeat, call switch, call-action result, camera/ACK/music/ACK and schema2 gimbal control. Unknown routes, extra bytes and unsupported enum values are rejected. This is not a complete action service: phone readiness, duplicate ACK settlement, ownership and actual media/find/gimbal work still require their separate state and backend integration.

`DeferredSystem` stages a reboot/poweroff without touching a backend. Only a matching `confirm_response` after full successful response admission may schedule the action; failed transmission, scope revocation or disconnect drops it. A consumed ticket is not replayable. The real backend must also verify its principal lease when the delayed action executes. No production backend is included.

```sh
cargo test -p device-actions --all-features --locked
cargo clippy -p device-actions --all-features --all-targets --locked -- -D warnings
```
