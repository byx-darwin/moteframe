# Source and dependency notices

MoteFrame is a selected extraction of owner-controlled portable Rust components.
This repository does not redistribute the source project's private product,
chip/board implementations, original visual resources, binary dumps or vendor
software. The selection carries regression tests and changes package/import
names. A generic display driver remains; the extracted board initialization
profile does not.

The owner authorized publication of the selected first-party code under
Apache-2.0. The canonical attribution notice is NOTICE; the full license is
LICENSE. Both files accompany every component. Redistribution, including
commercial products, follows Apache-2.0 section 4. No additional license
restriction is imposed by this provenance document.

Excluded private material is not relicensed. Dependencies retain their original
license choices; the first-party Apache-2.0 choice does not relicense them.

Direct upstream dependencies are:

- embedded-hal 1.0.0: MIT OR Apache-2.0; maintained by the Rust Embedded community.
  <https://github.com/rust-embedded/embedded-hal>
- embedded-graphics 0.8.2: MIT OR Apache-2.0; the procedural example uses its
  built-in ASCII font and drawing primitives.
  <https://github.com/embedded-graphics/embedded-graphics>
- embedded-graphics-core 0.4.1: MIT OR Apache-2.0; shared upstream color types
  used directly by the RGB565 driver. Already present in the original lockfile.
  <https://github.com/embedded-graphics/embedded-graphics>

Dependencies are referenced through Cargo, not copied into this tree. The locked
transitive dependency graph is audited by cargo-deny. Each dependency retains
its own copyright notices and license terms. See Cargo.lock for exact versions.
