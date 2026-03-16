# hunkr

`hunkr` is built for reviewing stacked, fast-turnaround git changes.

It lets you select one commit, a stack of commits, or your current worktree changes and inspect them in a focused 3-pane workflow. The point is to review what the branch currently means without bouncing between patches or waiting for a pull request.

![hunkr preview](https://github.com/user-attachments/assets/97b17f1a-eb1d-4d65-8383-798dbb93280b)

## Workflow

- Select the commit range you want to review.
- Review the selected changes in a focused 3-pane layout: commits, files, diff.
- Track commit status as `UNREVIEWED`, `REVIEWED`, or `ISSUE_FOUND`.
- Filter commits and files, then resume the same session on restart.

## Safety

- Built-in repository actions are read-only.
- `hunkr` inspects git state and writes only review metadata under `.hunkr/`.
- Branch switching, history editing, merges, rebases, and other mutating git actions stay outside the UI.

## Capabilities

- Review uncommitted worktree/index changes alongside commits.
- Switch between linked git worktrees without restarting.
- Run shell commands in-app with streamed output and command history.
- Use the interface from the keyboard first, with mouse support where it helps.
- Tune behavior and colors with YAML config and theme files.

## Project Data

Review state lives under `.hunkr/` in the repo:

- `.hunkr/state.json` for review status and session state
- `.hunkr/shell-history.json` for `!` command history

## Quick Start

Download the archive for your platform from the [GitHub Releases page](https://github.com/bbonvi/hunkr/releases), unpack it, and run `hunkr`:

```bash
tar -xzf hunkr-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
cd hunkr-vX.Y.Z-x86_64-unknown-linux-gnu
./hunkr
```

Build from source instead:

```bash
cargo run
```

On first run inside a git repository, `hunkr` initializes `.hunkr/`, adds a repository-local exclude rule for it, and starts from a useful initial selection instead of an empty diff.

## Configuration

Optional config file:

- `$XDG_CONFIG_HOME/hunkr/config.yaml`
- `~/.config/hunkr/config.yaml`

Start from [`config.example.yaml`](./config.example.yaml). It covers startup theme, history depth, refresh cadence, diff context, and other runtime behavior.

Optional theme file:

- `$XDG_CONFIG_HOME/hunkr/theme.yaml`
- `~/.config/hunkr/theme.yaml`

Start from [`theme.example.yaml`](./theme.example.yaml) to override the built-in palette. Theme files can omit keys and keep the built-in default for them; unknown keys are ignored with a warning.

## Non-goals

- No built-in checkout, merge, rebase, reset, or other mutating git workflows.
- No requirement to push branches or open pull requests just to review a branch.

## Quality Checks

```bash
cargo fmt --all --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
