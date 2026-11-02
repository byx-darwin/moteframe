# Color Types Implementation Plan

Use executing-plans in the existing independent repository on codex/color-types.
The owner approved the design and explicitly requested implementation and offline
regression; proceed without another design approval. Do not commit/push or use hardware.

**Goal:** Typed colors through the display contract and UI, preserving RGB565 I/O.
**Architecture:** Core associated type, generic UI, concrete driver encoding.
**Tech stack:** Existing pinned Rust/graphics dependencies and Python validators.
**Spec:** `specs/color-types.md`.

## Review focus

- A color without Default or IntoStorage must render without hidden conversion.
- Adjacent RGB888/gray values must survive rather than being quantized to RGB565.
- Failing scene/display/service calls retain dirty state with no further work.
- Invisible input must preserve iterator positions and write order across runs.
- Memory figures distinguish target layout, wire encoding and stack guarantees.

### Task 1: Generalize contracts, drawing and refresh

- [x] Add multi-color behavioral tests and observe missing associated-type/generic
  API failures before changing production code.
- [x] Add Display::Color, remove core RGB565 wrapper, generalize UI and move
  encoding to the retained DCS driver; bump 0.2.0 and migrate existing fixtures.
- [x] Run complete Rust tests, host/ARM lint and builds offline.

### Task 2: Memory accounting and delivery

- [x] Add a repeatable memory-layout probe and regression checks; build host and
  Cortex-M0 evidence and compare the RGB565 renderer to the baseline.
- [x] Update package/boundary allowlists, migration docs, example and scope notes.
- [x] Run make check build, dependency/secret checks and hooks.
- [x] Independent final review; fix blockers test-first and repeat affected gates.
- [x] Record results and limits; leave local changes for review, no hardware/push.
