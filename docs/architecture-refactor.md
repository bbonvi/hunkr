# App Architecture Refactor Blueprint

This document defines the target architecture for hunkr's TUI runtime.
The current UX flow remains unchanged; only implementation boundaries are refactored.

## Goals

- Remove `App` as a god object by splitting dependencies, domain state, UI state, and runtime control.
- Keep UI rendering and input handling separate from business workflows.
- Introduce dependency-injected ports for infrastructure services.
- Make behavior testable via stable contracts rather than private implementation details.
- Keep forward extension cheap (new panes, modal workflows, data providers, commands).

## Layered Design

### 1) Infrastructure adapters (`infra/*`)

- Git adapter (`GitService`).
- State persistence adapter (`StateStore`).
- Comment persistence/report adapter (`CommentStore`).
- Config adapter (`AppConfig` loader).
- Clock adapter (`SystemClock`).

These implement application-level ports and must not depend on TUI rendering code.

### 2) Application services (`app/services/*`)

- `RepositoryWorkflow`: bootstrap, reload commits, aggregate diff projection, repo switch.
- `StatusWorkflow`: status transition + persistence + report sync.
- `CommentWorkflow`: comment target resolution + create/update/delete + side effects.
- `SessionWorkflow`: snapshot/restore persisted UI session.

Services depend on ports and pure state models, not ratatui widgets.

### 3) State model (`app/state/*`)

- `AppDependencies`: injected ports and long-lived resources.
- `DomainState`: commit rows, aggregate diff, review state, comment data projections.
- `UiState`: pane focus, search state, modal/editor state, theme, cached view anchors.
- `RuntimeState`: redraw/timer/quit/status flags.

`App` becomes a thin coordinator over these grouped states.

### 4) UI controller and rendering (`app/input/*`, `app/ui/*`)

- Input controller maps key/mouse events to app commands.
- Renderers consume view models and paint frames.
- Modal rendering and modal input handling use consistent base behavior.

No infrastructure calls should happen directly in rendering methods.

## Required Ports

- `GitReadPort`
- `GitRepoFactory`
- `ReviewStateRepo`
- `CommentRepo`
- `ConfigProvider`
- `Clock`

All ports must be object-safe for runtime dependency injection.

## Incremental Migration Plan

1. Extract grouped state structs (`AppDependencies`, `DomainState`, `UiState`) and move `App` fields behind them.
2. Add DI port traits and default adapters for existing concrete services.
3. Move repository reload/rebuild/switch flows into `RepositoryWorkflow`.
4. Move status and comment workflows into dedicated services.
5. Extract input/controller boundaries (commands/actions) while keeping keybind behavior.
6. Migrate tests from private internals to public behavior contracts and workflow tests.

## Testing Strategy

- Domain tests: pure functions and state transitions.
- Application workflow tests: command result + side effects at service boundaries.
- Integration tests: git/store/comment adapters against temp FS repos.
- App orchestration tests: driver-level key/mouse/event flows, session persistence, modal isolation.

Avoid brittle tests that lock glyphs, exact style palette, or private helper shape.
