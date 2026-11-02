# moteframe-core

Portable, allocation-free contracts for display geometry, associated color types,
display/board initialization, GPIO outputs, synchronous SPI writes and exact
offset reads. The `spi` module adapts embedded-hal 1.0 bus/device contracts to a
single foreground shared owner.

`hal::{HalSpiBus, HalOutputPin, HalDelay}` explicitly bridge embedded-hal 1.0
objects, owned or borrowed. Each wrapper holds only its original object and
returns it with `into_inner`. The SPI bridge always attempts `write` then
`flush`, retaining the first error. It wraps a `SpiBus`, not a CS-owning
`SpiDevice`; the platform must provide finite waits.

`Display::Color: Copy` does not depend on a graphics library. Pixel encoding
belongs to the implementing driver. The old RGB565 tuple wrapper is removed in 0.2.

Transactions retain exclusive borrowing through CS assertion, payload, flush
and attempted CS release. The first failure is preserved and any transfer or
cleanup error poisons the bus. IRQ sharing, per-device reconfiguration and
automatic recovery are outside the contract.

`SharedBus::try_device` returns the original CS pin on Busy, Faulted or initial
pin failure through `DeviceCreateError::into_parts`. The older `device` keeps
its signature and consumes the pin on error. `Device::into_select` and
`SharedBus::into_parts` return ownership without I/O or repair; reclaim every
device before consuming the owner, then recover hardware explicitly.

`Display::write` releases the input slice on every return; success means its
logical write and any synchronous transfer have finished, not optical settling.
`ReadAt` validates ranges before I/O; an empty read at or before capacity is a
no-op, while any offset past capacity is invalid. Result cleanup does not
promise recovery from unwinding panics.

See [core extension and resource evidence](../../docs/core-extensibility.md).
`make core-compat` verifies external consumers, rejected owner movement and
fixed ARM linked costs against an immutable legacy reference. These are
synthetic software measurements, not a product memory or hardware guarantee.

Runtime: no_std, no allocation, unsafe forbidden. License: Apache-2.0; see LICENSE and NOTICE.
