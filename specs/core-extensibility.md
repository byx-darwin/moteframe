# Core compatibility and resource recovery

Approved direction: preserve the 0.2 source API, add explicit embedded-hal 1.0
bridges and resource reclamation, and measure costs offline. The owner's start
instruction authorizes this implementation, not staging, commits, pushes or hardware.
The detailed assessment remains a local historical artifact under target.

## Scope

- Keep no_std, allocation-free, forbid unsafe, existing dependency pins and Rust 1.96.0.
- Keep Display::Color: Copy independent of graphics; keep RGB565 encoding in display.
- Keep old traits, methods, error variants, geometry, and existing device semantics.
- No dyn migration, DCS codecs, heterogeneous-pin wrappers, new CPU qualification,
  async/IRQ sharing, automatic retry or product code in this first increment.
- New adapters hold only their original HAL object and can return it unchanged.

## HAL bridges

Add core::hal::{HalSpiBus<S>, HalOutputPin<P>, HalDelay<D>} with private fields,
Debug, const new(value) and into_inner(self). Owned and borrowed HAL objects work.
HalSpiBus wraps embedded_hal::spi::SpiBus<u8>, implements SpiWrite by one write
and one attempted flush, even after write failure. Return the write error first,
otherwise flush error. No CS handling and no retries. A concrete HAL must provide
finite waits; a wrapper cannot interrupt a blocked HAL call.
HalOutputPin wraps embedded_hal::digital::OutputPin, preserving high/low and errors.
HalDelay wraps DelayNs and forwards u16 milliseconds as u32 without manual multiplication.
No blanket impls of old traits for arbitrary upstream implementors.

## Ownership recovery

Add SharedBus::into_parts(self)->(B,D), Device::into_select(self)->C, and
SharedPanelBus::into_parts(self)->(Device,P). These methods only move ownership;
they do not perform I/O, repair hardware or clear a live bus fault.

Add SharedBus::try_device(select)->Result<Device,DeviceCreateError<S,C>>.
DeviceCreateError<S,C:OutputPin> holds private Error<S,C::Error> and C, exposes
error(&self)->&Error<S,C::Error> and into_parts(self)->(Error<S,C::Error>,C).
Busy/Faulted/initial CS-high failure return the original pin; Busy/Faulted do no I/O;
initial CS failure latches fault. Old device keeps its signature and consumption.
Use a public TryDeviceResult alias to keep signatures readable.

Reclaim the nested DCS transport, panel device/DC and all CS pins before consuming
the bus. Caller stops/repairs hardware and rebuilds. When used with RefreshSession,
caller changes generation and explicitly rebinds, requesting a full redraw.

## Contracts

Successful Display::write processes exactly the rectangle in row-major order.
After any return the input can be reused; a failing operation may have written only
part of the region. Synchronous transport initiated by a successful call has ended.
No promise of optical visibility, scan completion or a universal present lifecycle.
Keep frame geometry stable. Specify finite-return obligations at I/O boundaries.
DisplayBus remains an 8-bit command/data byte transport with its existing CS/chunk
assumptions, not every display protocol. Result cleanup is not unwind recovery.
ReadAt validates length conversion and the whole range before I/O; errored output
is discarded. An empty read at offset<=capacity succeeds without I/O; beyond capacity
is invalid. This resolves a previously unspecified boundary; no storage backend is added.

## Verification and resource gates

Before production changes, observe missing adapters/recovery fail to compile.
Cover owned/borrowed HALs, write+flush error combinations, maximal delay, no-I/O
ownership moves, creation failures and nested panel reclamation. Preserve first error,
fault latch, wire sequences and old API consumers. Compile-fail probes must reject
for the intended consumer error, using structured diagnostics rather than text search.

Use fixed host/ARM consumers with runtime input and observable checksums. Compare
the identical legacy workload before/after, and a correct manual write+flush bridge
against the new bridge. Separately measure opt-in recovery, including larger pins.
Freeze reference data from revision 8727225c79ffe8d993a1efc4419f42122ae365f1 with
compiler, target, lock/probe hashes and identical compile/link flags. Normal checks
never refresh the baseline; missing/mismatched data or positive legacy cost deltas fail.
Pin the full reference checksum; require complete source provenance including all
workspace manifests. Explicitly set library release code-generation options and
fingerprint manifests and non-transport Cargo configuration. Pure registry/network
mirror configuration must not make otherwise identical hosts incomparable.
Reproducing the baseline from that revision is optional, not a CI history requirement.

Record final linked text/read-only/data/bss, type size/alignment and local function
frames. Reference consumers must not introduce static mutable storage or heap.
Opt-in costs are reported without claiming an accepted product budget. No unmeasured
blanket byte guarantee; no loss of flush/cleanup to achieve a smaller binary.
Single-function frames are not a complete call-chain/IRQ bound or device watermark.
The product root controls profile settings; all measurements specify theirs.

Run full make check/build, source/package boundaries, dependency and secret checks,
spelling and diff checks, then a fresh independent code review. Keep local evidence;
no commit or publication without a separate explicit instruction.
