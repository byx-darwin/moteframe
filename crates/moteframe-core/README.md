# moteframe-core

Portable, allocation-free contracts for display geometry, RGB565 pixels,
display/board initialization, GPIO outputs, synchronous SPI writes and exact
offset reads. The `spi` module adapts embedded-hal 1.0 bus/device contracts to a
single foreground shared owner.

Transactions retain exclusive borrowing through CS assertion, payload, flush
and attempted CS release. The first failure is preserved and any transfer or
cleanup error poisons the bus. IRQ sharing, per-device reconfiguration and
automatic recovery are outside the contract.

Runtime: no_std, no allocation, unsafe forbidden. License: Apache-2.0; see LICENSE and NOTICE.
