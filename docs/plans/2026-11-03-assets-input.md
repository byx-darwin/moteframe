# Assets and input implementation plan

> **For agentic workers:** use executing-plans inline. Preserve uncommitted work;
> no commit, push, publication or hardware operation is authorized.

**Goal:** implement the approved review boundaries with portable assets/input,
diagnostic redaction, compatibility and fixed consumer memory evidence.

**Architecture:** immutable asset descriptors read via existing ReadAt and caller
scratch. Input state machines publish atomically into a caller-owned bounded
queue. Neither creates a reverse dependency from current framework components.

**Tech Stack:** Rust 2024/1.96, existing embedded-hal/embedded-graphics dependencies,
Python offline gates, fixed Cortex-M0 probes.

**Spec:** specs/assets-input.md.

**Follow-up integration authorization (2026-11-03):** the user requested merge and
push after offline delivery. Commit this reviewed change, reconcile main, verify
the merged tree and push origin/main. This supersedes the original local-only Git
constraints below; it does not authorize releases, private migration or hardware.

## Global Constraints

- no_std, heap-free, unsafe forbidden in runtime crates; no new upstream dependency.
- Preserve old public trait requirements, features, object fields and error values.
- Product resources/protocols/algorithms and hardware remain outside this project.
- Keep frozen core/I2C baselines; only exact Cargo graph migration is permitted.
- Do not stage, commit, push, publish or operate hardware.

## Review Focus

- Debug after logical removal must not reveal old payload or backend contents.
- Resource views must not falsely promise mutable reader access or content snapshot.
- Reader/decoder failures invalidate the entire span and preserve UI redraw.
- Full input queue must not consume the event's state transition, including hold.
- Timestamp regression and move-away-return must not create false touch events.

### Task 1: Redact containers and clarify contracts

Files: storage/ble src/lib.rs and tests; docs/public-contracts.md.
Consumes: existing ReadAt, NotificationBackend, WriteSlot; produces no API changes.
- [x] Add behavior tests for current/stale byte redaction and non-Debug backend.
- [x] Run targeted tests; expect payload-containing Debug failures.
- [x] Implement bounded metadata-only Debug and documentation; keep raw errors.
- [x] Run storage/BLE and original workspace tests; expect all pass.

### Task 2: Bounded assets

Files: new crates/moteframe-assets/{Cargo.toml,src/*.rs,tests/*.rs,README.md,LICENSE,NOTICE}; workspace manifest/lock.
Consumes: ReadAt; produces Raster<F>, PixelFormat, format decoders, Glyph metrics.
- [x] Write hand-derived decoding, bounds, error and scratch-size tests first.
- [x] Observe missing API failure, then implement the documented interfaces.
- [x] Add a real UI/ReadWindow consumer: exact image, failed read retry, version update.
- [x] Run new tests and existing UI tests; expect exact pixels and retained dirty.

### Task 3: Input state machines

Files: new crates/moteframe-input/{Cargo.toml,src/*.rs,tests/*.rs,README.md,LICENSE,NOTICE}.
Produces: EventQueue<T,N>, Button/ButtonConfig/ButtonEvent, Touch/TouchConfig/TouchSample/TouchEvent.
- [x] Write timestamp, debounce/hold, queue-full retry, touch geometry/gesture tests.
- [x] Observe missing API failure; implement separate queue/button/touch modules.
- [x] Test N=0/1/4 and regression after failure; expect no lost transitions.

### Task 4: Compatibility, memory and delivery

Files: boundary/packaging gates, Makefile, scripts/probes/assets_input.rs,
scripts/check_assets_input.py, Python gate regressions, docs/assets-input.md.
Consumes: tasks 1-3 public APIs and untouched historical baselines.
- [x] Add boundary/worktree and consumer/evidence rejection regressions first.
- [x] Register eight reviewed crates and exact Cargo context transitions.
- [x] Execute host consumers and ARM probes: small/large scratch, queue capacities.
- [x] Preserve legacy consumers and run make check build supply-chain secrets.
- [x] Obtain one independent final review; fix important findings with RED/GREEN.
- [x] Save measured results and final validation without Git publication.
