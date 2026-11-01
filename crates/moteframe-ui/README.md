# moteframe-ui

Allocation-free embedded-graphics integration over moteframe-core. Offers
geometry clipping, stack layout, small-buffer drawing targets, dirty regions,
and cooperative repaint helpers. The caller supplies a display, scratch buffer,
scene closure and cooperative yield/cancel policy.

`cargo run -p moteframe-ui --example offline --offline --locked -- target/demo.ppm`
renders a procedural host image without external assets or hardware. The host
example uses std; the library does not. Repaint work budgets are logical bounds,
not measured device latency guarantees.

Runtime: no_std, no allocation, unsafe forbidden. License: Apache-2.0; see LICENSE and NOTICE.
