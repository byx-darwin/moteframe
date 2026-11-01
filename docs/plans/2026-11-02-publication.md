# Apache-2.0 and GitHub Publication Implementation Plan

> Use executing-plans in the existing isolated MoteFrame repository. The owner
> has explicitly authorized this implementation, commit, push and public visibility.

**Goal:** Publish the portable workspace on GitHub with Apache-2.0 attribution.

**Architecture:** Preserve the four existing runtime crates. Ship the same
canonical LICENSE and NOTICE in the root and every crate; audit package contents
and resolved dependencies before exposing the first commit.

**Tech Stack:** Existing pinned Rust, Python validators, Git and GitHub CLI.

**Spec:** `specs/foundation-extraction.md`, amended by the owner's publication request.

## Global constraints

- Only portable interfaces, UI drawing, display transport and buses are public.
- Product code, board/chip adapters, vendor resources and source Git history stay private.
- Apache-2.0 is the first-party license; retain upstream licenses separately.
- Redistribution, including commercial products, carries the applicable NOTICE
  attribution and LICENSE under Apache-2.0 section 4; no extra license restriction.
- No hardware access, firmware programming, crates.io upload, tag or release.

## Review focus

- A crate archive must not omit NOTICE or retain an unintended alternative license.
- Root and crate notices must identify the same project, copyright and source URL.
- The initial Git history must contain only explicitly reviewed portable files.
- Publication documentation must distinguish local checks from remote CI results.
- Public visibility changes apply only to `byx-darwin/moteframe`.

### Task 1: License, attribution and offline gates

- [x] Update package/metadata regression fixtures for Apache-2.0, required NOTICE
  and rejection of obsolete first-party MIT files; observe expected failures.
- [x] Use canonical LICENSE plus NOTICE at root and each crate, add SPDX headers,
  update manifests, allowlists, package audits, README and contributor guidance.
- [x] Update current scope and verification documentation; retain old extraction
  constraints explicitly as historical records superseded by this authorization.
- [x] Run Python regressions, `make check build`, dependency/secret checks and
  pre-commit. Confirm original selected source hashes unchanged.
- [x] Complete one independent final publication-boundary review; fix blockers.

### Task 2: Commit, upload and verify public access

- [ ] Enumerate and review the exact Git candidate list; exclude ignored local
  evidence and private paths. Stage only the approved files and run hooks.
- [ ] Create the initial Conventional Commit on main and push only that branch
  to the existing private empty repository. Expect local/remote commit equality.
- [ ] Change only the authorized repository to public; verify anonymous access,
  source commit, LICENSE, root/crate NOTICE and remote CI status.
- [ ] Record actual delivery evidence locally and report any remaining limits.

The existing source repository is never staged, committed, pushed or relicensed.
