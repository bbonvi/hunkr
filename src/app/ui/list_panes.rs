use crate::model::{FileChangeKind, ReviewStatus};
use chrono::Utc;
use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState},
};

use super::super::{
    CommitPushChainMarkerKind, CommitRow, CommitStatusFilter, FocusPane, TreeRow, UiTheme,
    blend_colors, commit_push_chain_marker, commit_selection_marker, commit_status_badge,
    commit_status_filter_label_prefix, display_width, format_file_change_badge,
    format_relative_time, list_highlight_symbol, list_highlight_symbol_width,
    sanitize_terminal_text, sanitized_span, truncate, uncommitted_badge,
};
use super::style::{CursorSelectionPolicy, apply_row_highlight, list_content_width, status_style};

const MAX_COMMIT_DECORATION_WIDTH: usize = 40;

/// Renders commit/file list panes so App keeps high-level orchestration only.
pub(in crate::app) struct ListPaneRenderer<'a> {
    theme: &'a UiTheme,
    focused: FocusPane,
    nerd_fonts: bool,
    now_ts: i64,
}

/// Render payload for the files pane.
pub(in crate::app) struct FilePaneModel<'a> {
    pub file_rows: &'a [TreeRow],
    pub changed_files: usize,
    pub shown_files: usize,
    pub search_display: &'a str,
    pub search_enabled: bool,
    pub file_list_state: &'a mut ListState,
}

/// Render payload for the commits pane.
pub(in crate::app) struct CommitPaneModel<'a> {
    pub commits: &'a [CommitRow],
    pub status_counts: (usize, usize, usize, usize),
    pub selected_total: usize,
    pub shown_commits: usize,
    pub total_commits: usize,
    pub status_filter: CommitStatusFilter,
    pub search_display: &'a str,
    pub search_enabled: bool,
    pub commit_list_state: &'a mut ListState,
}

impl<'a> ListPaneRenderer<'a> {
    pub(in crate::app) fn new(theme: &'a UiTheme, focused: FocusPane, nerd_fonts: bool) -> Self {
        Self {
            theme,
            focused,
            nerd_fonts,
            now_ts: Utc::now().timestamp(),
        }
    }

    pub(in crate::app) fn render_files(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        model: FilePaneModel<'_>,
    ) {
        let FilePaneModel {
            file_rows,
            changed_files,
            shown_files,
            search_display,
            search_enabled,
            file_list_state,
        } = model;
        let filter_style = if search_enabled {
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.dimmed)
        };
        let title = Line::from(vec![
            Span::styled(
                " 2 FILES ",
                Style::default()
                    .fg(self.theme.panel_title_fg)
                    .bg(self.theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{shown_files}/{changed_files} shown "),
                Style::default().fg(self.theme.muted),
            ),
            Span::styled("filter:", Style::default().fg(self.theme.muted)),
            sanitized_span(search_display, Some(filter_style)),
        ]);
        let border_style = if self.focused == FocusPane::Files {
            Style::default().fg(self.theme.focus_border)
        } else {
            Style::default().fg(self.theme.border)
        };

        let highlight_symbol = list_highlight_symbol(self.nerd_fonts);
        let width = list_content_width(rect.width, list_highlight_symbol_width(self.nerd_fonts));
        let line_width = width as u16;
        let cursor_idx = file_list_state.selected();
        let visible_rows = rect.height.saturating_sub(2) as usize;
        let file_top = effective_list_top_for_selection(
            cursor_idx,
            file_list_state.offset(),
            visible_rows,
            file_rows.len(),
        );
        let file_age_column_width =
            max_visible_age_width(file_rows, self.now_ts, file_top, visible_rows, |row| {
                row.modified_ts
            });
        let presenter = ListLinePresenter::new(width, self.now_ts, self.theme, self.nerd_fonts)
            .with_age_column_width(file_age_column_width);

        let items: Vec<ListItem<'static>> = file_rows
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let is_cursor = cursor_idx == Some(idx);
                let cursor_bg = if self.focused == FocusPane::Files {
                    self.theme.visual_bg
                } else {
                    self.theme.cursor_bg
                };
                let line = apply_row_highlight(
                    &presenter.file_row_line(row),
                    line_width,
                    false,
                    is_cursor,
                    self.theme.cursor_bg,
                    cursor_bg,
                    CursorSelectionPolicy::BlendCursorOverSelection { weight: 170 },
                );
                ListItem::new(line).style(Style::default())
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style),
            )
            .highlight_style(Style::default())
            .highlight_symbol(highlight_symbol);

        frame.render_stateful_widget(list, rect, file_list_state);
    }

    pub(in crate::app) fn render_commits(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        model: CommitPaneModel<'_>,
    ) {
        let CommitPaneModel {
            commits,
            status_counts,
            selected_total,
            shown_commits,
            total_commits,
            status_filter,
            search_display,
            search_enabled,
            commit_list_state,
        } = model;
        let (unreviewed, reviewed, issue_found, resolved) = status_counts;
        let filter_style = if search_enabled {
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.dimmed)
        };
        let mut title_spans = vec![
            Span::styled(
                " 1 COMMITS ",
                Style::default()
                    .fg(self.theme.panel_title_fg)
                    .bg(self.theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ];
        title_spans.extend(commit_status_count_spans(
            (unreviewed, reviewed, issue_found, resolved),
            self.theme,
            self.nerd_fonts,
        ));
        title_spans.extend([
            chip_separator(),
            Span::styled(
                format_commit_count_chip(shown_commits, total_commits, selected_total),
                Style::default().fg(self.theme.muted),
            ),
        ]);
        if status_filter != CommitStatusFilter::All {
            title_spans.extend([
                chip_separator(),
                Span::styled(
                    commit_status_filter_label_prefix(self.nerd_fonts).to_owned(),
                    Style::default().fg(self.theme.muted),
                ),
                Span::raw(" "),
            ]);
            title_spans.extend(commit_status_filter_spans(
                status_filter,
                self.theme,
                self.nerd_fonts,
            ));
        }
        if search_enabled {
            title_spans.extend([
                chip_separator(),
                sanitized_span(search_display, Some(filter_style)),
            ]);
        }
        let title = Line::from(title_spans);
        let border_style = if self.focused == FocusPane::Commits {
            Style::default().fg(self.theme.focus_border)
        } else {
            Style::default().fg(self.theme.border)
        };

        let highlight_symbol = list_highlight_symbol(self.nerd_fonts);
        let width = list_content_width(rect.width, list_highlight_symbol_width(self.nerd_fonts));
        let line_width = width as u16;
        let cursor_idx = commit_list_state.selected();
        let visible_rows = rect.height.saturating_sub(2) as usize;
        let commit_top = effective_list_top_for_selection(
            cursor_idx,
            commit_list_state.offset(),
            visible_rows,
            commits.len(),
        );
        let commit_age_column_width =
            max_visible_age_width(commits, self.now_ts, commit_top, visible_rows, |row| {
                (!row.is_uncommitted).then_some(row.info.timestamp)
            });
        let presenter = ListLinePresenter::new(width, self.now_ts, self.theme, self.nerd_fonts)
            .with_age_column_width(commit_age_column_width);
        let push_chain_kinds = commit_push_chain_kinds(commits);
        let items: Vec<ListItem<'static>> = commits
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let is_cursor = cursor_idx == Some(idx);
                let push_chain_kind = push_chain_kinds.get(idx).copied().flatten();
                let cursor_bg = if self.focused == FocusPane::Commits {
                    self.theme.visual_bg
                } else {
                    self.theme.cursor_bg
                };
                let line = apply_row_highlight(
                    &presenter.commit_row_line_with_push_chain(row, push_chain_kind),
                    line_width,
                    row.selected,
                    is_cursor,
                    self.theme.cursor_bg,
                    cursor_bg,
                    CursorSelectionPolicy::BlendCursorOverSelection { weight: 170 },
                );
                ListItem::new(line).style(Style::default())
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(border_style),
            )
            .highlight_style(Style::default())
            .highlight_symbol(highlight_symbol);

        frame.render_stateful_widget(list, rect, commit_list_state);
    }
}

/// Builds styled status-filter tokens for the commits pane title.
pub(in crate::app) fn commit_status_filter_spans(
    status_filter: CommitStatusFilter,
    theme: &UiTheme,
    nerd_fonts: bool,
) -> Vec<Span<'static>> {
    match status_filter {
        CommitStatusFilter::All => Vec::new(),
        CommitStatusFilter::UnreviewedOrIssueFound => vec![
            Span::styled(
                commit_status_badge(ReviewStatus::Unreviewed, nerd_fonts).to_owned(),
                status_style(ReviewStatus::Unreviewed, theme),
            ),
            Span::styled(
                commit_status_badge(ReviewStatus::IssueFound, nerd_fonts).to_owned(),
                status_style(ReviewStatus::IssueFound, theme),
            ),
        ],
        CommitStatusFilter::ReviewedOrResolved => vec![
            Span::styled(
                commit_status_badge(ReviewStatus::Reviewed, nerd_fonts).to_owned(),
                status_style(ReviewStatus::Reviewed, theme),
            ),
            Span::styled(
                commit_status_badge(ReviewStatus::Resolved, nerd_fonts).to_owned(),
                status_style(ReviewStatus::Resolved, theme),
            ),
        ],
    }
}

fn commit_status_count_spans(
    status_counts: (usize, usize, usize, usize),
    theme: &UiTheme,
    nerd_fonts: bool,
) -> Vec<Span<'static>> {
    let (unreviewed, reviewed, issue_found, resolved) = status_counts;
    let chips = [
        (ReviewStatus::Unreviewed, unreviewed),
        (ReviewStatus::Reviewed, reviewed),
        (ReviewStatus::IssueFound, issue_found),
        (ReviewStatus::Resolved, resolved),
    ];
    let last_idx = chips.len().saturating_sub(1);
    let mut spans = Vec::new();
    for (idx, (status, count)) in chips.into_iter().enumerate() {
        let token = if nerd_fonts {
            format!("{} {count}", commit_status_badge(status, nerd_fonts))
        } else {
            format!("{}: {count}", commit_status_badge(status, nerd_fonts))
        };
        spans.push(Span::styled(token, status_style(status, theme)));
        if idx < last_idx {
            spans.push(chip_separator());
        }
    }
    spans
}

fn format_commit_count_chip(
    shown_commits: usize,
    total_commits: usize,
    selected_total: usize,
) -> String {
    if selected_total == 0 {
        return format!("{shown_commits}/{total_commits}");
    }
    format!("{shown_commits}/{total_commits}({selected_total})")
}

fn chip_separator() -> Span<'static> {
    Span::raw("  ")
}

/// Presenter for composing list pane rows with shared truncation and age columns.
pub(in crate::app) struct ListLinePresenter<'a> {
    width: usize,
    now_ts: i64,
    theme: &'a UiTheme,
    nerd_fonts: bool,
    age_column_width: usize,
}

impl<'a> ListLinePresenter<'a> {
    pub(in crate::app) fn new(
        width: usize,
        now_ts: i64,
        theme: &'a UiTheme,
        nerd_fonts: bool,
    ) -> Self {
        Self {
            width,
            now_ts,
            theme,
            nerd_fonts,
            age_column_width: 0,
        }
    }

    pub(in crate::app) fn with_age_column_width(mut self, age_column_width: usize) -> Self {
        self.age_column_width = age_column_width;
        self
    }

    pub(in crate::app) fn file_row_line(&self, row: &TreeRow) -> Line<'static> {
        let label = sanitize_terminal_text(&row.label);
        if row.selectable {
            let right = row
                .modified_ts
                .map(|ts| {
                    pad_min_width(format_relative_time(ts, self.now_ts), self.age_column_width)
                })
                .unwrap_or_default();
            let badge = row
                .change
                .as_ref()
                .map(|change| format_file_change_badge(change, self.nerd_fonts))
                .unwrap_or_default();
            let right_width = display_width(&right);
            let badge_width = display_width(&badge);
            let reserved = right_width
                + usize::from(right_width > 0)
                + badge_width
                + usize::from(badge_width > 0);
            let max_label = self.width.saturating_sub(reserved).max(1);
            let left_render = truncate(&label, max_label);
            let static_used = display_width(&left_render)
                + badge_width
                + usize::from(badge_width > 0)
                + right_width;
            let spaces = if static_used >= self.width {
                " ".to_owned()
            } else {
                " ".repeat(self.width - static_used)
            };
            let mut spans = vec![Span::styled(
                left_render,
                Style::default().fg(self.theme.text),
            )];
            if !badge.is_empty() {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    badge,
                    file_change_style(row.change.as_ref().map(|change| change.kind), self.theme),
                ));
            }
            if !spaces.is_empty() {
                spans.push(Span::raw(spaces));
            }
            if !right.is_empty() {
                spans.push(Span::styled(right, Style::default().fg(self.theme.dimmed)));
            }
            Line::from(spans)
        } else {
            Line::from(Span::styled(
                label,
                Style::default()
                    .fg(self.theme.dir)
                    .add_modifier(Modifier::BOLD),
            ))
        }
    }

    #[cfg(test)]
    pub(in crate::app) fn commit_row_line(&self, row: &CommitRow) -> Line<'static> {
        let default_push_chain = if row.is_uncommitted {
            None
        } else if row.info.unpushed {
            Some(CommitPushChainMarkerKind::Unpushed)
        } else {
            Some(CommitPushChainMarkerKind::Pushed)
        };
        self.commit_row_line_with_push_chain(row, default_push_chain)
    }

    pub(in crate::app) fn commit_row_line_with_push_chain(
        &self,
        row: &CommitRow,
        push_chain_kind: Option<CommitPushChainMarkerKind>,
    ) -> Line<'static> {
        let summary = sanitize_terminal_text(&row.info.summary);
        if row.is_uncommitted {
            let marker = commit_selection_marker(row.selected, self.nerd_fonts);
            let left = format!("{marker} {} {summary}", row.info.short_id);
            let badge = uncommitted_badge(self.nerd_fonts);
            let right = "draft";
            let reserved = 1 + display_width(badge) + 1 + display_width(right);
            let max_left = self.width.saturating_sub(reserved).max(1);
            let left_render = truncate(&left, max_left);
            let static_used =
                display_width(&left_render) + display_width(badge) + display_width(right) + 1;
            let spaces = if static_used >= self.width {
                " ".to_owned()
            } else {
                " ".repeat(self.width - static_used)
            };

            return Line::from(vec![
                Span::styled(left_render, Style::default().fg(self.theme.text)),
                Span::raw(" "),
                Span::styled(
                    badge,
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(spaces),
                Span::styled(right, Style::default().fg(self.theme.dimmed)),
            ]);
        }

        let marker = commit_selection_marker(row.selected, self.nerd_fonts);
        let left = format!("{marker} {} {summary}", row.info.short_id);
        let max_right_width = self.width.saturating_sub(1);
        let mut right_spans: Vec<Span<'static>> = Vec::new();
        let mut right_width = 0;
        let decorations = commit_decoration_label(row, self.nerd_fonts);
        if !decorations.is_empty() {
            let remaining =
                max_right_width.saturating_sub(right_width + usize::from(right_width > 0));
            if remaining > 0 {
                let max_decorations = remaining.min(MAX_COMMIT_DECORATION_WIDTH);
                let rendered = truncate(&decorations, max_decorations);
                if right_width > 0 {
                    right_spans.push(Span::raw(" "));
                }
                right_spans.push(Span::styled(
                    rendered.clone(),
                    Style::default().fg(self.theme.accent),
                ));
                right_width += display_width(&rendered) + usize::from(right_width > 0);
            }
        }
        let status_badge = commit_status_badge(row.status, self.nerd_fonts).to_owned();
        let status_needed = display_width(&status_badge) + usize::from(right_width > 0);
        if right_width + status_needed <= max_right_width {
            if right_width > 0 {
                right_spans.push(Span::raw(" "));
            }
            right_spans.push(Span::styled(
                status_badge.clone(),
                status_style(row.status, self.theme).add_modifier(Modifier::BOLD),
            ));
            right_width += status_needed;
        }
        if let Some(push_chain_kind) = push_chain_kind {
            let marker = commit_push_chain_marker(push_chain_kind, self.nerd_fonts).to_owned();
            let needed = 1 + display_width(&marker);
            if right_width + needed <= max_right_width {
                right_spans.push(Span::raw(" "));
                right_spans.push(Span::styled(
                    marker.clone(),
                    commit_push_chain_style(push_chain_kind, self.theme),
                ));
                right_width += needed;
            }
        }
        let age = pad_min_width(
            format_relative_time(row.info.timestamp, self.now_ts),
            self.age_column_width,
        );
        let age_width = display_width(&age);
        if right_width + 1 + age_width <= max_right_width {
            right_spans.push(Span::raw(" "));
            right_spans.push(Span::styled(
                age.clone(),
                Style::default().fg(self.theme.dimmed),
            ));
            right_width += 1 + age_width;
        }
        let max_left = self.width.saturating_sub(right_width + 1).max(1);
        let left_render = truncate(&left, max_left);
        let static_used = display_width(&left_render) + right_width;
        let spaces = if static_used >= self.width {
            " ".to_owned()
        } else {
            " ".repeat(self.width - static_used)
        };
        let mut spans = vec![Span::styled(
            left_render,
            Style::default().fg(self.theme.text),
        )];
        if !spaces.is_empty() {
            spans.push(Span::raw(spaces));
        }
        spans.extend(right_spans);
        Line::from(spans)
    }
}

pub(in crate::app) fn commit_push_chain_kinds(
    commits: &[CommitRow],
) -> Vec<Option<CommitPushChainMarkerKind>> {
    let mut markers = vec![None; commits.len()];
    let top_real = commits.iter().position(|row| !row.is_uncommitted);
    let bottom_real = commits.iter().rposition(|row| !row.is_uncommitted);

    for (idx, row) in commits.iter().enumerate() {
        if row.is_uncommitted {
            continue;
        }
        let kind = if Some(idx) == top_real {
            if row.info.unpushed {
                CommitPushChainMarkerKind::TopUnpushed
            } else {
                CommitPushChainMarkerKind::TopPushed
            }
        } else if Some(idx) == bottom_real {
            if row.info.unpushed {
                CommitPushChainMarkerKind::FirstUnpushed
            } else {
                CommitPushChainMarkerKind::FirstPushed
            }
        } else if row.info.unpushed {
            CommitPushChainMarkerKind::Unpushed
        } else {
            CommitPushChainMarkerKind::Pushed
        };
        markers[idx] = Some(kind);
    }

    markers
}

fn commit_decoration_label(row: &CommitRow, nerd_fonts: bool) -> String {
    if row.info.decorations.is_empty() {
        return String::new();
    }
    let labels = row
        .info
        .decorations
        .iter()
        .map(|item| sanitize_terminal_text(&item.label))
        .collect::<Vec<_>>()
        .join(", ");
    if nerd_fonts {
        format!(" {labels}")
    } else {
        format!("refs:{labels}")
    }
}

fn pad_min_width(value: String, min_width: usize) -> String {
    let width = display_width(&value);
    if width >= min_width {
        return value;
    }

    format!("{}{}", " ".repeat(min_width - width), value)
}

fn max_visible_age_width<T, F>(
    rows: &[T],
    now_ts: i64,
    top: usize,
    visible_rows: usize,
    mut timestamp_of: F,
) -> usize
where
    F: FnMut(&T) -> Option<i64>,
{
    if visible_rows == 0 || top >= rows.len() {
        return 0;
    }

    let end = (top + visible_rows).min(rows.len());
    rows[top..end]
        .iter()
        .filter_map(&mut timestamp_of)
        .map(|ts| display_width(&format_relative_time(ts, now_ts)))
        .max()
        .unwrap_or(0)
}

/// Predicts the effective top list row after selection changes so one-frame layout calculations
/// (like age-column width) stay in sync with jump navigation before widget state is committed.
pub(in crate::app) fn effective_list_top_for_selection(
    selected: Option<usize>,
    current_top: usize,
    visible_rows: usize,
    total_rows: usize,
) -> usize {
    if visible_rows == 0 || total_rows == 0 {
        return 0;
    }

    let max_top = total_rows.saturating_sub(visible_rows);
    let mut top = current_top.min(max_top);
    let Some(selected) = selected else {
        return top;
    };

    let selected = selected.min(total_rows - 1);
    if selected < top {
        top = selected;
    } else {
        let bottom_exclusive = top.saturating_add(visible_rows);
        if selected >= bottom_exclusive {
            top = selected + 1 - visible_rows;
        }
    }

    top.min(max_top)
}

fn file_change_style(kind: Option<FileChangeKind>, theme: &UiTheme) -> Style {
    match kind {
        Some(FileChangeKind::Added) => Style::default().fg(theme.diff_add),
        Some(FileChangeKind::Deleted) => Style::default().fg(theme.diff_remove),
        Some(FileChangeKind::Modified) => Style::default().fg(theme.accent),
        Some(FileChangeKind::Renamed | FileChangeKind::Copied) => {
            Style::default().fg(theme.focus_border)
        }
        Some(FileChangeKind::Unmerged) => Style::default()
            .fg(theme.issue)
            .add_modifier(Modifier::BOLD),
        Some(FileChangeKind::TypeChanged | FileChangeKind::Untracked | FileChangeKind::Unknown) => {
            Style::default().fg(theme.muted)
        }
        None => Style::default().fg(theme.text),
    }
}

fn commit_push_chain_style(kind: CommitPushChainMarkerKind, theme: &UiTheme) -> Style {
    match kind {
        CommitPushChainMarkerKind::FirstUnpushed
        | CommitPushChainMarkerKind::TopUnpushed
        | CommitPushChainMarkerKind::Unpushed => Style::default().fg(theme.muted),
        CommitPushChainMarkerKind::Pushed
        | CommitPushChainMarkerKind::FirstPushed
        | CommitPushChainMarkerKind::TopPushed => {
            Style::default().fg(subdued_pushed_chain_color(theme))
        }
    }
}

fn subdued_pushed_chain_color(theme: &UiTheme) -> Color {
    blend_colors(theme.unpushed, theme.muted, 110)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ThemeMode;

    #[test]
    fn commit_status_count_spans_toggle_separator_by_font_mode() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let counts = [
            (ReviewStatus::Unreviewed, 9),
            (ReviewStatus::Reviewed, 8),
            (ReviewStatus::IssueFound, 7),
            (ReviewStatus::Resolved, 6),
        ];
        let nerd_text = commit_status_count_spans((9, 8, 7, 6), &theme, true)
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let ascii_text = commit_status_count_spans((9, 8, 7, 6), &theme, false)
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(!nerd_text.contains(':'));
        for (status, count) in counts {
            let nerd_token = format!("{} {count}", commit_status_badge(status, true));
            let ascii_token = format!("{}: {count}", commit_status_badge(status, false));

            assert!(nerd_text.contains(&nerd_token));
            assert!(ascii_text.contains(&ascii_token));
        }
    }

    #[test]
    fn format_commit_count_chip_includes_selected_only_when_non_zero() {
        assert_eq!(format_commit_count_chip(165, 165, 0), "165/165");
        assert_eq!(format_commit_count_chip(165, 165, 6), "165/165(6)");
    }
}
