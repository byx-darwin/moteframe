# Core extensibility implementation plan

> **For agentic workers:** Use executing-plans inline. Track each task with test-first evidence and request a fresh final review.

**Goal:** Add compatible HAL bridges and explicit resource recovery with linked cost checks.

**Architecture:** Preserve existing traits and runtime state. Add single-object
HAL newtypes and consuming resource APIs; keep old device construction unchanged.
Use standalone offline consumers for compatibility, wire checksums and ARM costs.

**Tech Stack:** Rust 1.96.0, embedded-hal 1.0.0, Python 3.9+, thumbv6m-none-eabi.

**Spec:** specs/core-extensibility.md

## Global Constraints

- no_std, allocation-free, forbid unsafe; no dependency/toolchain upgrades.
- Existing public traits, methods, error variants and geometry remain compatible.
- Preserve synchronous completion, bounded bottom-layer I/O, first errors and fault latch.
- No hardware, staging, commit, push or publication is authorized.
- Preserve the existing README changes in this checkout.

## Review Focus

- Write error plus flush error: preserve first error, perform cleanup, never resend.
- Construction Busy/Faulted: return the same pin with no pin/SPI operations.
- CS-high failure: return pin, latch fault, allow explicit ownership recovery only.
- Borrowed/large HAL and pin objects: no hidden clone, shared access or unmeasured stack claims.
- Missing/empty/mutated resource evidence: fail rather than report zero cost.

### Task 1: HAL bridges and local contracts

**Files:** create core/src/hal.rs, core/tests/hal.rs; modify core/src/lib.rs,
core/src/read.rs, UI session Drawn documentation and boundary allowlists.

**Interfaces:** HalSpiBus<S>, HalOutputPin<P>, HalDelay<D>; const new and
into_inner; implement existing SpiWrite/OutputPin/DelayMs with original errors.

- [x] Write behavior tests for success, write/flush/both failures, borrowed HALs,
  pin high/low and errors, u16::MAX delay; missing imports must fail first.
- [x] Run cargo test -p moteframe-core --test hal --offline --locked; expect E0432.
- [x] Implement minimal adapters and precise contracts from the spec.
- [x] Run the same tests and core/display tests; expect all pass.
- [x] Update source/package allowlists and run host/ARM lint after integration.

### Task 2: Ownership recovery

**Files:** core/src/spi.rs, core/tests/recovery.rs,
display/src/shared.rs, display/tests/recovery.rs.

**Interfaces:** DeviceCreateError<S,C:OutputPin>, TryDeviceResult; try_device;
Device::into_select, SharedBus::into_parts, SharedPanelBus::into_parts.

- [x] Test all creation paths retain the original pin; resource moves produce no
  I/O; first error/fault remain; nested DCS transport returns CS/DC/controller/delay.
- [x] Run new tests; expect missing methods/types rather than unrelated errors.
- [x] Implement additions while retaining the original device path verbatim.
- [x] Verify tests plus existing shared bus/display/session cases.
- [x] Compile an external consumer rejecting owner consumption followed by use of
  the old device; verify structured consumer-owned E0505 (or exact equivalent).

### Task 3: Compatible consumers and cost gates

**Files:** scripts/probes/core_compat.rs, scripts/check_core_compatibility.py,
scripts/core-compatibility-baseline.json, scripts/tests/test_core_compatibility.py,
Makefile and reviewed-file allowlists.

**Interfaces:** core-compat Make target; immutable JSON reference for legacy
dedicated/shared workloads; reports under target/core-extensibility-implementation.

- [x] Build legacy workloads against a frozen original source before comparing.
- [x] Add failing script regressions for missing/mismatched baselines, changed
  checksums, positive cost delta, empty code and missing frames/layout.
- [x] Implement standalone host/ARM compilation with identical fixed flags;
  legacy modes, manual/adapter owned/borrowed modes and small/large recovery modes.
- [x] Verify deterministic host checksums, linked sections, layouts and local
  frames; record baseline without automatically updating it on normal runs.
- [x] Verify actual rejected ownership consumer diagnostics and a no-graphics
  custom Copy color consumer; mark source/flags/hash mismatches as errors.
- [x] Integrate core-compat into make check; run Python regressions and real gate.

### Task 4: Documentation, full verification and independent review

**Files:** docs/core-extensibility.md, crate READMEs, root README,
docs/progress.md and approved documentation/source allowlists.

- [x] Document adapters, complete recovery, new/old constructor behavior,
  measured legacy/opt-in costs and evidence limitations.
- [x] Run make check build, make supply-chain secrets, typos and git diff --check.
- [x] Confirm existing README content retained and runtime constraints unchanged.
- [x] Dispatch a fresh independent reviewer with spec, plan, diff and evidence.
- [x] Reproduce substantive findings first, fix, rerun relevant and full checks.
- [x] Deliver local changes and measured deltas; retain evidence, do not commit.
