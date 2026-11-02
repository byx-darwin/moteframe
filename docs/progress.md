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
