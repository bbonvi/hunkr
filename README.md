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
- In-app worktree switcher to move across linked git worktrees without leaving the session
- Built-in git actions are read-only by product design
- In-app shell command modal with streamed output and command history
- Keyboard-first UX with mouse support (drag-select commit ranges with edge auto-scroll, wheel scroll in list panes)
- Responsive behavior on large diff sessions

## Git Safety Model

`hunkr` keeps built-in repository interactions read-only.

- Built-in flows inspect git state and update local `.hunkr/` review metadata only.
- Mutating git actions (for example checkout/switch/merge/rebase/reset/prune) are intentionally out of scope for built-in UI actions.
- Branch switching remains an external git workflow, not a built-in `hunkr` action.

## Worktree Support

- Open the worktree switcher in-app and move between linked worktrees without restarting the session.
- Worktree entries are ordered by latest commit activity (freshest first) while keeping the main repo worktree pinned at the top.
- Review metadata remains anchored to the launch workspace `.hunkr/` directory, so commit statuses/comments stay shared inside that session.

## Local-First Data

All review state stays in your repo under `.hunkr/`:

- `.hunkr/state.json` for commit statuses
- `.hunkr/shell-history.json` for `!` command history
- `.hunkr/comments/index.json` for comment storage
- `.hunkr/comments/<branch>-review-tasks.md` for exported review tasks

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
