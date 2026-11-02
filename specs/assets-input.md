# Portable assets and input foundations

The user approved implementing the multi-role review recommendations: tighten
debug output and public contracts, extract assets and input, preserve compatibility,
measure memory and run offline regression. No hardware, product migration, Git
commit, push or publication is included.

Follow-up authorization (2026-11-03): after offline acceptance, the user explicitly
requested committing this work, merging it into main and pushing main to GitHub.
This supersedes the initial local-only Git restriction for this change. Hardware,
private product/resource migration, crates.io publication and releases remain out
of scope.

## Boundaries

Keep the existing six crates and their public traits, feature defaults and object
fields unchanged. Add independently selected `moteframe-assets` and
`moteframe-input`; neither is a dependency of core, UI, BLE or storage. No new
upstream dependencies. Runtime remains no_std, heap-free and unsafe forbidden.
Do not copy product formats, assets, thresholds, register maps or golden packets.

## Diagnostic and public contracts

`WriteSlot`, `SliceSource`, `ReadRef` and `ReadWindow` debug output must omit raw
bytes, routing target contents and backend Debug. The wrappers must not require
backend Debug just to be printable. Clear/take remain logical removal, not secure
erasure. Preserve all existing error variants, payloads and send behavior.

Document synchronous completion and caller buffer ownership, finite waits,
foreground-only shared SPI, queue acceptance versus peer ACK, uncertain submission
errors, and logical redraw versus optical completion. Backend errors remain typed;
applications must not treat every error as retryable. No common error registry,
clock executor or automatic recovery is added.

## Assets

Reuse core::ReadAt; callers can pass storage::ReadWindow. No second window or
resource-version implementation. Existing UI ResourceKey/Metadata/View stay in UI.
Immutable descriptors and version metadata are separate from a caller-held mutable
reader and transfer scratch; no global sharing or hidden interior mutability.

`Raster<F>` describes a nonempty tightly packed image at a byte offset with u16
width/height and a checked u32 byte extent. `PixelFormat` decodes individual pixels
from a fixed-width byte encoding; built-in Rgb565Be/Rgb565Le, Rgb888Bytes, Gray8Bytes
and Mono8 decode into upstream color types. These are stored-asset formats, not
panel wire formats. Mono8 admits only 0/1. Format bytes-per-pixel must be nonzero.
The decoder contract is fallible and requires exactly that byte length.

`read_span` accepts one nonempty in-row color slice and caller-owned byte scratch.
Validate geometry, full source extent and at least one pixel of scratch before I/O.
Read bounded chunks fitting scratch, stop on first read/decode failure, never retry.
Any error invalidates the whole output, which may already contain partial data.
No implicit 256-byte buffer, entire image load, font table or cache.
Glyphs reuse rasters with caller-provided signed bearings and advance; character
mapping, font contents and resource catalogs remain provider concerns.

## Input

Input is independent of screen, I2C, BLE, chip registers and product runtime.
Callers supply normalized pressed state or single-contact logical touch samples.
All timestamps are explicitly monotonic microseconds in the same boot epoch;
regression rejects without changing state or queued events. A new instance starts
a new epoch. Hardware counter extension and UTC conversion belong to adapters.

`EventQueue<T:Copy,N>` owns exactly N slots, supports N=0, preserves FIFO order and
reports Full without overwrite. Debug reports capacity/count only. External
synchronization is required across interrupts/tasks.

Button thresholds are caller-configured: debounce microseconds and optional
nonzero hold time. Emit Pressed/Released for debounced changes and one Held per
press. Initially released. On queue Full the attempted state transition is not
committed; caller retains/retries the sample before advancing its input stream.

Touch configuration contains logical bounds, tap slop, swipe threshold greater
than slop, and a nonzero hold threshold. Down/Move/Up/Cancel are normalized samples,
not register or vendor gesture codes. Down while active and Move/Up without Down
are sequence errors. Track maximum travel so moving away and back cannot create a
tap/hold. Up emits at most one Tap/Swipe/Held; tick emits one Held while down.
Cancel emits Cancelled only for an active contact. Queue Full, bad coordinates,
sequence or time errors preserve all state. Coordinate rotation/mirroring and
page actions stay outside. No multitouch or synthetic measurement values.

## Compatibility and evidence

Test redacted Debug after push/take/clear/shorter reuse and non-Debug backends.
Retain old consumers and frozen core/I2C resource baselines byte-for-byte. Adding
workspace members changes manifests/lock: record exact reviewed graph transitions
without changing historical cost values or accepting arbitrary graph drift.

Assets tests use synthetic, hand-derived byte fixtures, multiple colors and
scratch sizes, a shrinking/failing reader, full-range and overflow rejection and
glyph metadata. Combine real assets+storage+UI to verify exact pixels, resource
revision updates and retained redraw on read failure.
Input tests cover deadline equality, bounce, once-only hold, N=0/full queue,
release/hold retry, invalid sequence/coordinates, all swipe directions,
move-away-return, cancellation and clock regression.

Fixed host-executed and Cortex-M0-linked consumers measure descriptors, input
states/queues at N=1/4/16, transfer scratch and output capacities, linked Flash,
static data/bss and local stack frames. Report these as consumer costs, not whole
firmware/IRQ stack or hardware qualification. Verify old API consumers, independent
opt-in, no/default/all UI features and private-source/package boundaries.
