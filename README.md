# hunkr

`hunkr` is a local terminal diff reviewer for iterative AI-agent code review.

It is optimized for reviewing **multiple commits together** with commit-aware hunks, file-tree navigation, and explicit per-commit status tracking.

## Why it exists

When agents produce several commits quickly, reviewing one commit at a time is slow. `hunkr` lets you select a commit range, inspect all changed files, and see hunk provenance (`short_sha + summary`) inline while scrolling one diff stream.

## Core UX

- 3-pane TUI:
  - left/top: changed-file tree (only files/directories changed by selected commits)
  - left/bottom: commit history with multi-select and review status
- right: syntax-highlighted diff viewer
- right pane includes a simple vertical scrollbar for quick position awareness
- Theme modes: dark and light (`dark` by default)
- Initial default selection: unpushed + unreviewed commits; newly arriving commits are highlighted but not auto-selected
- Review statuses: `UNREVIEWED`, `REVIEWED`, `ISSUE_FOUND`, `RESOLVED`
- Leaving a comment automatically marks referenced commit(s) as `ISSUE_FOUND`
- Commit status can be changed from any status to any status
- Commits moved to `REVIEWED` or `RESOLVED` are auto-deselected and their linked comments are auto-cleared
- Unreviewed/issue/resolved/reviewed are explicitly badged in commit list
- File-switch memory: each file remembers last diff cursor/scroll position
- File tree shows relative last-modified time (from latest selected commit touching the file)
- Mouse support: pane focus, item selection, and wheel scrolling
- Vim-like keys by default
- Inline key hints are contextual to the focused pane
- Hunk comments are rendered inline in the diff and can be edited/deleted in place
- Hunk comments can be anchored to commit/file/hunk/line or visual range and auto-export to Markdown
- Commit-header comments are supported in diff viewer (comment directly on commit banner lines)

## Data storage

`hunkr` keeps all local data in:

- `.hunkr/state.json`: persisted commit statuses
- `.hunkr/comments/index.json`: persisted comment index for inline rendering/edit/delete
- `.hunkr/comments/<timestamp>-<branch>-review.md`: review comment sessions
- Markdown events explicitly label target type (`COMMIT` vs `HUNK`)

This storage is project-local and independent of Git remotes.

## Keybindings

Global:

- `q`: quit
- `1` / `2` / `3`: focus files/commits/diff pane
- `Tab` / `Shift-Tab`: cycle focus between all panes
- `h` / `l`: switch focus between file tree and diff only
- `f` / `c` / `d`: jump focus to files/commits/diff
- `t`: toggle theme
- `?`: toggle quick-help overlay
- `F5` / `Ctrl-r`: refresh commits/diffs

Navigation:

- `j` / `k`, arrows: move
- `Ctrl-d` / `Ctrl-u`: big jump down/up in focused pane
- `g` / `G`: top/bottom
- `PageDown` / `PageUp`: page jump

Commit pane:

- `space`: toggle commit selection
- `v`: visual range selection (moves select an inclusive range)
- `x`: clear selection
- `u` / `r` / `i` / `s`: set current commit to `UNREVIEWED` / `REVIEWED` / `ISSUE_FOUND` / `RESOLVED`
- `U` / `R` / `I` / `S`: set all selected commits to target status

Diff pane:

- `Ctrl-d` / `Ctrl-u`: half-page scroll
- `PageDown` / `PageUp`: page scroll
- `v` / `V`: visual line-range selection
- `m`: add comment for current commit/hunk anchor or selected visual range
- `e`: edit comment under cursor
- `D`: delete comment under cursor

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
- `src/app.rs`: UI state, key/mouse routing, selection/status logic, rendering
- `src/git_data.rs`: git commit discovery, unpushed detection, commit-range aggregation
- `src/store.rs`: review state persistence (`.hunkr/state.json`)
- `src/comments.rs`: persisted comment store (`index.json`) + markdown session writer
- `src/model.rs`: shared domain models

## Notes

- The current unpushed strategy follows branch upstream diff and is intentionally extensible.
