# MoteFrame contributor and agent rules

- Scope: portable interfaces, drawing, generic display transport, buses,
  read-only storage views, bounded BLE value handling, source-backed assets and normalized input state machines.
- Keep runtime crates `no_std`, allocation-free, and `forbid(unsafe_code)`.
- Product applications, chip registers, board wiring, radio implementations,
  panel-specific initialization, original assets, vendor files, and device dumps
  belong outside this repository.
- Storage/BLE here are portable sources/views and checked application-value
  adapters. Physical Flash drivers, BLE host/controller/radio stacks, pairing,
  private product protocols and business persistence remain outside this repo.
- Use `embedded-hal` and `embedded-graphics`; avoid duplicate abstractions when
  their existing contracts apply. Runtime crates must not depend on `std`.
- Preserve synchronous completion, bounded I/O obligations, first-error
  reporting, and poisoned-bus behavior. Never retry uncertain writes silently.
- Hardware examples must be supplied by adapters outside the portable core.
  Host fixtures demonstrate software behavior, not electrical qualification.
- Use one responsibility per crate; keep dependencies within this workspace or
  the explicitly approved upstream dependency set.
- Run `make check` for Rust changes and `make supply-chain` for dependency changes.
  Run `make hooks` before any explicitly authorized commit.
- Never stage, commit, push, publish, create a release, or operate hardware
  without explicit user authorization. Repository creation alone is not push
  authorization. Never run `cargo clean` or overwrite unrelated changes.
- Test behavior, errors and boundaries; carry the existing regressions with
  extracted implementations. New behavior needs a failing test first.
- Public APIs need documentation, explicit errors and `Debug` implementations.
  Byte/event containers redact payloads, routing identities and backend internals from Debug.
  No `todo!`, `unimplemented!`, production `unwrap`/`expect`, or hidden failures.
- Apply Conventional Commits. Keep secrets and private paths out of source,
  documentation, examples, logs intended for distribution, and commit messages.
