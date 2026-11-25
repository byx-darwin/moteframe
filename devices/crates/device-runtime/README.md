# Portable cooperative runtime

`device-runtime` is a `no_std`, allocation-free, unsafe-forbidden CONTROL foreground coordinator. It owns the existing `ControlConnection` and the factory's actual `ServiceHandler`; it does not reimplement framing, reassembly, replay/cache, authorization, readiness or admission. Persistent owners and trusted policy live outside each application session in the borrowed factory. There is no product metadata, physical backend or permissive production handler in this crate.

## Frozen public API

Exports: `Runtime`, `SessionFactory`, `Transport`, `Work`, `Scope`, `OwnedRx`, `Budgets`, `Progress`, `StepReport`, `RuntimeError`, `PortError`, `RX_CAPACITY` (512).

```rust,ignore
pub trait SessionFactory {
    type Handler<'a>: device_services::ServiceHandler where Self: 'a;
    fn open(&self, scope: Scope)
        -> Result<(device_services::Config<'_>, Self::Handler<'_>), PortError>;
}
pub trait Transport {
    type Queue<'a>: device_connection::FirstFragmentQueue where Self: 'a;
    fn poll(&mut self, now_ms: u64, budget: u8) -> Result<(), PortError>;
    fn scope(&self) -> Result<Option<Scope>, PortError>;
    fn receive(&mut self) -> Result<Option<OwnedRx>, PortError>;
    fn queue(&mut self, expected: Scope) -> Result<Self::Queue<'_>, PortError>;
    fn close_scope(&mut self, old: Scope) -> Result<(), PortError>;
}
pub trait Work {
    fn step(&mut self, now_ms: u64) -> Result<(), PortError>;
}
impl<'a, F: SessionFactory + 'a> Runtime<'a, F> {
    pub const fn new(factory: &'a F, budgets: Budgets) -> Self;
    pub fn step(&mut self, now_ms: u64, transport: &mut impl Transport,
        ui: &mut impl Work, background: &mut impl Work) -> StepReport;
    pub fn shutdown(&mut self, transport: &mut impl Transport) -> Result<(), RuntimeError>;
    pub fn retry_cleanup(&mut self, transport: &mut impl Transport) -> Result<(), RuntimeError>;
}
```

`Scope::new(generation: u64, frame_max: u16) -> Result<Scope, PortError>` requires a nonzero, monotonically increasing actual subscription generation and 20..=512 actual characteristic-value capacity, including the ATT/GATT limit. `Scope::generation()` and `Scope::frame_max()` expose validated values. Disconnect or CCCD disable yields `None`; each subsequent enable advances generation, even on the same physical link. Repeated enable without disable preserves the same scope. Peer IDs or protocol session IDs cannot establish scope. Capacity changes require a new generation.

`OwnedRx::new(scope, bytes) -> Result<OwnedRx, PortError>` copies a nonempty value no larger than that scope's frame maximum into fixed ownership. `OwnedRx::scope()` exposes its origin; payload bytes remain private and Debug shows only scope/length. No Clone implementation duplicates pending ownership. The platform transfers one actual value through `receive`; it must return an explicit error or close on overflow, never silently overwrite/drop live RX. The runtime holds at most one Busy value and stops receiving more while it remains Busy. Stale scoped packets are discarded without entering the current actor. Actual scope is rechecked after RX ownership transfer and before dispatch.

`Budgets::new(rx: u8, tx: u8, transport: u8) -> Result<Budgets, PortError>` rejects zero in every lane. One step calls `poll` once with the chosen transport-unit bound, processes at most `rx` incoming values/pending retries and at most `tx` queue attempts, then calls UI once and background once. A connection or UI failure never skips the other slices. `StepReport` has `connection: Result<Progress, RuntimeError>`, `ui: Result<(), PortError>`, `background: Result<(), PortError>`. `Progress` exposes `received`, `transmitted` and `stale` u8 counters; received counts transfers from transport, transmitted counts actual admitted fragments, not peer receipt. Retrying a retained packet does not increment received.

These budgets bound logical calls, not wall-clock latency. Each port must implement bounded nonblocking slices. A synchronous snapshot/display/transport adapter must service BLE during long waits or supply a qualified latency bound. The scheduler does not preempt an adapter and the offline tests establish logical fairness only; no real Flash/display timing guarantee is claimed.

## Owner and lifecycle integration

`open(&self, scope)` creates a fresh per-subscription handler and immutable Config borrowing stable metadata. An external product factory may hold durable owners behind appropriate serialized borrowing/interior mutability; its handler borrows that factory for the runtime lifetime. Factory error must release any work it started. Factory success transfers the handler to the runtime, including cleanup responsibility if Config validation then fails.

Every open requires a fresh nonzero application `Config.session` strictly greater than all previously accepted IDs in this runtime. IDs come from the actual factory, never a constant runtime identity. `Config` module entries, capability revision, build identity and access policy remain the caller's truthful inputs; merely compiling codecs does not make modules available. The runtime checks RX/TX maxima against the actual scope. It accepts CONTROL-only configurations (`data_max == 0`). Optional events/DATA are explicitly unbound here: their producer/admission owners are not duplicated or inferred. Do not advertise features requiring unbound owners; composition needing events/DATA must bind the existing ownership ports explicitly in a future extension.

A scope change drops pending old RX, closes/mutes the old scoped transport, invokes `ControlConnection::close`/`ServiceHandler::connection_closed`, and drops old actors before constructing a new actor. The new session reuses the factory's persistent owners, not its old protocol Session/cache. Old cleanup must not revoke a newer actual scope. `close_scope` is idempotent and must account for actual old queued RX/TX and platform work.

Failed/uncertain transport or handler cleanup retains ownership and blocks all new actors. Subsequent steps still service transport/UI/background but return `CleanupBlocked`; they never retry fallible handler cleanup implicitly. Call `retry_cleanup` after the actual cause is resolved, then a subsequent step may open a newer scope. If multiple cleanup operations fail, the handler error takes precedence while both ownership boundaries remain blocked and are retried explicitly. Factory-open failure also retains the old transport scope until cleanup succeeds.

Call `shutdown` before destroying the runtime/factory. Rust Drop cannot perform fallible backend cleanup, so destruction alone is not a successful shutdown. No handler cleanup or external owner recovery is claimed by Drop. Explicit shutdown is repeatable. A retired generation cannot reopen, including after publication failure or shutdown.

A backwards `now_ms` closes CONTROL and returns `ClockRegression`. Host/Controller/UI/background receive the last valid monotonic time; a new actor needs a fresh actual generation. Actor wire/readiness/response deadlines continue through the existing `ControlConnection::tick` and `poll_pending` contracts.

## Atomic queue obligations

`Transport::queue(expected)` returns a guard implementing the real existing `FirstFragmentQueue`. The guard must retain the serialized actor boundary until Drop, including the runtime's final `admit_frame`/handler `response_admitted` callback. Actual scope validation, queue reservation, fill and publication share that boundary. Validate expected scope again when reserving/publishing; changed scope is a Contract failure with no clean retry. The runtime rechecks actual handler authorization/replay validity inside fill through `ControlConnection::next_frame` and keeps the guard alive through `admit_frame`.

`NoSlot` means no fill call, no preparation consumption, no `response_admitted` call and no source release. A later real slot retries the existing actor response. `Transport`/`Contract` failure after fill is fatal uncertainty, not FIFO-full; close/mute the old scope and clean up the handler. An adapter must return those outcomes truthfully. This crate cannot prove a platform implementation respects its queue serialization contract, and supplies no fake production implementation.

`PortError` values are `Unavailable`, `InvalidInput`, `Fault`, `Uncertain`; retain detailed backend diagnostics in the adapter. `RuntimeError` values are `Port(PortError)`, `Connection(ConnectionError)`, `ClockRegression`, `StaleScope`, `InvalidConfiguration`, `Publication`, `CleanupBlocked`. UI and background errors are independently visible. An error never claims a failed/uncertain commit succeeded.

## Offline evidence

Twenty integration tests run the actual production protocol/session/connection against test-only queue/executor/handler fixtures. They cover real HELLO, fragmented dispatch/response and replay, NoSlot, Busy RX ownership, stale RX, scope change during RX/publication, uncertain publication, authorization at publication, five CCCD cycles with fresh wire session IDs and retained owner state, deferred readiness/fairness, cleanup retry (including factory-open failure), clock/deadline failures, invalid capacities/configurations, redacted RX and explicit shutdown. The Task1 report records compiler RED separately from genuine assertion RED. Product durable-store/UI evidence belongs to the product composition tests; no physical radio, display or Flash backend was exercised here.
