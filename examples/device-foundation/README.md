# Offline device foundation

An independent consumer of input, UI, source-backed assets, storage views and BLE
value contracts. The library is `no_std`, allocation-free and unsafe-forbidden.
All ports are explicit synthetic models. There is no radio, private protocol,
product screen, health algorithm, original resource or hardware access.

Run from the repository root with prepared dependencies:

```sh
make device-foundation
cargo run --manifest-path examples/device-foundation/Cargo.toml --offline --locked --example replay -- target/foundation.ppm
cargo run --manifest-path examples/device-foundation/Cargo.toml --offline --locked --features small-memory --example replay -- target/foundation-small.ppm
```

The replay's parent output directory must already exist; `make device-foundation`
creates its own output directory. It exports final PPM pixels, behavior counters,
Cortex-M0 layouts, linked sections and local frame evidence under
`target/device-foundation`. See [the complete integration report](../../docs/device-foundation.md).

`UiMemory` owns UI state and pixels. `Demo` borrows it for a long-lived session and
owns `Services`, immutable `Catalog` descriptors and decoder scratch. Display and
reader are supplied per step. A copied visual snapshot stays fixed for a complete
frame; events arriving between tiles request only the next frame.

Services run before/after every frame attempt, including Idle, pacing or errors,
and after each successful tile. Each round polls once, receives at most one whole
value, attempts at most one pending notification, processes one raw sample and
consumes one recognized event (alternating button/touch). Adapters must separately
bound their operation time. Logical work counts are not real-time guarantees.

The caller retains any sample/value rejected by local Full. The service owner
retains a popped sample when a recognized event queue fills. A notification retry
keeps the original payload and target. Unknown acceptance stops resubmission until
explicit resolution using the exact token; it never guesses. Backend capacity
faults require explicit repair/retry. Queue acceptance is not peer delivery.
Generation/characteristic/bearer changes retire old work; a failed poll does not
use a cached route. Real BLE stacks and concurrent ISR access need external adapters.

Default / `small-memory` capacities are actual inline arrays:

| Owner | Default | Small |
| --- | ---: | ---: |
| UI pixels | 32 | 8 |
| Raw samples | 8 | 2 |
| Each recognized event queue | 4 | 1 |
| Single notification / each committed RX slot (bytes) | 16 | 8 |
| Encoded asset scratch (bytes) | 16 | 2 |

The app feature changes capacity; it does not enable extra features on the runtime
crates. Six relative path dependencies, an independent lock and empty default
features keep the example copyable within this repository layout. It is not a
published crate. LICENSE and NOTICE apply to distribution, including commercial use.

Owner generations are supplied by the caller to `Services::new` and must never
be reused for coexisting or reconstructed owners in one recovery domain. Tokens
combine that generation with a checked, nonwrapping sequence; no global allocator
is hidden in the example. Real adapters retire their own old committed values and
allocate a new subscription identity even when disable/re-enable occurs between
polls. The synthetic model's `set_enabled` purges its RX source and advances the
route generation; exhaustion keeps the route disabled.
