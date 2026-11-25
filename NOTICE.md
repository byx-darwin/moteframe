# Source and dependency notices

MoteFrame began as a selected extraction of owner-controlled portable Rust
components. Its portable crates carry regression tests and use the MoteFrame
package and import names. The separate [device workspace](devices/README.md)
includes the selected FR8029D first-party device source.

The owner authorized publication of the selected first-party code under
Apache-2.0. The canonical attribution notice is NOTICE and the full license is
LICENSE. The portable crates carry their own copies. Redistribution, including
commercial products, follows Apache-2.0 section 4. No additional license
restriction is imposed by this provenance document.

Dependencies retain their original licenses. See the
[upstream FreeRTOS-Kernel license](https://github.com/FreeRTOS/FreeRTOS-Kernel/blob/V11.2.0/LICENSE.md)
for the separately fetched optional kernel.

Direct upstream dependencies are:

- embedded-hal 1.0.0: MIT OR Apache-2.0; maintained by the Rust Embedded community.
  <https://github.com/rust-embedded/embedded-hal>
- embedded-graphics 0.8.2: MIT OR Apache-2.0; the procedural example uses its
  built-in ASCII font and drawing primitives.
  <https://github.com/embedded-graphics/embedded-graphics>
- embedded-graphics-core 0.4.1: MIT OR Apache-2.0; shared upstream color types
  used directly by the RGB565 driver. Already present in the original lockfile.
  <https://github.com/embedded-graphics/embedded-graphics>

The root workspace's locked dependency graph is audited by cargo-deny. See its
Cargo.lock and the independent device Cargo.lock files for exact versions.
