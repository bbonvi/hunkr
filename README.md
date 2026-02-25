# hunkr

`hunkr` is a local terminal diff reviewer for iterative AI-agent code review.

It is optimized for reviewing **multiple commits together** with commit-aware hunks, file-tree navigation, and explicit per-commit approval tracking.

## Why it exists

When agents produce several commits quickly, reviewing one commit at a time is slow. `hunkr` lets you select a commit range, inspect all changed files, and see hunk provenance (`short_sha + summary`) inline while scrolling one diff stream.

## Core UX

- 3-pane TUI:
  - left/top: changed-file tree (only files/directories changed by selected commits)
  - left/bottom: commit history with multi-select and review status
  - right: syntax-highlighted diff viewer
- Default selection: all **unpushed** commits (`@{upstream}..HEAD` behavior)
- Review flow: approve current commit, all selected commits, or all unreviewed branch commits
- Unreviewed commits are explicitly badged in commit list
- File-switch memory: each file remembers last diff cursor/scroll position
- Mouse support: pane focus, item selection, and wheel scrolling
- Vim-like keys by default
- Inline key hints shown in footer at all times
- Hunk comments: add comments anchored to commit/file/hunk/line and auto-export to Markdown

## Data storage

`hunkr` keeps all local data in:

- `.hunkr/state.json`: persisted approval state
- `.hunkr/comments/<timestamp>-<branch>-review.md`: review comment sessions

This storage is project-local and independent of Git remotes.

## Keybindings

Global:

- `q`: quit
- `Tab` / `h` / `l`: cycle focus between panes
- `f` / `c` / `d`: jump focus to files/commits/diff
- `R`: refresh commits/diffs

Navigation:

- `j` / `k`, arrows: move
- `g` / `G`: top/bottom

Commit pane:

- `space`: toggle commit selection
- `v`: visual range selection (moves select an inclusive range)
- `x`: clear selection
- `a`: approve current commit
- `A`: approve selected commits
- `B`: approve all unreviewed commits on current branch view

Diff pane:

- `Ctrl-d` / `Ctrl-u`: half-page scroll
- `PageDown` / `PageUp`: page scroll
- `m`: add comment at current hunk/line anchor

Comment mode:

- type text, `Enter` to save, `Esc` to cancel

## Build and run

```bash
cargo run
```

## Test and lint

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## Architecture

- `src/main.rs`: terminal lifecycle + event loop
- `src/app.rs`: UI state, key/mouse routing, selection/approval logic, rendering
- `src/git_data.rs`: git commit discovery, unpushed detection, commit-range aggregation
- `src/store.rs`: review state persistence (`.hunkr/state.json`)
- `src/comments.rs`: markdown comment session writer
- `src/model.rs`: shared domain models

## Notes

- The current unpushed strategy follows branch upstream diff and is intentionally extensible.
- Branch approval currently writes per-commit approval records with branch scope metadata.
