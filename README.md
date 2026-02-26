# hunkr

`hunkr` is a local-first TUI commit reviewer built for fast iteration loops and optimized for agent-generated code changes.

It lets you review a selected commit range as one net diff across all changed files, without opening a pull request.

## Highlights

- Multi-commit net diff review in a focused 3-pane workflow (commits, changed files, diff)
- Fast commit range selection with per-commit review states: `UNREVIEWED`, `REVIEWED`, `ISSUE_FOUND`, `RESOLVED`
- Inline review comments anchored to commit/hunk/line selections
- Auto-generated review task markdown for agent handoff and follow-up
- Built-in filters/search for commits and files
- Keyboard-first UX with mouse support
- Local performance optimizations for large diff sessions (viewport-based rendering and lazy syntax highlighting)

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
