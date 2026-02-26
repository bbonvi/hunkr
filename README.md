# hunkr

`hunkr` is a local-first TUI commit reviewer built for fast iteration loops and optimized for agent-generated code changes.

It lets you review a selected commit range as one net diff across all changed files, without opening a pull request.

## Highlights

- Multi-commit net diff review in a focused 3-pane workflow (commits, changed files, diff)
- Fast commit range selection with per-commit review states: `UNREVIEWED`, `REVIEWED`, `ISSUE_FOUND`, `RESOLVED`
- First-run baseline: visible pushed commits are auto-marked `REVIEWED`; unpushed commits stay `UNREVIEWED`
- Inline review comments anchored to commit/hunk/line selections
- Auto-generated review task markdown for agent handoff and follow-up
- Built-in filters/search for commits and files
- Keyboard-first UX with mouse support (drag-select commit ranges with edge auto-scroll, wheel scroll in list panes)
- Local performance optimizations for large diff sessions (viewport-based rendering and bounded, lazy syntax highlighting cache)

## Local-First Data

All review state stays in your repo under `.hunkr/`:

- `.hunkr/state.json` for commit statuses
- `.hunkr/comments/index.json` for comment storage
- `.hunkr/comments/<branch>-review-tasks.md` for exported review tasks

No remote service is required.

## Quick Start

```bash
cargo run
```

On first run in a repository, `hunkr` opens a centered onboarding screen before showing review panes:

- Verifies you are inside a git repository.
- Asks permission to create `.hunkr/` for local review state.
- Offers to append `.hunkr` to the project `.gitignore` (or you can manage it globally).

## Configuration

Optional config file:

- `$XDG_CONFIG_HOME/hunkr/config.yaml`
- `~/.config/hunkr/config.yaml`

See [`config.example.yaml`](./config.example.yaml) for supported options.

## Quality Checks

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
