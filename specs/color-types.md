# Color-independent display and UI

Approved scope: make color an associated type of Display, use that type through
UI drawing/refresh/damage, preserve the RGB565 DCS driver, and verify offline.
No business code, hardware, private-source edits or automatic GitHub push.

## Contracts

- `Display::Color: Copy` has no graphics-library dependency in core.
- UI requires `D::Color: PixelColor`, using the upstream color unchanged.
- `Tile<C>` and caller-owned scratch use C directly. No Default, IntoStorage,
  RGB conversion, byte encoding, full framebuffer or heap requirement in UI.
- Stream drawing seeds its 32-color run buffer from the first visible pixel.
- Dirty rectangles, clipping, overwrite order, bounded cooperative service,
  error propagation and dirty retention keep existing semantics.
- DcsDisplay keeps its name and RGB565-only capability; re-export the upstream
  RGB565 type from the display crate. Only that driver encodes MSB-first bytes.
- Reuse embedded-graphics-core 0.4.1 (already locked) in the display crate.
- Existing API is intentionally changed: version the workspace 0.2.0 and document
  migration from the removed core tuple wrapper to the upstream RGB565 type.

## Evidence

Exercise RGB565, RGB888, Gray8 and BinaryColor, plus a custom color lacking
Default/IntoStorage. Cover clipping/source positioning, run boundaries, precise
color preservation, refresh/damage and failure/service semantics. Carry every
existing test, updating fixture types only. Verify RGB565 wire bytes unchanged.

Report actual host and Cortex-M0 color/adapter/tile layouts; compute 32-color
runs and two-row scratch explicitly using size_of rather than wire byte counts.
Report fixed RGB565 driver scratch separately. Code-size/static-frame probes,
if used, describe only their fixed workload and compiler settings, never a full
firmware or on-device worst-case stack guarantee.

Other color drivers, conversions, dithering, alpha and deferred panel commits
remain future work. Generic color support is not a claim of hardware support.
