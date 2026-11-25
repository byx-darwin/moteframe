# Content service integration record

Scope: offline runtime integration in this independent no_std workspace; no heap, unsafe, RTOS, new third-party packages, device operations, staging or commits. The separately frozen device-content crate was reopened only after explicit parent authorization for the previously-unfrozen 06.E80 pattern byte; its full verification was rerun.

## Feature selection and versions

`default = []`. Independent module features are find (06), phone (07), companion-radio (08), notifications (30), media (33 camera/music) and gimbal (33 C34/E82). Optional event-pump selects only a path dependency on device-connection; without a module feature it exposes no runtime adapter. Disabled modules have no backend associated type or concrete fallback. This crate does not duplicate the 16 typed property/object schemas provided by device-content and the root ContentSchemas bridge.

Binding.entries is a sorted, unique trusted catalogue subset containing only this handler's selected modules, availability1, features0, major1/minor0, and actual input/output bounds128..512. Core wire1.2 and body schema2 are separate versions; body schema2 never silently changes a frozen complete contract. Compiled-but-omitted modules cannot be activated through product/UI callbacks.

## Entry points and real ports

- `SharedContentOwner<'data,B,N,C,A,Q>::new(backend)` is the single boot-global owner. N bounds retained action/resource metadata; C notification objects, A call-action tuples, Q immutable queued events. One authenticated application connection can bind it at a time. Connections must have strictly increasing nonzero incarnations.
- `ContentHandler::new(owner, Binding { scope, entries, manifest }, access, clock)` implements the actual device-services ServiceHandler interface. Manifest is required when phone/notifications is selected; it supplies actual codec/product limits. Scope originates in the trusted authenticated transport, not packet fields.
- `Backend` has only feature-selected associated real ports: Actions, Find, Notifications, Radio, RadioState and Gimbal. There are no production simulated/default/always-success implementations.
- `ContentAccess: device_content::AccessPort` supplies actual scope, nonzero authority/provider generation and precise module/opcode/resource grants. Coarse source-port checks and precise grants are both rechecked before success/cache/event release. It must also validate notification committed generations and radio property snapshot generations against their actual stores.
- `ContentClock` supplies actual external monotonic milliseconds. Reversal/failure ends old work. Peer UTC is not used for deadlines or authority.

Module command routes are 06.C30 FIND/C31 STOP/C32 RESULT; 07.C30 heartbeat and candidate C31 SOS result; 08.C30 desired independent-radio switch; 30.C30 actual call-action result; 33.C30 camera mode/C31 camera ACK/C32 local music/C33 remote music ACK/C34 gimbal. Every raw command passes Command::decode before state changes: exact lengths, nonzero identities, bool/enumeration/result dictionaries and frozen body layouts. Public notification/status blob callbacks reuse existing typed decoders.

Unknown/disabled route produces UnsupportedOpcode, omitted module UnsupportedModule. Missing actual backend produces the contract's unsupported response, never a completed action. Malformed input is InvalidArgument; unknown enumeration adds FF08. Missing identity is NotFound. Parameter/desired-state conflict is Conflict/0000; changed immutable terminal result is Conflict/FF07. Wrong lifecycle/lane is InvalidState/FF06. Denial is AccessDenied/FF03; real backend failure HardwareFailure. Insufficient caller output space is local Capacity before mutation. Deferred is an actual local prerequisite, never wire Pending.

## Response-first and FIFO

Dispatch prepares a successful status/body but does not actuate response-first operations. An exact original request/response guard is retained until ServiceHandler.response_admitted; a replay never repeats its side effects. A changed callback identity/body, authority/provider generation or resource generation cannot release old success bytes. Tests exercise the actual Session replay cache and clearing behavior, not just direct handler calls.

One immutable FIFO is shared by every selected content module. Queue-full attempts do not prepare another action; FIND reserves the event slot needed by its accepted response. New responses defer behind queued events. prepare_event returns no eligible event while an original response is pending, and retries return identical token/body bytes. confirm_event is called only after the final fragment entered the same actual FIFO. Exact confirmation repeats are harmless. Events are never dropped or replaced to make room; actual terminal observations remain in their state-machine ledgers until a later queue retry.

Ordinary control event bodies are exact and have no invented CRC or public status prefix:

| Event | Business body |
| --- | --- |
| 06.E80 | find_id:u32,target:u8,pattern:u8,duration_s:u16 =8B |
| 06.E81 | find_id:u32,result:u8 =5B |
| 07.E80 | Empty, still one header-only event frame |
| 07.E81 | event_id:u32,source:u8,utc_ms:i64,time_valid:u8 =14B |
| 30.E82 | notification:u32,action:u8 =5B |
| 33.E80/E81 | event_id:u32,action:u8 =5B |
| 33.E82 | action_id:u32,result:u8,reason:u16 =7B |
| 08.E90 | key:u16,actual_revision:u32 =6B |

06.E80's new 8B definition was explicitly adjudicated while unfrozen: it is ID plus the exact frozen C30 target/pattern/duration block. Target0device/1phone; pattern1vibration/2screen-flash/3both, never sound. Unsupported actual phone screen flash is rejected by the real FindPort capability; there is no implicit default, downgrade or extra undocumented field. Frozen C30 and the 91 complete contracts remain unchanged.

07 and33 have separate body-ID namespaces. Camera and music share the one 33 remote ledger, start at1, never reuse within the connection, and allow only one event awaiting ACK. Local camera-mode/player controls have a separate internal ledger and do not consume 33 remote ID1. 06 find identities and30 notification/action tuples keep their separate contractual scopes. EventToken is an opaque connection/generation/FIFO receipt; it is not a wire message ID. The physical control sender alone allocates ordinary header IDs, independently of DATA lane IDs.

Actual ActionPort.execute/phone FindPort.start/NotificationPort.request_action callbacks occur after final real event admission. Their adapters must bind to that same actual submission and must not enqueue a second copy of the private wire event. Remote action submission must remain AwaitingResult; a fabricated Completed result from merely sending an event is rejected. Camera/music results are0..3; notification actions use their separate1..5 dictionary. Empty successful result ACK only acknowledges receipt of the actual result.

## Device/UI and storage hooks

`request_action` queues actual camera/music/SOS/status requests; `request_find` queues device-originated FIND including its requested pattern. Backend capability validation precedes queueing. Local target FIND starts only after C30 response admission; phone target starts only after E80 final admission. Duration starts at that actual externally timed submission. poll_find/found_device retain real terminal results; STOP waits for its own response admission. A failed actual start/stop is tracked separately as uncertain even if the frozen state machine retains BackendFailure; disconnect reports Unknown and retries the actual stop. Failed cleanup blocks rebinding on the owner. Frozen C32 result1found/2timeout/3cancelled is distinct from candidate E81 result1found/2stop/3timeout/4backend-failure.

configure_heartbeat validates actual manifest bounds. poll_heartbeat derives Waiting/Healthy/Grace/TimedOut from external monotonic time and actual heartbeat reception. Tokens are exact four-byte echoes, not credentials or proof of phone network availability. A status request completes only after phone_status_received decodes an actual P0002 push. There is no automatic fabricated SOS/find action on timeout.

Notification object integration must supply stable committed schema2 bytes with a real nonzero generation to stage_notification_bytes. The bytes remain borrowed for the owner's 'data lifetime: use a pinned immutable committed version, not a mutable reused transfer buffer. The root object handler must call notification_response_admitted after its durable COMMIT success enters the same FIFO; only then may the UI present/alert. Exact COMMIT retry does not alert twice; update does not create a new alert. notification_delete_ready is the actual drain gate before durable deletion; notification_deleted runs after that deletion's response admission, withdraws the real UI and clears call pending/result ledgers. The generation must be checked against the same actual object store; a phone cannot declare its own authorization or committed generation.

ClassicPort extends the frozen real radio control port with observe, so stable-ticket snapshots never overwrite newer independent Classic/link observations. RadioStateStore must be the same real property backend used by configuration's08.P0001. It atomically publishes actual validated readback and returns its real revision/generation; no local revision is manufactured. E90 waits for that actual commit, and a queued E90 drains before another radio generation is published.

GimbalLifecyclePort extends the real controller with readonly ready, idempotent scope-specific cancel_scope and actual drained observation. It compensates for a frozen-state-machine backend-read failure that otherwise marks a private ticket Failed without proving the actuator stopped. Such uncertainty becomes Unknown and triggers actual scope cancellation; a new connection cannot bind until the old scope really drains. Cleanup failure is retained in CleanupReport and permanently blocks new binding on that owner. Successful cancellation alone never proves physical stop. The drain gate applies only to submitted or uncertain jobs; a compiled-but-omitted, unused gimbal never triggers hardware probing or blocks an unrelated module connection.

## Optional EventPump bridge

With event-pump, ContentHost owns the handler cell; ContentRequests implements ServiceHandler and ContentEventSource implements device_connection::EventSource through short checked borrows. with_handler exposes ContentOperations only, not a replaceable mutable handler or DerefMut, so a pending sender cannot be rebound to a new actor's authorization. Call product operations outside a pump call; reentrant borrow failures are explicit and fail closed.

Create one hosting cell, give host.requests() to the control router, and permanently bind host.events() to the physical connection's EventPump/aggregator. The bridge always offers EventPlane::Control and data_message=None. It peeks routing metadata without preparing a body; actual prepare occurs only inside the real FirstFragmentQueue first-slot callback. The final admitted callback confirms the same FIFO token. No ready-boolean transport or production queue implementation is supplied here. The outer serialized executor must coordinate all handlers, cached responses, actual source revocation and FIFO publication; its control_response_pending input must reflect actual control transport state. These are compilation/contract tests, not a claim that a BLE/phone/hardware backend exists in the candidate product.

## Verification and freeze

Run `python3 tools/verify.py`. It records fmt, default/each module/event-pump alone/all-feature Host tests, strict all-target Clippy, Cortex-M0 checks, plus each module with event-pump and all-feature ARM Clippy. Full logs are in target/verification; verification.json records command exit codes, source/dependency hashes and real passing test totals. Behavioral RED logs include red-decoder, red-handler, red-revoked-admission, red-radio-fifo, red-response-fifo, red-ledger-idempotence, red-radio-unknown, red-catalogue-fence, red-heartbeat-poll and red-event-pump, red-find-cleanup, red-omitted-gimbal-drain and red-find-submission. The reopened codec's red-find-pattern-v06 log is retained in device-content/target.

After the final matrix this implementation is frozen for independent review. Real backends, actual stores and the GATT FIFO must be injected before advertising those capabilities. No hardware operation or physical interop was performed.
