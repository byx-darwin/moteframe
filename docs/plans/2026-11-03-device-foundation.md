# Offline foundation implementation plan

> Execute inline with executing-plans and test-driven-development. Implementation
> was qualified offline first. The user subsequently authorized duplicate-test
> cleanup, offline regression, commit, merge to main and push on 2026-11-03.
> Hardware operations and release publication remain outside this task.

**Goal:** a runnable input/UI/assets/BLE composition with fault recovery, fairness,
replay and measured combined costs.

**Architecture:** an independent example owns capacities and scheduling. Existing
runtime crate APIs/manifests remain unchanged. UI session outlives individual
frames, while services progress independently before/after drawing and at tiles.

**Tech Stack:** pinned Rust 2024/1.96, existing path crates, Python standard library.

**Spec:** specs/device-foundation.md.

## Global constraints

- no_std, heap-free, unsafe forbidden in example library; std only for replay/tests.
- No business protocol, health algorithms, vendor/original resources or hardware.
- Preserve eight crate APIs, root Cargo manifests/lock and frozen cost baselines.
- One pending notification, retained failed raw sample, explicit uncertainty recovery.
- Commit, merge to main and push are authorized after the checks below pass.
- No release, product/vendor resource migration or hardware operations.

## Review focus

- UI Idle/Deferred/errors cannot be the only source of BLE progress.
- Mid-frame events must change next-frame pixels, not tear the current frame.
- Full must retain original sample/value and identity until real acceptance.
- Uncertain notification errors must never be automatically retried.
- Resource/display recovery must retain redraw and reject stale identities.

### Task 1: Service owner and full-queue behavior

Files: examples/device-foundation/Cargo.toml, src/lib.rs, src/services.rs,
src/model.rs and tests/services.rs. Independent lockfile and licenses.
Interfaces: Link extends NotificationBackend with bounded poll/receive and error
classification; Services owns input queues, RX WriteSlot and one pending TX.
- [x] Write tests for retained input, local/backend Full, uncertainty and routes.
- [x] Observe missing API failure; implement bounded rounds with first-error reports.
- [x] Test same payload/token on retry, no uncertain resubmit and generation retirement.

### Task 2: UI/assets composition and fairness

Files: src/scene.rs, src/runtime.rs, tests/integration.rs.
Consumes Services; produces UiMemory, Catalog and Demo::step/rebind/update_resources.
- [x] Write exact-pixel and Idle/Deferred/error fairness tests first.
- [x] Connect real rasters/ReadWindow/session and service callbacks with stable snapshots.
- [x] Test source/decode/display errors, explicit rebind, resource revisions and flooding.

### Task 3: Replay and combined memory

Files: examples/replay.rs, src/probe.rs, scripts/check_device_foundation.py,
scripts/tests/test_device_foundation.py, docs/device-foundation.md and README.
- [x] Add replay/evidence rejection tests before validator implementation.
- [x] Run both profiles with identical fixture events and compare actual pixels/counters.
- [x] Measure real ARM layouts, linked Flash/data/bss and combined local frame evidence.

### Task 4: Gates and final review

Files: root Makefile, .gitignore, README, scripts/check_boundary.py and its tests.
- [x] Add narrowly scoped standalone-example/target exceptions and regression tests.
- [x] Run default/small host/ARM and full make check build supply-chain secrets hooks.
- [x] Independent final review, fix important findings, preserve local candidate.

### Task 5: Reviewed source integration

Files: scripts/tests/test_boundary.py, docs/device-foundation.md and this plan.
Existing eight runtime APIs, manifests, locks and frozen baselines remain unchanged.
- [x] Add a test detecting shadowed test method names; observe current duplicates fail.
- [x] Remove the identical duplicate and rename the distinct unknown-member case,
  retaining both membership checks; run all Python tests and the source boundary.
- [x] Update source-delivery notes and run make hooks build on the exact candidate.
- [ ] Stage only reviewed portable source files, inspect the staged scope, then commit
  with the actual hooks enabled.
- [ ] Fetch origin/main, integrate into a clean main checkout without discarding changes,
  rerun make check build supply-chain secrets, then push main without force.
- [ ] Verify the remote head and GitHub checks for the exact pushed revision.
