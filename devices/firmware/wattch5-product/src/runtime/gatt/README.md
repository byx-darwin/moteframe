# Concrete pinned TrouBLE CONTROL transport

Enable `runtime-trouble` (which includes `runtime`). `ControlProfile::add` adds a real upstream service and variable-length Write/Notify characteristics to the caller's `AttributeTable`, using caller UUIDs and two caller-owned 20-byte buffers. It checks space for all six actual attributes before mutation. The actual pinned packet pool supports ATT MTU23, so this bridge exposes scope value capacity20 even if a peer offers a larger MTU. Logical WAP messages can be larger and retain their existing protocol fragmentation.

The caller constructs the actual `AttributeServer`, obtains a real accepted `GattConnection`, and keeps these owners outside the transport. The foreground arrangement is:

```rust,ignore
let bridge = Bridge::<2, 2>::new()?; // retain across actual connections
let mut host = core::pin::pin!(runner.run());
let mut gatt = core::pin::pin!(drive_connection(&connection, &profile, &bridge));
let mut transport = TroubleTransport::new(&bridge, host.as_mut(), gatt.as_mut());
// Existing ProductFactory and its actual owners/policy/metadata:
let mut runtime = factory.runtime(budgets);
loop {
    let report = runtime.step(actual_monotonic_ms, &mut transport, &mut ui, &mut background);
    // Handle each fallible lane. Continue polling actual Host/GATT futures.
}
```

Actual advertising/connection acceptance remains caller-owned; this module does not invent product identity, controller startup, trust, Flash partitions, time or critical-section drivers. An accepted connection's driver runs until actual disconnect or a fail-closed ATT/Host/notification error. Keep the Host runner alive and borrow it into a new transport for each actual newly accepted connection; reuse the Bridge so generation numbers never restart underneath a live Runtime. Do not drop a live driver as a cleanup mechanism. Shut down the Runtime explicitly, then allow actual disconnect processing before releasing its connection/driver. Rust Drop does not establish fallible application cleanup success.

`TroubleTransport::poll` alternates Host and GATT future polls across calls; each budget unit polls exactly one lane. The concrete GATT driver handles at most two incoming events and one notification poll before yielding. Pending notification futures retain an owned exact-length frame. The no-op waker assumes a continuously running foreground; no sleep/interrupt-driven executor or wall-clock latency guarantee is supplied. Upstream Host internals and platform storage/display calls remain nonpreemptive.

`Bridge` owns bounded RX/TX FIFOs behind checked `RefCell` borrowing. It is deliberately !Sync and must remain in the same serialized foreground as Runtime and GATT. The queue's `RefMut` guard remains live across reserve, fill, publication and Runtime's `response_admitted`. No async await retains a FIFO borrow. Full TX never invokes fill. Admission means real ordered local FIFO publication, never peer receipt or radio completion. The driver calls actual `Characteristic::notify_raw` with the exact WAP fragment slice and `store=false`; it never pads short fragments.

Actual accepted CCCD transitions create nonzero checked monotonic scope generations. Repeated enable preserves the existing scope. Disable, disconnect, Host failure or explicit close drops old local FIFO work. Closing an errored subscription mutes it while remembering its actual enabled state, so another enable write cannot reopen it; actual off/on or a newly accepted physical connection is required. Old scope close and old driver completion cannot revoke a newer scope. A pending old notification is checked against live scope and actual `should_notify` before every poll, and is canceled on revocation. This matters because upstream `notify_raw` itself returns Ok when unsubscribed. PDUs already admitted into the upstream Host before revocation cannot be retracted through its public API; they are irreversible earlier transport admission, with no peer-receipt promise. Scope retirement prevents new publication and cancels local/unpublished pending work, and does not claim withdrawal of already admitted radio/Host work.

CONTROL RX copies each actual write into owned storage before acknowledging it. Full RX write-with-response gets actual ATT `INSUFFICIENT_RESOURCES` (0x11). A command cannot receive an ATT error, so command overflow requests disconnect and fails the driver explicitly. Malformed/empty/oversized writes are rejected. ATT reply queue failure and notification uncertainty mute/drop the old scope and request disconnect. No live RX is overwritten. Other actual GATT events are processed by the upstream server; there is no independent DATA/event owner.

Offline tests use the actual pinned upstream Host, HCI controller adapter, AttributeServer, GATT events and ATT encoding, with a scripted link backend only in tests. They also run the actual ProductFactory/Runtime, WAP HELLO and persistent settings SET/GET over captured ATT notifications through five CCCD sessions, real durable owners and reconstruction. A deterministic backpressure test additionally pauses Host TX in a test-only future lane, fills the actual upstream outbound queue, and directly revokes its actual per-connection CCCD table: the pending third notification is canceled while the two PDUs already admitted upstream remain. A real upstream allocation failure reports Uncertain and requests actual disconnect. The global default packet pool is protected by a test-only serialization mutex. This is software integration evidence; no air, hardware timing, physical Flash, display or trusted boot/identity qualification is implied.
