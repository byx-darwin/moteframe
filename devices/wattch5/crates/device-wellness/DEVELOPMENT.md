# Independent wellness implementation record

Scope: only crates/device-wellness. No health,root,old C,PDF or hardware edits. Specification: specs/protocol/v06-contract-proposal.json effective_layout/constraints and shared rri/uv. All new optional schemas remain proposed design decisions; corewire1.2/frozen91 are unchanged.

Plan: concrete2C/2D/2E codecs first,then bounded RRI validation and actual-backend/config/UV lifecycle,then default/single/all feature Host+ARM verification. TDD and verification-before-completion apply. No dependency,production simulated backend,algorithm,authorization authority or product GATT registration.

RED1: independent byte literals for RRI16/25B,women10B,UV7/8/21/22B written before implementation. Expected missing API exports.

## Observed implementation and rulings

- Initial codec tests preceded implementation and failed on missing exports;after concrete codecs,4testsGREEN.
- RRI state/atomic women-port tests preceded API and failed on missing exports. Implemented strict scalar/session/index/delta,fullu64parent,pending admission,pause and overflow behavior. Actual test fixtures remain only tests.
- UV state tests preceded engine API and failed on missing exports. Added two retained event slots,explicit original-ID transitions,cancel-response-before-E81 gating,strict unavailable/no-result ports,monotonic sample/report timing and finite no-wrap IDs.
- Genuine regression RED:finished RRI summary was incorrectly recalculated against a much later query time and returnedExhausted. Moved completed-summary return before live-duration calculation;GREEN,while backward-clock checks remain.
- Ruling: schema1 RRI valid0 cannot distinguish a real baseline from a rejected candidate. Trusted internal BeatClass metadata provides the distinction without changing wire;untrusted wire-only handler quality ingestion is explicitly not implemented. Cost:consumer requiring that distinction needs a separately designed schema/channel,not guessed anchors.
- Ruling: UV fixed cancellation retention never silently evicts known cancelled IDs. Full retention rejects a new alarm cycle withCapacity;active-cycle threshold/enable mutation isBusy. Cost:production requires actual ledger sizing/persistence and a policy migration contract. No full routed-handler claim.
- Ruling: exact2D signed civil-day layout supports the fulli32 day range with checked Gregorian conversions;no unsupported medical date window is invented.
- Added actual backend unavailable/no-data/failed-ACK regressions plus Gregorian endpoints,exact cancelID,and immutable retry tests. Currentall-feature suite18 tests before final matrix.

## Scope / future review

No health,root,C,PDF or device modifications. No actual sensor/algorithm/provider proof,authorization authority,GATT registration,stable persistent ID allocator or durable history is supplied. Full entry-by-entry remaining integration is inREADME;codecs are not counted as complete generic handlers. Final independent review is pending coordinator;author verification does not replace it. Freeze functionality after the final matrix.

Final matrix:23 commands all exit0;allfeatures18 tests/default1;eachfeature Host tests,Host/ARM clippy,ARM checks;Hostdefault/all check;fmt check. Functionality frozen for coordinator fresh review.
