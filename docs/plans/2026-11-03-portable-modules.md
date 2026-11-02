# Portable modules Implementation Plan

> **For agentic workers:** Use subagent-driven-development. User explicitly requested multi-role assessment, autonomous implementation and merge/push; no new approval gates are needed for this authorized scope.

**Goal:** Deliver compatible storage/BLE primitives and maintained RAM-switch guidance, together with the prior authorized improvements.

**Architecture:** Two independent optional-by-dependency runtime crates plus a standalone app example. Existing consumers retain their APIs and frozen cost budgets via an explicit reviewed workspace-context migration.

**Tech Stack:** Rust 1.96, no_std/no heap/unsafe forbidden; existing embedded-hal/graphics and core::ReadAt; Python quality/resource tooling.

**Spec:** specs/portable-modules.md.

## Global Constraints

- Preserve previous work, existing APIs/defaults, bounded operations, raw first errors and explicit ownership.
- No vendor/private implementation, hardware, unapproved external dependency, hidden retry or release publication.
- User authorizes commits, merge and push to existing origin after all checks. No force push or destructive cleanup.

## Review Focus

- Storage empty-after-shrink vs nonempty dynamic bounds, usize/u32 overflow and original-source recovery.
- BLE stale identities, capacity/connection races, queued-success semantics and no uncertain retries.
- Slot zero-length event, oversized/full rejection and destination-too-small nondestructive behavior.
- Both actual 8/32 app feature states, preserved errors and matching stream/refresh capacities.
- Frozen baseline provenance stays immutable; only exact recorded workspace migration is accepted.

### Task 1: Storage runtime and tests

**Files:** crates/moteframe-storage/{Cargo.toml,README.md,LICENSE,NOTICE,src/lib.rs,tests/read.rs}.

**Interfaces:** produces SliceSource, ReadRef, ReadWindow, RangeError, ReadError and WindowCreateError exactly as specified. Parent owns root manifest and allowlists.

- [ ] Write behavior/boundary/resource tests first; run against missing API and record RED.
- [ ] Implement concrete slice/window/reference APIs without changing core; docs/Debug and private validation overflow test.
- [ ] Host tests and host/ARM lint/build pass in isolated target. Review resource lifecycle and provider-error preservation.

### Task 2: BLE runtime and tests

**Files:** crates/moteframe-ble/{Cargo.toml,README.md,LICENSE,NOTICE,src/lib.rs,tests/values.rs}.

**Interfaces:** produces NotificationBackend/Ref, CheckedNotifier, QueueAccepted, NotifyError, WriteMeta, WriteSlot, WriteBufferError exactly as specified. Parent owns root manifest/allowlists.

- [ ] Write notification/slot tests first; observe RED before implementation.
- [ ] Implement bounded stateless notification checks and const-capacity complete-event buffering; expose resource recovery with no backend calls.
- [ ] Test stale targets, changing limits and enqueue races, raw failures/no retry, empty/512 limits, all nondestructive rejections, N=0 and N>512. Host/ARM checks in isolated target.

### Task 3: Maintained application RAM switch

**Files:** examples/ui-low-memory/{Cargo.toml,Cargo.lock,README.md,src/lib.rs,tests/render.rs}, scripts/check_ui_low_memory.py; root README RAM subsection and docs/ui-memory-profiles.md only.

**Interfaces:** app-local ui-low-memory, UI_PIXELS=8/32, typed errors; checker tests/builds both states. Parent integrates Makefile/allowlists.

- [ ] Add tests for actual array/output/write counts and error behavior; observe missing app implementation RED.
- [ ] Implement small reusable app consumer using existing UI APIs; no library feature/default changes.
- [ ] Run host tests and ARM build for both feature states; update README with runnable commands and limitations.

### Task 4: Workspace and cost-context integration

**Files:** root manifests/lock, CLAUDE/README overview, boundary/package tests, Makefile; scripts/check_build_context.py and migration JSON/tests; existing core/I²C checkers; new fixed-consumer cost probes/checker.

**Interfaces:** consumes new crates/example. Produces six-crate allowlist, explicit current-vs-historical context policy and new resource reports while old budgets stay unchanged.

- [ ] Freeze start-of-task files/context and existing reference hashes in target; write migration acceptance/rejection tests and observe RED.
- [ ] Add new members/approved files/independent checks. Record exact reviewed Cargo-only context migration; never alter old reference bytes or source provenance.
- [ ] Measure new fixed consumers and rerun old gates; positive legacy cost/layout/behavior deltas still reject.
- [ ] Extend public docs/architecture/range/success semantics and progress. Run full make check build and supply-chain.

### Task 5: Review, merge and remote delivery

**Interfaces:** complete worktree including prior changes; produces verified commits/main remote state.

- [ ] Fresh task reviews of storage/BLE/RAM and integration; fix material findings with RED→GREEN.
- [ ] One broad fresh final review; address material findings, record deferred minors and rulings.
- [ ] Run make hooks on actual sources, commit scoped delivery after clean checks; fetch and merge current main without force, verify merged state.
- [ ] Push feature/main and verify origin/main commit and available remote CI. Preserve local evidence and worktree.
