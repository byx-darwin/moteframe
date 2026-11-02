# Color types — 0.2.0 source candidate

The display contract and UI now preserve the driver's logical color type.
Core has `Display::Color: Copy` and no graphics dependency. StreamTarget uses
`D::Color: PixelColor`; Tile is `Tile<C>`, and repaint/damage scratch contains C
directly. No Default, IntoStorage, RGB565 conversion or byte packing is required
by the generic renderer. Geometry and dirty tracking remain independent of C.

The concrete DcsDisplay still supports RGB565 only, encoding MSB-first bytes in
64-byte chunks. It re-exports embedded-graphics-core's RGB565 type. The display
crate directly references locked embedded-graphics-core 0.4.1; no new upstream
package/version was introduced. SPI and software I2C contracts are unchanged.

## Migration from 0.1

- Add `type Color = ...` to each Display implementation; `write` accepts that type.
- Replace `moteframe_core::Rgb565(value)` with the upstream color, for example
  `moteframe_display::Rgb565::from(RawU16::new(value))`. Import RawU16 from
  embedded-graphics/core's pixelcolor::raw module.
- Use upstream colors directly in scratch arrays, such as `[Rgb565::BLACK; 16]`.
  The black constant requires the upstream RgbColor trait in scope.
- Explicit Tile annotations become `Tile<'_, Rgb565>` or `Tile<'_, C>`.
- Generic scenes must accept their target's color or a caller-supplied palette;
  RGB565-specific scene literals do not automatically become grayscale artwork.

The version changes to 0.2.0 because these are intentional source API changes.
No migration or hardware adaptation was made to the separate product firmware.
Other color types currently have offline reference displays, not new physical
panel drivers. Conversions, dithering, alpha and deferred panel commits remain
outside this change.

## Memory accounting

Measured with the pinned Rust 1.96.0 compiler on the host and thumbv6m-none-eabi.
Both targets produced these pixel and buffer sizes, in bytes:

| Color | One logical color | 32-color stream run | Two rows at width 240 |
| --- | ---: | ---: | ---: |
| RGB565 | 2 | 64 | 960 |
| RGB888 | 4 | 128 | 1920 |
| Gray8 | 1 | 32 | 480 |
| BinaryColor | 1 | 32 | 480 |

These are logical color arrays. RGB888's wire encoding is a separate driver
decision; BinaryColor scratch is not a one-bit-per-pixel framebuffer. Repaint
uses caller-owned scratch and never allocates a full screen. The RGB565 encoder
keeps its existing 64-byte local buffer; it is not an additional persistent
framebuffer. Scratch and nested function frames must not be counted as if these
figures were a complete program RAM total.

| Adapter object | Cortex-M0 | 64-bit host | 0.1 Cortex-M0 |
| --- | ---: | ---: | ---: |
| StreamTarget | 4 | 8 | 4 |
| RGB565 Tile | 16 | 24 | 16 |
| DirtyRegion | 14 | 14 | 14 |
| DcsDisplay with the probe's four-byte ByteBus | 24 | 24 | 24 |

The bus type in a real application can have a different size. These adapters
have no heap allocation or runtime color-format discriminator.

## Fixed-workload compiler comparison

Compare source revision `0210229ae440f114a627f8b00c0461f0e3dce1a5` with the current 0.2.0
source candidate, using the same two RGB565 probe operations: streaming runtime-width
pixels and repainting with sixteen caller scratch pixels. The synthetic byte
consumer keeps the output observable; neither probe accesses a device.

| Compiler evidence | 0.1 baseline | 0.2 RGB565 | Difference |
| --- | ---: | ---: | ---: |
| Probe object executable sections | 1604 B | 1700 B | +96 B |
| StreamTarget draw_iter local frame | 136 B | 144 B | +8 B |
| Stream entry local frame | 56 B | 56 B | 0 B |
| Repaint entry local frame | 104 B | 104 B | 0 B |
| Cooperative repaint local frame | 72 B | 72 B | 0 B |

Adding RGB888, Gray8 and BinaryColor fill probes makes the candidate object
2580 B of executable sections (+880 B over RGB565-only). Those probes are absent
from the RGB565-only compilation. These measurements show instantiation costs
for this workload, not an application's final Flash footprint. The example's
layout-marker arrays and all other non-executable data are excluded from the
executable-section figures.

Probes use opt-level=s, panic=abort and no final link/LTO. Only the standalone
diagnostic rustc invocation sets RUSTC_BOOTSTRAP=1 to enable `-Zemit-stack-sizes`;
Cargo builds and all runtime libraries remain ordinary stable Rust. LLVM's
per-function stack-size metadata excludes callees, indirect calls, interrupts
and hardware exception frames. These values are not a complete worst-case stack
bound or on-device watermark, and final linked firmware can optimize differently.

The final source candidate includes the later const-generic stream capacity and
optional UI configuration interfaces. The table above is refreshed against that
combined source; the earlier color-only snapshot measured 1696 B and 2570 B.

## Verification and reproduction

The following counts describe the color-migration stage. Later configuration
and acceptance results (82 maintained Rust and 57 Python cases) are recorded in
[the multi-role acceptance](ui-configuration-review-r02.md).

- Retained 43 Rust cases migrated without weakening their assertions.
- Added 20 cases: RGB565, RGB888, Gray8, BinaryColor and a custom PixelColor with
  neither Default nor IntoStorage. Adjacent RGB888/gray values are preserved.
- Covered streaming/clipping/overwrite order, partial refresh, exact damage,
  scene/display/service failure and dirty retention across the five types.
- 44 Python validator/parser tests pass; host/ARM lint, documentation, package
  lists, offline example and Cortex-M0 release libraries pass.
- The procedural RGB565 PPM matches the baseline byte-for-byte; DCS wire tests
  retain their byte expectations, chunk boundaries, window and failure behavior.

```sh
make check build
make supply-chain secrets
make hooks
python3 scripts/check_memory.py --baseline 0210229ae440f114a627f8b00c0461f0e3dce1a5
```

`make memory` is part of `make check`. Its report is under ignored
`target/color-types/memory/report.json`; the optional baseline command preserves
the comparison separately as `comparison.json`. All commands use cached locked
Cargo dependencies. Baseline comparison also requires that revision in local Git
history. No network, hardware, firmware programming or business service is used.
