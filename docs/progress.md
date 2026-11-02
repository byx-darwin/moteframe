# MoteFrame progress

## Initial private extraction (historical)

Plan: `docs/plans/2026-11-02-foundation-extraction.md`.

- Ruling: The user-selected empty destination is the isolated project workspace;
  initialize a feature branch there rather than create another source checkout.
- Ruling: The user's latest instruction explicitly authorizes the four-crate
  extraction and private GitHub creation. Continue that scope without another
  scope approval. No source push or public publication is authorized.
- Ruling: This is behavior-preserving extraction. Carry existing behavior tests;
  use test-first development for the new boundary validator.
- Ruling: Proposed first-party licensing is MIT OR Apache-2.0; excluded material
  receives no new license or redistribution permission.
- Completed: four portable crates extracted with unchanged runtime algorithms,
  reviewed tests, procedural renderer, documentation and local quality gates.
- Completed: selected original baseline passed 50 tests; extracted workspace
  passed 43 unique Rust tests and 35 Python checks. Per-crate tests also passed.
- Review: independent read-only review found two important boundary-validator
  gaps. Failing fixtures reproduced them; full resolved-graph/source-target
  checks and nested-directory/forced-tracking checks now cover both gaps.
- Review: secret-scanner fixture detects nested documentation secrets; ignored
  extraction destination hashes refreshed after formatting.
- Ruling: offline advisory results are limited to the cached database dated
  2026-08-06. Refreshing that database and owner rights confirmation remain
  prerequisites for any later public release.
- Closeout: original selected files unchanged; source remains local and
  uncommitted. GitHub remains private and empty; no device was accessed.

## Apache-2.0 and public GitHub publication

- Authorization: the owner explicitly requested Apache-2.0 plus NOTICE, commercial
  attribution retention, offline regression, commit/push and public visibility.
  This supersedes the historical no-push/private-empty restrictions for this repo.
- Workspace: continue in the independent user-selected repository and existing
  feature branch; no original source checkout or history is copied.
- Pre-flight: package manifests, metadata validation and package-file auditing
  must all agree on Apache-2.0, LICENSE and NOTICE before staging.
- Test-first: updated metadata/package fixtures failed in 3 cases as expected;
  production validators now enforce Apache-2.0 and required NOTICE.
- Scope: no runtime algorithm changes, hardware, crates.io publication or releases.
- Task 1 complete: 43 unique Rust tests, 38 Python regressions, host/ARM lint,
  docs, renderer, package lists, Cortex-M0 release builds and pre-commit passed.
- Publication review: independent read-only review found no Critical, Important
  or Minor issue in the exact 68-file candidate set.
- Refreshed advisory check: official RustSec snapshot dated 2026-10-30 was
  fetched into an isolated cache; all dependency policy checks then passed offline.
- Review rulings: runtime correctness retains prior reviewed regression evidence;
  hardware remains outside scope. Owner publication authorization is accepted
  without claiming independent legal certification. Remote commit, visibility
  and CI are verified separately after the authorized upload.
- Task 2 complete: the 68-file initial commit was uploaded to main, repository
  visibility is PUBLIC, and anonymous root/crate LICENSE/NOTICE requests match
  the local files. GitHub recognizes Apache-2.0. Initial source CI run
  `41362033364` passed. No hardware, private source history, registry upload,
  release or tag is involved.

## Local color-interface 0.2 candidate

- Approved design: `specs/color-types.md`; plan: `docs/plans/2026-11-02-color-types.md`.
- Core associated Color and generic UI are implemented; RGB565 byte encoding
  remains in the concrete DCS driver. Existing tests migrate fixture types only.
- Test-first: new color cases first rejected the missing associated-type API;
  all 20 added cases now pass. Memory parser first failed unimplemented, then
  passed controlled LLVM/ELF fixtures and real compiler artifacts.
- Complete offline gates: 63 unique Rust cases, 44 Python tests, host/ARM lint,
  docs, package audits, example, ARM builds, memory checks and hooks passed.
- Independent final review found no Critical, Important or Minor issue; it
  independently verified test outputs, compiler report and identical PPM hashes.
- Measured: RGB565 buffers and adapter objects unchanged; fixed ARM probe
  executable sections +92 B, stream local frame +8 B. See `docs/color-types.md`.
- Review scope: real panels and timing remain unverified; complete firmware,
  indirect-call/IRQ stack and product integration are separate work. Additional
  color physical drivers are not provided by this abstraction change.
- Delivery: local branch codex/color-types; no new commit or push, no hardware,
  no business implementation, and no changes to the separate private product.

## Local UI configuration r0.2 candidate

- Approved: `docs/ui-configuration.md`; plan: `docs/plans/2026-11-02-ui-configuration.md`.
- Implemented: typed settings/state/context, validated resource views, optional
  layout/refresh/damage/pacing, atomic updates, display-generation recovery and
  bounded next-frame invalidation. Low-level default interfaces remain available.
- Compatibility rulings: inferred StreamTarget::new retains RUN32; explicit RUN
  uses with_run. draw stays feature-additive; draw_timed supplies optional clocks.
- Test-first missing APIs and absent feature failures were observed before their
  implementation. Integration covers read-window I/O/decode faults and retries.
- Verification: 82 unique Rust cases, 47 Python cases, 16 feature subsets and 30
  real consumer cases; ARM layouts/linked consumers, complete make check/build,
  dependency policy, secrets and hooks passed. Repeated runs are not new cases.
- Independent review: no actionable correctness/safety defect; resource-fixture
  evidence gap was supplemented and separately rechecked.
- ARM RGB565 budgets: base state24; default state32/session48; paced session88;
  caller scratch and transient candidates are separate. Full call-chain/IRQ
  stack and product/device performance are not established by object frames.
- No changes to the other three runtime crates since this task's snapshot.
  No hardware, staging, commit, upload or release. See implementation report.

## UI configuration r0.2 multi-role acceptance

- Three fresh independent roles audited trimming/API/memory evidence,
  updates/resources, and refresh/recovery/pacing.
- Confirmed P2: compile-fail validation could accept a type name in a warning
  while compilation failed for an unrelated reason. Reproduced before changes.
- Fixed the gate using structured Cargo errors with consumer ownership, expected
  code/message, primary locations and rejection of additional unrelated errors.
  Ten maintained Python regressions cover the gate; real adversarial consumers
  were rechecked and the original reviewer confirmed the correction.
- Independent runtime fixtures: 6 updates/resources cases plus 3 rejection cases;
  9 refresh/pacing cases including 201 injected failure positions and maximum
  width/height traversal. No runtime correctness defect identified.
- Full corrected-tree regression closeout is recorded in
  docs/ui-configuration-review-r02.md. No business, hardware or source upload.

## Authorized 0.2 source delivery

- The owner explicitly authorized committing and pushing the typed-color
  migration, UI configuration interfaces and acceptance-gate correction.
  This supersedes the earlier local-only restrictions for those changes.
- Delivery branch: `codex/ui-configuration`; preserve the existing main branch.
  No release tag, crates.io publication, firmware image or hardware operation.
- Scope: only allowlisted portable source, maintained tests, docs and validators.
  Product code, original assets, vendor libraries, device data, target output,
  independent review fixtures and local execution logs remain excluded.
- Before committing, rerun the complete repository gates, ARM release build,
  dependency policy, source/package boundary, secret scanning and pre-commit.
  Retain Apache-2.0 and canonical NOTICE in all four crates.
- Refresh color probe numbers against the combined 0.2 source. These are offline
  object/link/layout observations, not product/device qualification.

## Core extensibility and resource recovery

- Owner approved the multi-role incremental proposal and requested implementation.
  Work is on local `codex/core-extensibility`, preserving earlier README edits.
- Added explicit embedded-hal 1.0 bus/pin/delay bridges; old traits, construction
  signatures, errors and RGB565 driver scope remain unchanged. SPI bridge writes
  once, attempts flush even on failure and retains the first error.
- Added pin-preserving try_device and consuming core/device/shared-panel resource
  reclamation. Extraction does no I/O and is not hardware recovery or fault reset.
- Clarified Display completion vs optical visibility, finite waits, Result-only
  cleanup and ReadAt empty-range behavior; no new storage backend or product code.
- New `make core-compat` is part of `make check`, including real consumers,
  consumer-owned E0505 rejection and frozen linked Cortex-M0 cost evidence.
- Legacy dedicated/shared consumers: Flash/static RAM/local frame deltas all 0.
  Owned HAL bridge matches correct manual bridge; borrowed variant -36 B Flash
  but +16 B exercise frame. Opt-in recovery workloads are reported separately,
  not treated as equivalent DCS comparisons or accepted product stack budgets.
- Fresh final review found two Important evidence-gate gaps, both corrected
  after failing regressions: explicit/fingerprinted build configuration, and
  immutable complete baseline provenance including display/workspace manifests.
- Corrected-tree `make check build` passed: 91 maintained Rust and 67 Python
  tests, existing feature matrix/probes, new consumers/cost gates and package
  boundaries. Offline dependency policy, secrets, spelling and diff checks passed.
- No new dependencies, hardware, staging, commit, push, tag or publication.
  See `docs/core-extensibility.md`; full local evidence remains ignored in target.

## Stack-cost follow-up

- Rechecked the identical workload using full ownership, all borrowed objects,
  and only borrowed SPI: exercise frames 160/176/168 B, linked Flash
  1314/1278/1324 B. Executed output checksums are identical.
- The all-owned form saves 16 B local frame at +36 B Flash compared with all
  borrowed; borrowing only SPI saves 8 B at +46 B Flash. These are fixed offline
  fixture results, not arbitrary HAL or complete-product stack guarantees.
- Three temporary inlining variants did not reduce the borrowed exercise frame
  and were not adopted. No runtime code, buffers or completion/error contracts
  changed; added the ownership trade-off to the integration guide only.

## UI memory-profile study

- Evaluated 14 fixed-workload cases (7 capacity/storage combinations, with and
  without compiled pacing) and four seeds, including accepted pixel output,
  final image, failures/retry, future requests, idle and capacity rejection.
- Both stream and refresh checksums remain observable, preventing unused early
  drawing from disappearing. Final default 32/8 results: UI object budgets
  144/96 B, exercise frames 288/240 B, linked Flash 1452/1290 B. A 64x8 frame
  requires 16/64 write and service calls. This is not a measured latency ratio.
- Tile-only limit changes leave the 32-pixel array and object budget unchanged.
  Bundling caller state/scratch does not remove the existing session's borrows.
- Actual application-local compile-switch consumers built in both configurations
  for Cortex-M0; existing 63 maintained UI tests passed. No new library feature,
  runtime modification, hardware or private-product integration.
- Documented the recommended explicit 8/32 compile-time capacity selection in
  docs/ui-memory-profiles.md. Scratch study artifacts remain ignored in target,
  outside source packages and continuous CI.

## I²C compatibility extensions (2026-11-03)

- Implemented assessment stage A: ordinary NormalAddress7/read_normal, opt-in
  read_checked with both STOP idle observations, and resource-preserving
  try_new/InitError/into_parts. Legacy Line/new/read/I2cError and instance fields
  remain compatible; no dependency, heap, retry, poison or hardware additions.
- Promoted line-state fixtures: multi-byte ACK/NACK, all 49 SCL wait failures,
  both STOP SDA holds, post-STOP SCL drop, final-budget release and error cleanup.
  Added core select/operation/flush/release 16-combination regression.
- Added frozen I2C legacy resource/behavior gate to make check and existing CI.
  Legacy/normal fixed consumers: Flash 1226 B, exercise frame 72 B, static RAM
  0 B, legacy deltas all 0. Checked consumer: Flash 1356 B (+130), frame 80 B
  (+8), static RAM 0 B. Separate recovery workload: 104 B Flash / 28 B frame.
- Full make check build passed: 111 maintained Rust tests and 77 Python tests,
  feature consumers, Cortex-M0 probes/build, source/package boundaries and docs.
  Fresh review found one Important compiler-wrapper environment bypass; failing
  core/I2C regressions reproduced it, and the shared guard now rejects Cargo
  compiler aliases, target-specific flags and incremental overrides.
- Usage, budgets and limitations: docs/i2c-extensibility.md. Stage B HAL backend
  and stage C full software transactions remain deferred. No staging, commit,
  push, publication or hardware qualification.

## Portable storage / BLE and maintained RAM example (2026-11-03)

- Three-role assessment selected independent read-only storage views and bounded
  GATT value handling; physical Flash, BLE stacks/radio and product protocols stay
  external. No new upstream dependency or core/UI dependency on these modules.
- Storage adds SliceSource/ReadRef/ReadWindow and failure-preserving ownership.
  BLE adds checked notification admission and complete-event WriteSlot buffers.
  Existing public trait requirements, defaults and legacy error enums are unchanged.
- Maintained examples/ui-low-memory owns the application-only 8/32 switch,
  actual arrays/stream runs and typed errors/pending-work reports. Four final
  images match in both modes, with 14 behavior tests per mode and ARM builds.
- Original core/I2C frozen references remain byte-identical; exact reviewed Cargo
  graph migration admits the two new members without relaxing build/probe/context
  or resource checks. Every legacy Flash/static-RAM/local-frame delta remains 0.
- Fresh storage, BLE, RAM, integration and broad final reviews found no actionable
  issues. Full make check build passed: 148 workspace Rust tests, 89 Python tests,
  both RAM profiles, all consumer/probe/package boundaries and Cortex-M0 build.
  make supply-chain and make hooks passed, including source secret/spelling checks.
- Fixed resource reports and scope are documented in docs/portable-modules.md.
  All measurements are offline synthetic consumers, not hardware qualification,
  BLE interoperability or whole-firmware/interrupt-stack budgets.
