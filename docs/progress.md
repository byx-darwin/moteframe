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
