# I²C compatible extensions Implementation Plan

> **For agentic workers:** Use executing-plans task-by-task. Implementation was requested after the written assessment; continue in this existing feature checkout and preserve all earlier work. No commits are authorized.

**Goal:** Implement stage A of the approved core/I²C assessment without breaking existing consumers.

**Architecture:** Add explicit address, checked-read and resource-recovery APIs to the existing narrow reader. Use separately linked internal completion methods; preserve legacy fields and behavior. Keep standard HAL backends and full transaction engines deferred.

**Tech Stack:** Stable Rust 1.96, no_std, allocation-free, unsafe forbidden; Python resource gates and fixed Cortex-M0 consumers.

**Spec:** `specs/i2c-extensibility.md`, derived from the reviewed assessment in `target/core-i2c-assessment-2026-11-02/assessment-and-proposal.md`.

## Global Constraints

- Preserve Line/new/read/I2cError, object fields, old addresses and cleanup semantics.
- No dependency additions, no implicit retry/recovery, no hardware operations or Git staging/commits/pushes.
- Retain prior uncommitted core/display/UI/documentation work.
- Host fixtures are software evidence; measured local frames are not whole call-chain or IRQ stack.

## Review Focus

- Pointer STOP SDA stuck must fail before sending a second address; final STOP stuck must also fail.
- NACK combined with cleanup failure must retain NACK and still release master outputs.
- Last-budget simultaneous SCL/SDA high must succeed; bounded failures must terminate.
- Capturing non-Debug owned closures/pins must remain recoverable without extra trait bounds.
- Strict implementation must be absent from legacy linked consumers; resource gates must reject provenance/build-context drift.

### Task 1: Freeze legacy I²C compatibility and resources

**Files:** scripts/check_i2c_compatibility.py, scripts/probes/i2c_compat.rs, scripts/i2c-compatibility-baseline.json, scripts/tests/test_i2c_compatibility.py.

**Interfaces:** consumes current BitBangI2c/Line; produces frozen build, source, workload, host-output, layout, linked-section and local-frame evidence, with read-only normal checking.

- [x] Write gate tests rejecting changed behavior/layout/context, positive resource deltas and tampered provenance; observe missing validator failure.
- [x] Implement a fixed consumer/probe and diagnostic measurement using existing ELF/frame readers and pinned build conventions.
- [x] Before production edits, record baseline only into target, review and freeze its JSON/hash; normal execution must never update it.
- [x] Run Python gate tests and legacy probe. Expected: all pass, old candidate deltas zero.

### Task 2: Add resource recovery and ordinary-address APIs

**Files:** crates/moteframe-i2c/src/lib.rs, src/resources.rs, tests/extensions.rs.

**Interfaces:** produces NormalAddress7::{new,get}, InitError::{error,into_parts}, BitBangI2c::{try_new,into_parts,read_normal} with signatures in the spec.

- [x] Write tests for ordinary-address bounds, no-I/O failed construction retaining owned and borrowed resources, non-Debug closure recovery and normal/raw behavior; run and observe missing API failure.
- [x] Implement additive APIs with private fields, complete docs, first-error and zero-I/O ownership semantics.
- [x] Run all I²C tests and legacy cost gate. Expected: pass, nonpositive legacy resource deltas.

### Task 3: Add checked completion and maintained fault regressions

**Files:** crates/moteframe-i2c/src/lib.rs, tests/wire.rs, crates/moteframe-core/tests/faults.rs.

**Interfaces:** consumes NormalAddress7; produces CheckedReadError and read_checked; reuses existing Line and budgets without adding fields.

- [x] Add two-STOP stuck-SDA target assertions and core matrix; run to observe missing checked API failure.
- [x] Implement bounded actual-line checks at both STOPs via a separately linked opt-in helper; preserve first error and final cleanup.
- [x] Extend tests for multi-byte ACK/NACK, 49 legacy wait failures, checked faults/first error, last-budget success, output invalidation, Busy side effects and no automatic poison/retry.
- [x] Add strict/normal/recovery probe modes. Run all crate tests and resource gate. Expected: old behavior and costs remain compatible; new modes measured independently.

### Task 4: Integrate docs, gates and final review

**Files:** README.md, crate README, docs/i2c-extensibility.md, docs/progress.md, Makefile, scripts/check_boundary.py, package tests.

**Interfaces:** consumes all prior APIs and evidence; produces reviewable configuration/usage and automatic make check coverage.

- [x] Update reviewed-file allowlists and package tests, and document ordinary/raw/checked API choice, resource lifecycle, timing/recovery obligations and actual costs.
- [x] Run make check build, typos and diff checks. Expected: clean.
- [x] Dispatch one fresh read-only final review including untracked changes; address material findings with RED→GREEN and full checks. Preserve artifacts because changes are not committed.
