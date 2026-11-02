# moteframe-display

Caller-configured RGB565 DCS display transport. Supports checked address windows,
64-byte pixel streaming, unchanged-axis caching, dedicated SPI+CS+DC, and shared
SPI transactions. Rendering errors invalidate the panel state until explicit
reinitialization. Initialization command sequences, offsets, power/reset timing
and compatible panel selection belong to the caller.

The public `Rgb565` re-export is the upstream embedded-graphics-core color type.
`DcsDisplay::Color` is RGB565; MSB-first encoding remains in this crate. It is
not a universal driver for every format accepted by the generic UI.

No product-specific initialization table or assets are bundled. Synchronous
transports must fully finish before returning; shared SPI is foreground-only
and all devices use the same configured mode and frequency.

For explicit recovery, `DcsDisplay::into_bus` returns its transport.
`SharedPanelBus::into_parts` then returns its device and DC pin; take the CS
with `Device::into_select`, and reclaim all devices before taking the shared
controller/delay through `SharedBus::into_parts`. These ownership moves do no
I/O and do not guarantee idle hardware. Board code repairs/reinitializes it;
UI sessions must use a fresh generation and explicitly rebind afterward.

Runtime: no_std, no allocation, unsafe forbidden. License: Apache-2.0; see LICENSE and NOTICE.
