# MoteFrame foundation extraction

The owner approved the MoteFrame name, first portable extraction, destination,
offline regression and GitHub repository creation on 2026-11-02. The subsequent
publication instruction authorizes Apache-2.0 plus NOTICE, local regression,
commit, upload and public visibility for the selected source repository.
Package registry publication, releases and hardware operations remain outside scope.

## Deliverable

Create four crates: `moteframe-core`, `moteframe-ui`, `moteframe-display` and
`moteframe-i2c`. Shared SPI remains in core. Preserve the selected existing
behavior and integration tests; rename package/import paths only. Remove the
product-specific display profile from the public selection. Keep one procedural
host renderer that needs no external image, firmware, font file or device.

All runtime crates are `no_std`, allocation-free and unsafe-forbidden. Direct
external dependencies remain `embedded-hal =1.0.0` and
`embedded-graphics =0.8.2`. Pin Rust 1.96.0, retain the Cortex-M0 build check,
and provide workspace lint, Makefile, pre-commit and CI conventions.

## Boundaries

No product business, product pages, board/chip adapters, BLE implementation,
persistent storage, device images, vendor resources, extracted initialization
tables or proprietary assets enter the new tree. Existing private repositories
and Git history remain untouched. A positive source-file allowlist and metadata
validation enforce this initial scope; secret and dependency scans complement
that check but cannot certify ownership of every line.

Use Apache-2.0 for the selected owner-controlled first-party code and ship LICENSE
plus NOTICE at the root and in every crate. Preserve upstream dependency licenses.
Commercial redistribution must retain applicable source/attribution notices under
section 4, without additional restrictions. Excluded private or third-party
material receives no redistribution authorization.

## Acceptance

- Selected pre-extraction regressions pass; extracted regressions pass offline.
- Workspace formatting, host/ARM Clippy, host tests, docs and ARM release builds
  pass with locked dependencies and no hardware access.
- Each crate passes the same checks individually and has a package-file audit.
- The host renderer produces deterministic, bounded RGB output; no assets needed.
- Boundary-check tests reject injected board code, binary resources, escaping
  dependency paths, symlinks and private identifiers in portable Rust.
- Supply-chain and secret scans pass, with any incomplete check reported.
- Commit only reviewed portable files, upload to the named GitHub repository,
  make it public, and verify its exact commit and anonymously readable license/notices.
