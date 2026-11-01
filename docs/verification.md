# Offline verification — 2026-11-02

MoteFrame's first portable extraction contains four crates. No device, debug
probe, radio, firmware image, product backend or private asset is required.

## Executed checks

| Check | Result |
| --- | --- |
| Selected unchanged source baseline | 50 Rust test executions passed |
| Extracted workspace | 43 Rust tests passed |
| Each extracted crate selected individually | The same 43 tests passed again |
| Python boundary/package/image/convention regressions | 38 tests passed after the Apache-2.0 update |
| Workspace rustfmt and host Clippy | Passed with warnings denied |
| Cortex-M0 library Clippy and release builds | All four crates passed |
| Rustdoc | Passed with warnings denied |
| Procedural host renderer | Complete 64x40 RGB image; required colors verified |
| Cargo package candidate lists | All four crates contain only reviewed files and license texts |
| File/dependency boundary | Passed: no product/platform files or external path dependencies |
| Nested-directory secret-scanner fixture | Root artifacts ignored; nested documentation secret detected and redacted |
| Dependency policy, spelling, secrets and pre-commit hooks | Passed locally |

The seven source tests not exported belong to the excluded product-specific
display profile and external-scene replay tool. They remain in the private
source project. The retained runtime algorithms were not redesigned; public
crate/import names changed and unsafe code is explicitly forbidden in each
runtime crate. The extracted display crate has no built-in board profile.

`make check build` executes offline with locked dependencies. Per-crate tests
are repeated verification, not additional unique cases. The first boundary,
package, render-output and commit-message test runs failed before their new
validators were installed; their subsequent runs passed.

## Independent review and corrections

A separate read-only review inspected all four runtime crates, retained tests,
source bindings, package candidates and the dependency graph. It found no runtime
extraction regression, and identified two important enforcement gaps, now fixed:

- Inspect Cargo's full resolved dependency graph, including transitive sources
  and path patches. Workspace manifests and all target sources must use reviewed
  paths; external source targets and custom build scripts are rejected.
- Ignore build artifacts only at the repository root. Nested artifact-named
  directories are scanned; documentation also uses an explicit path allowlist.
  Forced Git additions cannot bypass the reviewed-file boundary. Secret-scanner
  exclusions now apply only to root build and Git storage directories.

The added regression tests reproduced both gaps before their fixes, then passed.
A separate temporary fixture confirmed secret detection inside `docs/target/`.
The fixture used a fabricated, nonfunctional credential and redacted reports.
Destination digest bindings were refreshed after formatting; the 23 original
source input hashes remain unchanged.

## Reproduce

After the pinned toolchain and locked registry dependencies are available:

```sh
make check build
make supply-chain secrets
make hooks
```

No command above publishes crates or operates hardware. `make packages` only
inspects candidate file lists; it is not proof that unpublished interdependent
crates can already be installed from crates.io. Registry publication will need
an explicit dependency order and separate authorization.

## Evidence and current limits

Full local logs, source/destination digest bindings and intermediate failure
records are under ignored `target/verification/`. That directory is not part of
the public source selection and is excluded from package lists. Source input
hashes are checked against the unchanged private files at closeout.

The initial dependency policy check used the existing advisory database dated
2026-08-06. Publication preparation additionally fetched the official RustSec
database into an isolated local cache, then checked the unchanged locked graph
offline with fetching disabled. That refreshed check passed advisories, bans,
licenses and sources using database commit
`2295466c4e9df26647fd67d8fa1c705ac2fc7f81` (2026-10-30). This is a check against
that database snapshot, not a guarantee against undiscovered vulnerabilities.

The initial extraction ended with a private empty GitHub repository and no
commits or pushes. The subsequent owner instruction explicitly authorizes the
first source commit, upload and public GitHub visibility. Only this standalone
portable tree is included; no source-product Git history is imported.

The publication preparation uses Apache-2.0 consistently in Cargo metadata and
SPDX headers. Root and crate LICENSE/NOTICE files are identical. Package checks
require NOTICE and reject obsolete alternative first-party license files. The
updated regression fixtures failed before the validators changed, then passed.
All 23 extracted Rust files differ from the previously reviewed candidate only
by copyright/SPDX headers; the original selected private input files are unchanged.

The CI workflow runs on GitHub pushes and pull requests. Remote execution results
are separate from the local results above; see the repository's Actions page and
the delivery report for the actual run status. Cargo registry publication remains
disabled. No crates.io upload, release, tag or hardware operation is included.

## Public source delivery

The reviewed 68-file tree was committed as
`4b4728148fec3ebb220819b595ba66a43c9ab6e4` and uploaded to the repository's `main`
branch. The repository is public, its default branch is `main`, and GitHub
recognizes Apache License 2.0. Anonymous requests verified that the root and all
four crates' LICENSE and NOTICE files match the reviewed local contents.

The initial source commit's [GitHub offline regression](https://github.com/byx-darwin/moteframe/actions/runs/41362033364)
passed, including host tests, Python checks, package audits and Cortex-M0 release
library builds. The original selected private source files remain unchanged.
This document's closeout updates do not change runtime code or dependencies.

This is software extraction/regression evidence, not measured device timing,
electrical compatibility, power consumption, real stack usage or a hardware
release qualification. The owner authorized the selected first-party Apache-2.0
publication and commercial attribution requirements; excluded material is not
relicensed. The governing terms are LICENSE, with applicable attribution in NOTICE.
