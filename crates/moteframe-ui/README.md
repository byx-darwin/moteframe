# moteframe-ui

Allocation-free embedded-graphics integration over moteframe-core. Offers
geometry clipping, stack layout, small-buffer drawing targets, dirty regions,
and cooperative repaint helpers. The caller supplies a display, scratch buffer,
scene closure and cooperative yield/cancel policy.

Drawing and refresh use the display's PixelColor directly: `Tile<C>`, native
scratch slices and explicit background colors. No Default, IntoStorage, byte
encoding or RGB565 conversion is required. RGB565, RGB888, Gray8, BinaryColor
and a custom non-Default color are covered offline. Physical panel support is
provided by separate drivers; the included DCS driver remains RGB565-only.

`cargo run -p moteframe-ui --example offline --offline --locked -- target/demo.ppm`
renders a procedural host image without external assets or hardware. The host
example uses std; the library does not. Repaint work budgets are logical bounds,
not measured device latency guarantees.

Runtime: no_std, no allocation, unsafe forbidden. License: Apache-2.0; see LICENSE and NOTICE.

## UI configuration (0.2 source candidate)

`UiState<C>` owns validated private `UiSettings<C>` and logical dimensions.
Colors remain typed; configure an explicit `Palette<C>` without RGB565 encoding.
`ResourceView<R>` borrows a provider with validated directory metadata and a
collection/revision `ResourceKey`. Providers guarantee immutable content identity;
metadata validation does not promise successful later I/O/decoding.

Default features are `layout`, `refresh`, `damage`. With `default-features=false`,
base drawing/configuration remain available without layout fields, dirty/session
state or scheduling anchors. `damage` and `frame-pacing` imply `refresh`;
`frame-pacing` is opt-in. Features merge over the entire Cargo dependency graph.

`RefreshSession::new(&mut state, &mut scratch, policy)` validates actual capacity,
starts unbound and requests a full redraw. `try_update` validates the entire
candidate before applying it. Palette/layout/resource changes request a full
redraw; policy-only updates retain existing work. A frame borrows a stable
`UiContext` with full-display bounds; a Tile's bounding box is only its clip.
Context default stack helpers consume padding/gap; explicit drawing overrides
these defaults. Session initializes each tile using the palette background.

`DisplayBinding::new(&mut display, generation)` carries a caller-maintained
non-reused generation. Change it after reset/reinitialization, rotation or device
replacement, including at identical dimensions. A mismatch latches `NeedsRebind`;
`try_rebind` is the only recovery path and requests a full redraw on success.
Scene, write or service failure retains the whole attempted region. Service may
register future damage in its limited sink; both success and failure merge it
only after the current frame. One call never loops over future requests.

`draw` has the same signature under all feature combinations. Under optional
pacing use `draw_timed` with a bounded infallible microsecond clock. Dirty paced
work passed to ordinary `draw` reports `ClockRequired`; it cannot bypass pacing.
Idle, invalid and deferred calls do no rendering or service. Failed starts count
against the interval; clock regression reports an error and starts a new epoch.

`StreamTarget::new` keeps 32-pixel buffering. Explicit capacities use
`StreamTarget::<_, 8>::with_run`; invalid capacities fail compilation. RUN and
refresh tile limits are independent; smaller tile limits do not shrink arrays.

See [design](../../docs/ui-configuration.md) and
[implementation evidence](../../docs/ui-configuration-implementation.md).
`make ui-config` verifies feature consumers, rejected ownership/API uses, ARM
layouts and linked synthetic consumers. This is offline software evidence;
no hardware, product UI, asset codec, storage backend or firmware is included.
