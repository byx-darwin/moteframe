# moteframe-display

Caller-configured RGB565 DCS display transport. Supports checked address windows,
64-byte pixel streaming, unchanged-axis caching, dedicated SPI+CS+DC, and shared
SPI transactions. Rendering errors invalidate the panel state until explicit
reinitialization. Initialization command sequences, offsets, power/reset timing
and compatible panel selection belong to the caller.

No product-specific initialization table or assets are bundled. Synchronous
transports must fully finish before returning; shared SPI is foreground-only
and all devices use the same configured mode and frequency.

Runtime: no_std, no allocation, unsafe forbidden. License: Apache-2.0; see LICENSE and NOTICE.
