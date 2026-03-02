//! Shared UI/diff helper functions used across rendering, navigation, and onboarding flows.
use crate::app::*;
use chrono::DateTime;

pub(super) fn blend_colors(base: Color, overlay: Color, overlay_weight: u8) -> Color {
    match (base, overlay) {
        (Color::Rgb(br, bg, bb), Color::Rgb(or, og, ob)) => {
            let keep_weight = u16::from(255_u8.saturating_sub(overlay_weight));
            let overlay_weight = u16::from(overlay_weight);
            let mix = |base: u8, over: u8| -> u8 {
                (((u16::from(base) * keep_weight) + (u16::from(over) * overlay_weight)) / 255) as u8
            };
            Color::Rgb(mix(br, or), mix(bg, og), mix(bb, ob))
        }
        (_, over) => over,
    }
}

pub(super) fn contains(rect: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

pub(super) fn list_index_at(
    mouse_y: u16,
    rect: ratatui::layout::Rect,
    offset: usize,
) -> Option<usize> {
    if rect.height < 3 {
        return None;
    }
    if mouse_y <= rect.y || mouse_y >= rect.y + rect.height - 1 {
        return None;
    }
    let row = mouse_y.saturating_sub(rect.y + 1) as usize;
    Some(offset + row)
}

pub(super) fn list_drag_scroll_delta(
    mouse_y: u16,
    rect: ratatui::layout::Rect,
    edge_margin: u16,
) -> isize {
    if rect.height < 3 {
        return 0;
    }
    let content_top = rect.y.saturating_add(1);
    let content_bottom = rect.y.saturating_add(rect.height.saturating_sub(2));
    if mouse_y <= content_top.saturating_add(edge_margin) {
        return -1;
    }
    if mouse_y >= content_bottom.saturating_sub(edge_margin) {
        return 1;
    }
    0
}

pub(super) fn list_wheel_event_is_duplicate(
    last_event: Option<(FocusPane, isize, Instant)>,
    pane: FocusPane,
    delta: isize,
    now: Instant,
    min_interval: Duration,
) -> bool {
    if let Some((last_pane, last_delta, last_time)) = last_event {
        return last_pane == pane
            && last_delta == delta
            && now.duration_since(last_time) < min_interval;
    }
    false
}

pub(super) fn diff_index_at(
    mouse_y: u16,
    rect: ratatui::layout::Rect,
    scroll: usize,
    sticky_banner_indexes: &[usize],
) -> Option<usize> {
    if rect.height < 3 {
        return None;
    }
    if mouse_y <= rect.y || mouse_y >= rect.y + rect.height - 1 {
        return None;
    }

    let row = mouse_y.saturating_sub(rect.y + 1) as usize;
    if row < sticky_banner_indexes.len() {
        sticky_banner_indexes.get(row).copied()
    } else {
        Some(scroll + row.saturating_sub(sticky_banner_indexes.len()))
    }
}

pub(super) fn diff_column_at(mouse_x: u16, rect: ratatui::layout::Rect) -> usize {
    if rect.width < 3 {
        return 0;
    }
    let content_left = rect.x.saturating_add(1);
    let max_col = rect.width.saturating_sub(3) as usize;
    mouse_x.saturating_sub(content_left).min(max_col as u16) as usize
}

pub(super) fn diff_visual_from_drag_anchor(
    anchor: Option<usize>,
    cursor: usize,
) -> Option<DiffVisualSelection> {
    let anchor = anchor?;
    (anchor != cursor).then_some(DiffVisualSelection {
        anchor,
        origin: DiffVisualOrigin::Mouse,
    })
}

pub(super) fn should_clear_diff_visual_on_wheel(visual: Option<DiffVisualSelection>) -> bool {
    visual.is_some_and(|selection| selection.origin == DiffVisualOrigin::Keyboard)
}

/// Selection aftermath behavior after a copy action completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectionCopyPostAction {
    ClearNow,
    FlashThenClear(Duration),
}

/// Chooses post-copy selection behavior using one shared policy.
pub(super) fn selection_copy_post_action(
    had_visual_selection: bool,
    flash_without_visual: Option<Duration>,
) -> SelectionCopyPostAction {
    if had_visual_selection {
        return SelectionCopyPostAction::ClearNow;
    }
    flash_without_visual
        .map(SelectionCopyPostAction::FlashThenClear)
        .unwrap_or(SelectionCopyPostAction::ClearNow)
}

/// Formats a unified status line for clipboard copy operations.
pub(super) fn clipboard_copy_status<S, F>(
    result: anyhow::Result<&'static str>,
    success_item: S,
    failure_scope: F,
) -> String
where
    S: AsRef<str>,
    F: AsRef<str>,
{
    match result {
        Ok(backend) => format!("Copied {} via {backend}", success_item.as_ref()),
        Err(err) => format!(
            "Clipboard unavailable for {} ({err:#})",
            failure_scope.as_ref()
        ),
    }
}

pub(super) fn compose_sticky_banner_indexes(
    sticky_file_idx: Option<usize>,
    sticky_commit_idx: Option<usize>,
    viewport_rows: usize,
) -> Vec<usize> {
    let max_sticky = viewport_rows.saturating_sub(1);
    if max_sticky == 0 {
        return Vec::new();
    }

    let mut sticky = Vec::with_capacity(2);
    if let Some(file_idx) = sticky_file_idx {
        sticky.push(file_idx);
    }
    if sticky.len() < max_sticky
        && let Some(commit_idx) = sticky_commit_idx
    {
        sticky.push(commit_idx);
    }
    sticky
}

pub(super) fn truncate(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if display_width(text) <= max_chars {
        return text.to_owned();
    }
    if max_chars == 1 {
        return "…".to_owned();
    }

    let target_width = max_chars.saturating_sub(1);
    let mut out = String::new();
    let mut used_width = 0usize;
    for ch in text.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used_width + width > target_width {
            break;
        }
        out.push(ch);
        used_width += width;
    }
    out.push('…');
    out
}

pub(super) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub(super) fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

pub(super) fn commit_row_matches_query(row: &CommitRow, query: &str) -> bool {
    let status = status_short_label(row.status);
    contains_case_insensitive(&row.info.short_id, query)
        || contains_case_insensitive(&row.info.id, query)
        || contains_case_insensitive(&row.info.summary, query)
        || contains_case_insensitive(&row.info.author, query)
        || row
            .info
            .decorations
            .iter()
            .any(|item| contains_case_insensitive(&item.label, query))
        || contains_case_insensitive(status, query)
}

pub(super) fn commit_row_matches_filter_query(row: &CommitRow, query: &str) -> bool {
    row.is_uncommitted || query.is_empty() || commit_row_matches_query(row, query)
}

/// Returns first-parent history commit IDs that should be baseline-reviewed on first open.
///
/// All visible commits that are already pushed are marked `REVIEWED`; unpushed commits remain
/// `UNREVIEWED` so users can focus on local outgoing work.
pub(super) fn first_open_reviewed_commit_ids(commits: &[CommitInfo]) -> Vec<String> {
    commits
        .iter()
        .filter(|commit| !commit.unpushed)
        .map(|commit| commit.id.clone())
        .collect()
}

pub(super) fn canonical_gitignore_entry(entry: &str) -> String {
    let trimmed = entry.trim();
    let trimmed = trimmed.strip_prefix("./").unwrap_or(trimmed);
    let trimmed = trimmed.trim_start_matches('/');
    trimmed.trim_end_matches('/').to_owned()
}

pub(super) fn gitignore_contains_entry(contents: &str, entry: &str) -> bool {
    let needle = canonical_gitignore_entry(entry);
    if needle.is_empty() {
        return false;
    }

    contents.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return false;
        }
        canonical_gitignore_entry(trimmed) == needle
    })
}

pub(super) fn append_gitignore_entry(path: &Path, entry: &str) -> anyhow::Result<GitignoreUpdate> {
    let canonical = canonical_gitignore_entry(entry);
    if canonical.is_empty() {
        return Ok(GitignoreUpdate::AlreadyPresent);
    }

    let existing = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };
    if gitignore_contains_entry(&existing, &canonical) {
        return Ok(GitignoreUpdate::AlreadyPresent);
    }

    // Re-read right before write so concurrent updates that already added this entry noop.
    let latest = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };
    if gitignore_contains_entry(&latest, &canonical) {
        return Ok(GitignoreUpdate::AlreadyPresent);
    }

    let mut next = latest;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&canonical);
    next.push('\n');
    fs::write(path, next).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(GitignoreUpdate::Added)
}

pub(super) fn matching_file_indices_with_parent_dirs(rows: &[TreeRow], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return rows.iter().enumerate().map(|(idx, _)| idx).collect();
    }

    let mut include = BTreeSet::new();
    let mut ancestor_dirs: Vec<(usize, usize)> = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        if row.selectable {
            while ancestor_dirs
                .last()
                .is_some_and(|(depth, _)| *depth >= row.depth)
            {
                ancestor_dirs.pop();
            }
            if row
                .path
                .as_ref()
                .is_some_and(|path| contains_case_insensitive(path, query))
            {
                include.insert(idx);
                for (_, ancestor_idx) in &ancestor_dirs {
                    include.insert(*ancestor_idx);
                }
            }
            continue;
        }

        while ancestor_dirs
            .last()
            .is_some_and(|(depth, _)| *depth >= row.depth)
        {
            ancestor_dirs.pop();
        }
        ancestor_dirs.push((row.depth, idx));
    }

    include.into_iter().collect()
}

pub(super) fn key_chip(label: &'static str, theme: &UiTheme) -> Span<'static> {
    Span::styled(
        format!(" {} ", label),
        Style::default()
            .fg(theme.panel_title_fg)
            .bg(theme.panel_title_bg)
            .add_modifier(Modifier::BOLD),
    )
}

pub(super) fn status_short_label(status: ReviewStatus) -> &'static str {
    match status {
        ReviewStatus::Unreviewed => "UNREVIEWED",
        ReviewStatus::Reviewed => "REVIEWED",
        ReviewStatus::IssueFound => "ISSUE_FOUND",
        ReviewStatus::Resolved => "RESOLVED",
    }
}

pub(super) fn centered_rect(
    width_percent: u16,
    height_percent: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let vertical = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - height_percent) / 2),
            ratatui::layout::Constraint::Percentage(height_percent),
            ratatui::layout::Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);

    ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - width_percent) / 2),
            ratatui::layout::Constraint::Percentage(width_percent),
            ratatui::layout::Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

pub(super) fn format_relative_time(timestamp: i64, now: i64) -> String {
    let delta = now.saturating_sub(timestamp).max(0);
    if delta < 60 {
        format!("{}s", delta)
    } else if delta < 3_600 {
        format!("{}m", delta / 60)
    } else if delta < 86_400 {
        format!("{}h", delta / 3_600)
    } else if delta < 2_592_000 {
        format!("{}d", delta / 86_400)
    } else if delta < 31_536_000 {
        format!("{}mo", delta / 2_592_000)
    } else {
        format!("{}y", delta / 31_536_000)
    }
}

/// Strip ANSI/control sequences that can mutate terminal state when rendered in the TUI.
///
/// Keeps normal printable text plus newlines/tabs, while removing escape-driven control flows
/// like CSI/OSC and other control bytes.
pub(super) fn sanitize_terminal_text(input: &str) -> String {
    #[derive(Clone, Copy)]
    enum SanitizeState {
        Normal,
        Escape,
        Csi,
        Osc,
        OscEscape,
        St,
        StEscape,
    }

    let mut out = String::with_capacity(input.len());
    let mut state = SanitizeState::Normal;

    for ch in input.chars() {
        state = match state {
            SanitizeState::Normal => match ch {
                '\u{1b}' => SanitizeState::Escape,
                '\u{9b}' => SanitizeState::Csi,
                '\u{9d}' => SanitizeState::Osc,
                '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => SanitizeState::St,
                '\n' | '\t' => {
                    out.push(ch);
                    SanitizeState::Normal
                }
                _ if !ch.is_control() => {
                    out.push(ch);
                    SanitizeState::Normal
                }
                _ => SanitizeState::Normal,
            },
            SanitizeState::Escape => match ch {
                '[' => SanitizeState::Csi,
                ']' => SanitizeState::Osc,
                'P' | 'X' | '^' | '_' => SanitizeState::St,
                _ => SanitizeState::Normal,
            },
            SanitizeState::Csi => {
                if ('@'..='~').contains(&ch) {
                    SanitizeState::Normal
                } else {
                    SanitizeState::Csi
                }
            }
            SanitizeState::Osc => match ch {
                '\u{7}' => SanitizeState::Normal,
                '\u{1b}' => SanitizeState::OscEscape,
                _ => SanitizeState::Osc,
            },
            SanitizeState::OscEscape => match ch {
                '\\' => SanitizeState::Normal,
                '\u{1b}' => SanitizeState::OscEscape,
                _ => SanitizeState::Osc,
            },
            SanitizeState::St => match ch {
                '\u{1b}' => SanitizeState::StEscape,
                _ => SanitizeState::St,
            },
            SanitizeState::StEscape => match ch {
                '\\' => SanitizeState::Normal,
                '\u{1b}' => SanitizeState::StEscape,
                _ => SanitizeState::St,
            },
        };
    }

    out
}

/// Build a terminal-safe span, optionally applying style in one call.
pub(super) fn sanitized_span(text: &str, style: Option<Style>) -> Span<'static> {
    let text = sanitize_terminal_text(text);
    match style {
        Some(style) => Span::styled(text, style),
        None => Span::raw(text),
    }
}

pub(super) fn raw_diff_text(line: &HunkLine) -> String {
    let prefix = match line.kind {
        DiffLineKind::Add => '+',
        DiffLineKind::Remove => '-',
        DiffLineKind::Context => ' ',
        DiffLineKind::Meta => '~',
    };
    format!("{}{}", prefix, sanitize_terminal_text(&line.text))
}

pub(super) fn prune_diff_positions_for_missing_paths(
    diff_positions: &mut HashMap<String, DiffPosition>,
    existing_paths: &BTreeSet<String>,
) {
    diff_positions.retain(|path, _| existing_paths.contains(path));
}

pub(super) fn changed_paths_between_aggregates(
    current: &AggregatedDiff,
    next: &AggregatedDiff,
) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();
    let all_paths = current
        .files
        .keys()
        .chain(current.file_changes.keys())
        .chain(next.files.keys())
        .chain(next.file_changes.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for path in all_paths {
        if current.files.get(&path) != next.files.get(&path)
            || current.file_changes.get(&path) != next.file_changes.get(&path)
        {
            changed.insert(path);
        }
    }

    changed
}

pub(super) fn should_render_commit_banner(
    previous_commit_id: Option<&str>,
    current_commit_id: &str,
) -> bool {
    previous_commit_id != Some(current_commit_id)
}

pub(super) fn push_comment_lines_for_anchor(
    rendered: &mut Vec<RenderedDiffLine>,
    comments: &[&ReviewComment],
    injected_ids: &mut BTreeSet<u64>,
    anchor: &CommentAnchor,
    theme: &UiTheme,
    now_ts: i64,
) {
    for comment in comments {
        if injected_ids.contains(&comment.id) {
            continue;
        }
        if comment_anchor_matches(anchor, &comment.target.end) {
            injected_ids.insert(comment.id);
            push_comment_lines(rendered, comment, theme, now_ts);
        }
    }
}

pub(super) fn push_comment_lines(
    rendered: &mut Vec<RenderedDiffLine>,
    comment: &ReviewComment,
    theme: &UiTheme,
    now_ts: i64,
) {
    let age = comment_age(comment, now_ts);
    let location = comment_location_label(comment);
    let sanitized_comment_text = sanitize_terminal_text(&comment.text);
    rendered.push(RenderedDiffLine {
        line: Line::from(vec![
            Span::styled(
                format!("  [#{}] ", comment.id),
                Style::default()
                    .fg(theme.focus_border)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("[{}] ", comment.target.kind.as_str()),
                Style::default().fg(theme.accent),
            ),
            Span::styled(location, Style::default().fg(theme.muted)),
            Span::raw(" "),
            Span::styled(format!("({})", age), Style::default().fg(theme.dimmed)),
            Span::raw(" "),
            Span::styled("[e edit | D delete]", Style::default().fg(theme.dimmed)),
        ]),
        raw_text: format!("#{} {}", comment.id, sanitized_comment_text),
        anchor: None,
        comment_id: Some(comment.id),
    });

    for text in sanitized_comment_text.lines() {
        rendered.push(RenderedDiffLine {
            line: Line::from(vec![
                Span::styled("       ", Style::default().fg(theme.dimmed)),
                Span::styled(text.to_owned(), Style::default().fg(theme.text)),
            ]),
            raw_text: text.to_owned(),
            anchor: None,
            comment_id: Some(comment.id),
        });
    }
}

pub(super) fn comment_anchor_matches(actual: &CommentAnchor, expected: &CommentAnchor) -> bool {
    actual.commit_id == expected.commit_id
        && actual.file_path == expected.file_path
        && actual.hunk_header == expected.hunk_header
        && actual.old_lineno == expected.old_lineno
        && actual.new_lineno == expected.new_lineno
}

pub(super) fn is_commit_anchor(anchor: &CommentAnchor) -> bool {
    anchor.hunk_header == COMMIT_ANCHOR_HEADER
        && anchor.old_lineno.is_none()
        && anchor.new_lineno.is_none()
}

pub(super) fn comment_targets_commit_end(
    comment: &ReviewComment,
    path: &str,
    commit_id: &str,
) -> bool {
    comment.target.kind == CommentTargetKind::Commit
        && comment.target.end.file_path == path
        && comment.target.end.commit_id == commit_id
}

pub(super) fn comment_targets_hunk_end(
    comment: &ReviewComment,
    path: &str,
    commit_id: &str,
    hunk_header: &str,
) -> bool {
    comment.target.kind == CommentTargetKind::Hunk
        && comment.target.end.file_path == path
        && comment.target.end.commit_id == commit_id
        && comment.target.end.hunk_header == hunk_header
}

pub(super) fn format_anchor_lines(old_lineno: Option<u32>, new_lineno: Option<u32>) -> String {
    match (old_lineno, new_lineno) {
        (Some(old), Some(new)) => format!("old {old}/new {new}"),
        (Some(old), None) => format!("old {old}"),
        (None, Some(new)) => format!("new {new}"),
        (None, None) => "n/a".to_owned(),
    }
}

pub(super) fn comment_location_label(comment: &ReviewComment) -> String {
    if comment.target.kind == CommentTargetKind::Commit {
        let short = comment
            .target
            .end
            .commit_id
            .chars()
            .take(7)
            .collect::<String>();
        return format!("commit {short}");
    }

    let start = format_anchor_lines(
        comment.target.start.old_lineno,
        comment.target.start.new_lineno,
    );
    let end = format_anchor_lines(comment.target.end.old_lineno, comment.target.end.new_lineno);
    if start == end {
        format!("line {start}")
    } else {
        format!("range {start} -> {end}")
    }
}

pub(super) fn comment_age(comment: &ReviewComment, now_ts: i64) -> String {
    let ts = DateTime::parse_from_rfc3339(&comment.updated_at)
        .map(|dt| dt.timestamp())
        .unwrap_or(now_ts);
    format_relative_time(ts, now_ts)
}

pub(super) fn page_step(height: u16, multiplier: f32) -> isize {
    let visible = height.saturating_sub(2).max(1) as f32;
    (visible * multiplier).round() as isize
}

/// Absolute jump target shared by list/diff navigation bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AbsoluteNavTarget {
    Start,
    End,
}

/// Maps common absolute-navigation keys (`g/G`, `Home/End`) to a shared target.
pub(super) fn absolute_nav_target(code: KeyCode) -> Option<AbsoluteNavTarget> {
    match code {
        KeyCode::Char('g') | KeyCode::Home => Some(AbsoluteNavTarget::Start),
        KeyCode::Char('G') | KeyCode::End => Some(AbsoluteNavTarget::End),
        _ => None,
    }
}

pub(super) fn diff_empty_state_message(
    has_rendered_diff: bool,
    changed_files: usize,
    rendered_files: usize,
    file_search_query: &str,
) -> Option<String> {
    if has_rendered_diff || changed_files == 0 || rendered_files > 0 {
        return None;
    }

    let query = file_search_query.trim();
    if query.is_empty() {
        return None;
    }

    Some(format!(
        "Diff hidden: file tree filter /{query} hides all {changed_files} changed file(s)"
    ))
}

pub(super) fn next_poll_timeout(
    refresh_elapsed: Duration,
    relative_elapsed: Duration,
    theme_reload_elapsed: Duration,
    selection_rebuild_in: Option<Duration>,
) -> Duration {
    // Sleep until the earliest maintenance deadline: git auto-refresh or coarse age-label repaint.
    let timeout = AUTO_REFRESH_EVERY
        .saturating_sub(refresh_elapsed)
        .min(RELATIVE_TIME_REDRAW_EVERY.saturating_sub(relative_elapsed))
        .min(THEME_RELOAD_POLL_EVERY.saturating_sub(theme_reload_elapsed));
    if let Some(selection_timeout) = selection_rebuild_in {
        timeout.min(selection_timeout)
    } else {
        timeout
    }
}

pub(super) fn scrolled_diff_position_preserving_offset(
    current: DiffPosition,
    delta: isize,
    max_scroll: usize,
    max_index: usize,
) -> DiffPosition {
    let offset = current.cursor.saturating_sub(current.scroll);
    let delta_abs = delta.saturating_abs() as usize;
    let next_scroll = if delta >= 0 {
        current.scroll.saturating_add(delta_abs)
    } else {
        current.scroll.saturating_sub(delta_abs)
    }
    .min(max_scroll);

    DiffPosition {
        scroll: next_scroll,
        cursor: next_scroll.saturating_add(offset).min(max_index),
    }
}

/// Computes the next diff scroll top while preserving a Vim-style `scrolloff` cursor gutter.
///
/// The returned scroll keeps `cursor` at least `scrolloff` rows away from the viewport edges when
/// possible, while clamping the gutter for very small viewports where both edges cannot be
/// satisfied at once.
pub(super) fn diff_scroll_with_scrolloff(
    cursor: usize,
    current_scroll: usize,
    visible_rows: usize,
    scrolloff: usize,
) -> usize {
    let rows = visible_rows.max(1);
    let effective_scrolloff = scrolloff.min(rows.saturating_sub(1) / 2);
    let top_threshold = current_scroll.saturating_add(effective_scrolloff);

    if cursor < top_threshold {
        return cursor.saturating_sub(effective_scrolloff);
    }

    let bottom_threshold = current_scroll
        .saturating_add(rows.saturating_sub(1))
        .saturating_sub(effective_scrolloff);
    if cursor > bottom_threshold {
        return cursor
            .saturating_add(effective_scrolloff.saturating_add(1))
            .saturating_sub(rows);
    }

    current_scroll
}

/// Computes a list scroll offset that preserves the cursor's viewport row after a list mutation.
///
/// `prior_selected` and `prior_top` define the cursor row before mutation. `next_selected` is the
/// selected row after mutation/clamping. Returns `None` when there was no prior cursor anchor.
pub(super) fn list_scroll_preserving_cursor_to_top_offset(
    prior_selected: Option<usize>,
    prior_top: usize,
    next_selected: Option<usize>,
) -> Option<usize> {
    let cursor_to_top_offset = prior_selected?.saturating_sub(prior_top);
    Some(next_selected?.saturating_sub(cursor_to_top_offset))
}

pub(super) fn focus_with_h(current: FocusPane) -> FocusPane {
    match current {
        FocusPane::Commits => FocusPane::Diff,
        FocusPane::Files => FocusPane::Commits,
        FocusPane::Diff => FocusPane::Files,
    }
}

pub(super) fn focus_with_l(current: FocusPane) -> FocusPane {
    match current {
        FocusPane::Commits => FocusPane::Files,
        FocusPane::Files => FocusPane::Diff,
        FocusPane::Diff => FocusPane::Commits,
    }
}
