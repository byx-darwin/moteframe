# Paired Control Channel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Prevent logical control EVENT/RESP interleaving, preserve real cleanup failures, and advance owned history time from the connection executor.

**Architecture:** One external no-heap, non-Sync SharedControlChannel owns irreversible connection lifecycle, control logical TX state and request-pending state. ControlConnection::with_events validates Config and issues exactly one private non-Clone EventChannel token; EventPump consumes it and reads the same catalogue/active HELLO limits. Control-only construction cannot produce an event token.

**Tech Stack:** Rust1.96, no_std, fixed buffers/Cell, device-protocol/device-services, host plus thumbv6m.

**Spec:** Parent's authorized paired-channel/clock-fanout task and frozen review APP-002/APP-003 in target/application-protocol-review/review-final-application/review-final-application.json. Original review artifacts are immutable.

## Global Constraints

- Only device-connection/**, device-services/**; history only ServiceHandler::advance override; application shared.rs only forwarding and named pump test API adaptations; content-service pump test API adaptation only.
- No heap/unsafe/hardware/Git; no production fake queue/backend.
- Same real first-slot reservation contract; NoSlot cannot prepare/consume IDs/claim transmission.
- Idle/RESP/EVENT control state; partial REQ and handler-deferred block new events; started DATA remains independent.
- Source close invalidates control and control close invalidates event context permanently; explicit cleanup retry remains possible.
- HandlerTable advance attempts every handler even when one fails. Default hook is a true stateless noop, not fabricated backend progress.

## Review Focus

- A complete control EVENT cannot be interrupted by a new REQ/RESP; retain one complete bounded incoming REQ as Deferred until the event drains, with no early business effects.
- A partial incoming REQ owns priority before any new control EVENT; identical next fragment can complete normally.
- NoSlot/NotReady must not strand a logical lock; DATA drain during CANCEL remains possible.
- A token from another connection cannot carry a hand-written catalogue/limits; pre-HELLO emits nothing and negotiated lower limits govern.
- Cleanup errors remain visible/retryable across both close directions, while fresh actual time reaches GRANT before dispatch.

### Task 1: Retry cleanup and fan out actual time

**Files:** connection/src/lib.rs/tests/control.rs; services/src/session.rs/router.rs/tests/router.rs; history/src/lib.rs only override; application/src/shared.rs only forwarding; application/tests/history_pump.rs.
**Interfaces:** ServiceHandler::advance(&mut self,now_ms:u64)->Result<(),SessionError>; existing ControlConnection::tick; HistoryHandler inherent advance.

- [x] Add permanent cleanup retry/retained failure, fanout-all-after-error, and actual CoreGRANT-at60000 regressions; observe real RED before implementation.
- [x] Add default stateless hook, all-handler fanout, ServiceHandle forwarding and history override; invoke before request/dispatch from tick. Persist cleanup error and retry explicit close.
- [x] Run affected complete package tests and inspect outputs; no commits under parent no-Git instruction.

### Task 2: Pair the sole control logical TX owner

**Files:** create connection/src/channel.rs and tests/channel.rs; modify connection/src/lib.rs/pump.rs and existing pump tests.
**Interfaces:** SharedControlChannel::new(); ControlConnection::with_events(Config,&SharedControlChannel)->Result<(Self,EventChannel),ConnectionError>; EventPump::new(EventChannel,Source)->Result<Self,PumpError>.

- [x] Add minimal public type/API shells plus permanent source-bound pairing/peer-reassembly tests; reproduce missing exclusivity RED at runtime.
- [x] Implement unique issuance, same-source catalogue/HELLO limits, request/response/event locks, no-slot rollback and bidirectional close.
- [x] Remove old manual-catalogue/limits EventPump constructor path. Add compile-fail documentation for private token/non-Clone/moved token.
- [x] Prove full EVENT+REQ+RESP peer reassembly; partial REQ priority; NoSlot/NotReady rollback; fresh HELLO limits; DATA parallelism; both close directions.

### Task 3: Adapt existing integration tests to the paired API

**Files:** application/tests/history_pump.rs and content-service/tests/pump.rs only public setup/calls, preserving body/service assertions; connection tests/pump.rs.

- [x] Bind one external owner per test and perform actual HELLO through control before constructing event sender; do not invent active limits/readiness.
- [x] Remove caller-supplied response-pending boolean: pump reads actual paired state.
- [x] Run existing history RECORD42 golden/DONE, source-clock expiry, three content pump cases and all affected package tests.

### Task 4: Verification and stable API delivery

**Files:** connection README/docs and verification evidence inside authorized connection path; services docs as needed.

- [x] Run affected workspace package tests plus independent history/content suites (parent owns full workspace/product matrix), host/ARM strict Clippy, thumbv6m builds and format checks. Root's separate Cargo changes may require coordinating its lock update; never modify unrelated files.
- [x] Record exact commands/results/hashes; self-review against all five focus cases. Report newly exposed unrelated failures explicitly.
- [x] Send parent stable constructor/flush/advance API examples and evidence. Stop new implementation and prepare independent final review.
