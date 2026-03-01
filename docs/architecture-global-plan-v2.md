# Global Architecture Plan V2

This plan defines the next architecture step for hunkr after the first modular split.
The target remains behavior-preserving for current UX while upgrading scalability and maintainability.

## North-Star Principles

1. Event flow is unidirectional.
2. `App` is a runtime shell, not a workflow owner.
3. Application workflows are isolated from rendering concerns.
4. UI assembly consumes read-only snapshots/view-models.
5. Modal and input behavior follows one shared contract.
6. Tests validate contracts at seams, not private branch shape.
7. Performance is budgeted and observable.

## Phase Plan

### Phase 1: Flow Foundation (`Event -> Action -> Reducer -> Effect`)

- Introduce `app/flow.rs` with:
  - `AppAction` (runtime inputs)
  - `UiAction` (reducer intents)
  - `AppEffect` (side effects)
- Route `handle_event`/`tick` through flow dispatcher.
- Keep pane-level behavior parity by adapting existing handlers as effects.

### Phase 2: Shared Modal/Input Controller

- Introduce `app/input/modal_controller.rs`:
  - base trait for modal key/mouse handling
  - concrete controllers for comment/shell/worktree modes
- Replace ad-hoc modal dispatch in lifecycle input/mouse modules.

### Phase 3: Thin App Shell and Boundaries

- Keep orchestration in `app/services/*` and `app/flow`.
- Keep render/view-only work in `app/ui/*` and `lifecycle_view`.
- Limit cross-layer coupling by reducing direct service calls from render/input modules.

### Phase 4: Driver Harness and Contract Tests

- Add `app/driver.rs` test harness:
  - send event/key/mouse
  - tick
  - snapshot exposed observable state
- Add contract tests using harness for:
  - modal isolation from background panes
  - global key behaviors
  - persisted-flow regressions

### Phase 5: Performance Guardrails

- Track draw timing metrics and budget overages.
- Expose internal metrics API for tests.
- Avoid per-frame allocations in hot render paths.

## Acceptance Criteria

- `cargo clippy -- -D warnings` passes.
- `cargo test` passes.
- No UX regression in current key/mouse/navigation/comment flows.
- Commit pane rendering no longer clones full commit/comment sets per frame.
- New contract tests exist for flow, modal controller, and bootstrap DI seam.

## Execution Status

Completed:
- `Event -> Action -> Effect` runtime flow (`src/app/flow.rs`) is in place.
- Modal input handling is unified behind `ModalInputController`.
- Pane key handling is split by focus (`src/app/input/panes/*`).
- Global normal-mode key routing is extracted (`src/app/input/global_router.rs`).
- Input policy helpers are moved to input layer (`src/app/input/policy.rs`).
- Tick timeout/task policy is extracted (`src/app/runtime/tick_scheduler.rs`).
- Draw latency guardrails are recorded and contract-tested.
- Repository switching uses injected runtime DI ports and has a seam contract test.
- Clock usage in list/worktree render paths now uses injected app clock.

Remaining high-value next slices:
- Extract shell modal state machine into explicit mode subcontrollers.
- Introduce immutable frame snapshot contract for render/view-model builders.
- Remove ratatui primitives (`Line/Span`) from domain diff projections.
