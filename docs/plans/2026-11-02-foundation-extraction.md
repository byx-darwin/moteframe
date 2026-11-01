# MoteFrame Foundation Extraction Implementation Plan

Historical extraction plan: its no-commit/no-push/private-repository restrictions
applied to the initial preparation. The later owner-authorized publication plan
in `2026-11-02-publication.md` supersedes those restrictions for MoteFrame only.

> **For agentic workers:** Use executing-plans to implement this approved
> extraction in this session. User instructions control repository location,
> private resource exclusion and the no-publish/no-hardware boundary.

**Goal:** Prepare an independently testable portable MoteFrame workspace.

**Architecture:** Four allocation-free library crates, with a single procedural
host example. Product-specific adapters and data remain outside the repository.

**Tech Stack:** Rust 1.96.0, Python standard library, embedded-hal 1.0.0,
embedded-graphics 0.8.2, Cargo, pre-commit, Clippy, cargo-deny, gitleaks.

**Spec:** `specs/foundation-extraction.md`.

## Global Constraints

- Destination is the user-selected independent `moteframe` repository.
- Do not modify the source repository, create commits, push, publish or use hardware.
- Runtime: no_std, no allocation, no unsafe code.
- GitHub: private empty repository only; local feature branch for preparation.
- Only portable interfaces, graphics and buses; no private resources or profiles.

## Review Focus

- Product initialization data copied indirectly through generic display tests.
- Workspace dependencies or packaging reaching outside the standalone tree.
- Secret, binary or symlink inclusion despite a clean source-file listing.
- Invalid geometry, partial transfer failures and transaction cleanup regressions.
- Offline checks accidentally requiring assets, devices or a network connection.

### Task 1: Enforce the standalone boundary

- [x] Write tests for injected product paths, vendor binaries, symlinks, private
  identifiers and dependency escapes in `scripts/tests/test_boundary.py`.
- [x] Run the tests, observe expected failures before the validator exists.
- [x] Implement `scripts/check_boundary.py`; run the same tests to green.

### Task 2: Extract selected portable implementations

- [x] Run existing selected source regressions as the unchanged baseline.
- [x] Copy selected regression tests first, changing only crate import names.
- [x] Create four manifests and copy whitelisted portable implementations;
  remove display profile modules and forbid unsafe code in every runtime crate.
- [x] Create a standalone locked dependency graph using the existing pinned
  external versions. Run host regressions and ARM library builds offline.
- [x] Keep source/destination hashes in ignored verification artifacts.

### Task 3: Package and document

- [x] Add README, crate docs, licensing, contributor guide, Makefile, pre-commit,
  dependency policy and CI, without product identifiers or private paths.
- [x] Preserve the procedural host renderer and validate its real output.
- [x] Audit Cargo package file lists and independently test each crate.

### Task 4: Verify and deliver

- [x] Run `make check build`, boundary tests, supply-chain, secrets and hooks.
- [x] Complete a separate final review; fix findings and rerun affected checks.
- [x] Verify original source hashes unchanged and GitHub private/empty state.
- [x] Record results and remaining publication prerequisites in `docs/verification.md`.

No task contains an automatic commit, push or release step.
