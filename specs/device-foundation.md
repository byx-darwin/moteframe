# Complete offline foundation consumer

The user requested an offline input/UI/assets/BLE composition, full-queue retry,
error recovery, fairness tests and combined memory accounting. This is a standalone
consumer example, not a new public scheduler/protocol crate. No business, vendor,
health algorithms, private resources or hardware are included. The later source
integration request authorizes commit, merge to main and push after offline checks.

## Composition and ownership

Add `examples/device-foundation`, an independent workspace with relative path
dependencies on core/UI/assets/storage/input/BLE only. Keep the eight runtime
crates, their manifests and the root lockfile unchanged. The example library is
no_std, heap-free, unsafe forbidden; only replay/tests use std. No new upstream
dependency. The application owns an empty-default `small-memory` feature:
UI scratch 32/8 RGB565 pixels, raw input queue 8/2, recognized event queues 4/1,
notification value capacity 16/8 bytes, asset byte scratch 16/2. All arrays must
use these constants. Selection changes capacity, not payload semantics.

UiMemory owns state and UI scratch. A long-lived RefreshSession borrows them;
Demo owns that session and separate service, catalog and asset scratch owners.
Caller owns actual Display and mutable ReadAt. Catalog contains immutable raster
descriptors and one UI ResourceKey revision. Use synthetic monochrome glyph and
RGB565 strip bytes via a real storage ReadWindow. No resource format duplication.
Render a 32x16 generic pattern, input selection, RX/TX activity and connection
markers. No product screens, measurements, service UUIDs or application opcodes.

## Bounded scheduling and stable frames

Every outer step runs a service round before UI and another afterward, regardless
of UI Idle/Deferred/NeedsRebind/scene/display failure. Each successful tile also
runs one service round through DeferredInvalidation. A round attempts one BLE
poll, at most one complete RX and one notification, one retained raw input sample,
and at most one recognized event, alternating button/touch queues (including
empty turns, so either queue is served within two rounds). Record original first
error per lane and counts; an error in one lane must not suppress unrelated work.
After a failed BLE poll do not submit/consume RX using an unverified cached route;
input and UI may continue. Each backend operation has a finite adapter budget.
Counts bound logical work, not wall-clock or radio latency.

Each frame uses a copied visual snapshot. Service callbacks can mutate only next
frame state and post invalidation; never change the frame already rendering.
Connection/input changes invalidate the full generic scene, RX/TX activity only
their marker rectangles. Draw at most one frame per outer step. Frame pacing uses
explicit monotonic microseconds; raw input samples retain their original times.

## Input and BLE reliability

enqueue_input accepts or rejects an entire RawSample. Retain one popped raw sample
until Button/Touch accepts it; Full never loses a transition or advances to the
next sample. Other invalid samples return their original error, are explicitly
counted/rejected, and do not permanently block the remaining input stream.

Notifications use one explicit pending slot and opaque owner-generation/sequence token,
not a hidden 512-byte queue. The caller supplies a nonzero owner generation and
never reuses it across coexisting/reconstructed owners in one recovery domain;
the owner allocates checked, nonwrapping sequences. No global allocator is claimed.
Caller retries local Full without overwriting it.
CheckedNotifier uses a borrowed NotificationBackend and validates current capacity.
Only backend errors explicitly classified as definitely not queued may be retried
in a later service round. Uncertain outcome latches the same pending token, prevents
all resubmission, and requires explicit accepted/not-accepted resolution for that
exact token. Validation TooLong is known not submitted and can wait for capacity;
InvalidCapacity is a backend fault requiring explicit retry after repair.
QueueAccepted is never reported as over-air/peer ACK.

Route identity contains generation, characteristic and bearer, including a
nonreused subscription lifetime. The adapter must purge or permanently invalidate
its own retired committed writes, even for disable/re-enable between poll calls.
The model uses an explicit lifecycle method and rejects generation exhaustion. A new poll target,
disconnect or CCCD disable retires old pending TX/RX without relabeling payloads.
Reconnection never sends a queued old-generation value. RX uses a real WriteSlot;
only complete committed values are accepted, including empty values. RX contents
are not interpreted as a product protocol: arrival changes only a generic activity
marker. Notification fixture values are supplied by the replay caller independently.

Explicit display rebind validates the same fixed geometry and new generation,
retains redraw on failure and performs full redraw on success. Read/decode errors
retain the whole UI request; content repair uses a new resource revision. All
container Debug output is metadata-only, including raw sample and pending value.

## Acceptance and evidence

Test real input→state→pixels; exact synthetic asset pixels; raw and recognized
queue pressure; local TX Full and backend Full preserving bytes; uncertain accepted
and rejected recovery without duplication; invalid tokens; stale RX/TX retirement;
capacity races and empty RX. Verify poll/input progress in Idle, Deferred, source
failure, display failure, NeedsRebind and poll/TX failures. Flood RX while input and
notifications progress, preserving stable frame snapshots and next-frame damage.
Assert per-round service bounds and first-error retention.

Replay default/small profiles with the same event trace, compare exact final PPM
pixels, final counters and accepted notification fingerprint. Export artifacts and
machine-readable evidence under root target, not distributed source. Provide a
source-only independent consumer and no hardware claims.

Measure both actual compiled app capacities and Cortex-M0 owner/session/catalog/
backend layouts; fixed host-executed and ARM-linked combined consumers report
Flash, data/bss, local frames, and owned object total without double-counting
borrowed owners. No whole-firmware/IRQ or arbitrary backend stack guarantee.
Keep original core/I2C baselines byte-for-byte. Add precise source/package/example
allowlists, reject dependency/feature drift and invalid evidence, and integrate
default/small host/ARM checks into make check. Run full offline regression and one
independent final review. The user has now supplied the explicit source-integration
request; release creation and hardware access remain outside scope.
