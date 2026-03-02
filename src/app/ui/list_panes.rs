use std::collections::BTreeSet;

use crate::model::{CommitDecoration, CommitDecorationKind, FileChangeKind, ReviewStatus};
use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState},
};

use super::super::{
    CommitPushChainMarkerKind, CommitRow, CommitStatusFilter, FocusPane, TreeRow, UiTheme,
    blend_colors, commit_comment_badge, commit_push_chain_marker, commit_selection_marker,
    commit_status_badge, commit_status_filter_label_prefix, display_width,
    format_file_change_badge, format_relative_time, list_highlight_symbol,
    list_highlight_symbol_width, sanitize_terminal_text, sanitized_span, truncate,
    uncommitted_badge,
};
use super::style::{CursorSelectionPolicy, apply_row_highlight, list_content_width, status_style};

const MAX_COMMIT_LINE_DECORATION_WIDTH: usize = 30;

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
    pub comment_badge_commit_ids: &'a BTreeSet<String>,
    pub status_counts: (usize, usize, usize),
    pub selected_total: usize,
    pub shown_commits: usize,
    pub total_commits: usize,
    pub status_filter: CommitStatusFilter,
    pub search_display: &'a str,
    pub search_enabled: bool,
    pub commit_list_state: &'a mut ListState,
}

impl<'a> ListPaneRenderer<'a> {
    pub(in crate::app) fn new(
        theme: &'a UiTheme,
        focused: FocusPane,
        nerd_fonts: bool,
        now_ts: i64,
    ) -> Self {
        Self {
            theme,
            focused,
            nerd_fonts,
            now_ts,
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
            chip_separator(),
            Span::styled(
                format!("{shown_files}/{changed_files}"),
                Style::default().fg(self.theme.muted),
            ),
        ]);
        let title = if search_enabled {
            let mut spans = title.spans;
            spans.extend([
                chip_separator(),
                sanitized_span(search_display, Some(filter_style)),
            ]);
            Line::from(spans)
        } else {
            title
        };
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
                    self.theme.focused_cursor_bg
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
                    CursorSelectionPolicy::BlendCursorOverSelection {
                        weight: self.theme.cursor_visual_overlap_weight,
                    },
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
            comment_badge_commit_ids,
            status_counts,
            selected_total,
            shown_commits,
            total_commits,
            status_filter,
            search_display,
            search_enabled,
            commit_list_state,
        } = model;
        let (unreviewed, reviewed, issue_found) = status_counts;
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
            (unreviewed, reviewed, issue_found),
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
        let selected_commit_bg = commit_selection_background(self.theme);
        let items: Vec<ListItem<'static>> = commits
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let is_cursor = cursor_idx == Some(idx);
                let push_chain_kind = push_chain_kinds.get(idx).copied().flatten();
                let has_comments = comment_badge_commit_ids.contains(&row.info.id);
                let cursor_bg = if self.focused == FocusPane::Commits {
                    self.theme.focused_cursor_bg
                } else {
                    self.theme.cursor_bg
                };
                let line = apply_row_highlight(
                    &presenter.commit_row_line_with_push_chain(row, push_chain_kind, has_comments),
                    line_width,
                    row.selected,
                    is_cursor,
                    selected_commit_bg,
                    cursor_bg,
                    CursorSelectionPolicy::BlendCursorOverSelection {
                        weight: self.theme.cursor_visual_overlap_weight,
                    },
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
        CommitStatusFilter::Reviewed => vec![Span::styled(
            commit_status_badge(ReviewStatus::Reviewed, nerd_fonts).to_owned(),
            status_style(ReviewStatus::Reviewed, theme),
        )],
    }
}

fn commit_status_count_spans(
    status_counts: (usize, usize, usize),
    theme: &UiTheme,
    nerd_fonts: bool,
) -> Vec<Span<'static>> {
    let (unreviewed, reviewed, issue_found) = status_counts;
    let chips = [
        (ReviewStatus::Unreviewed, unreviewed),
        (ReviewStatus::Reviewed, reviewed),
        (ReviewStatus::IssueFound, issue_found),
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
        self.commit_row_line_with_push_chain(row, default_push_chain, false)
    }

    pub(in crate::app) fn commit_row_line_with_push_chain(
        &self,
        row: &CommitRow,
        push_chain_kind: Option<CommitPushChainMarkerKind>,
        has_comments: bool,
    ) -> Line<'static> {
        let commit_text_style = if row.selected {
            Style::default()
                .fg(self.theme.text)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.text)
        };
        let decoration_style = commit_decoration_style(row.selected, self.theme);
        let age_style = if row.selected {
            Style::default()
                .fg(self.theme.dimmed)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.theme.dimmed)
        };
        let summary = sanitize_terminal_text(&row.info.summary);
        if row.is_uncommitted {
            let marker = commit_selection_marker(row.selected, self.nerd_fonts);
            let left = format!("{marker} {summary}");
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
                Span::styled(left_render, commit_text_style),
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
        let left = format!("{marker} {summary}");
        let max_right_width = self.width.saturating_sub(1);
        let mut right_spans: Vec<Span<'static>> = Vec::new();
        let mut right_width = 0;
        let decorations =
            compact_commit_line_decoration_label(&row.info.decorations, self.nerd_fonts);
        if !decorations.is_empty() {
            let remaining =
                max_right_width.saturating_sub(right_width + usize::from(right_width > 0));
            if remaining > 0 {
                let max_decorations = remaining.min(MAX_COMMIT_LINE_DECORATION_WIDTH);
                let rendered = truncate(&decorations, max_decorations);
                if right_width > 0 {
                    right_spans.push(Span::raw(" "));
                }
                right_spans.push(Span::styled(rendered.clone(), decoration_style));
                right_width += display_width(&rendered) + usize::from(right_width > 0);
            }
        }
        if has_comments {
            let comment_badge = commit_comment_badge(self.nerd_fonts).to_owned();
            let comment_needed = display_width(&comment_badge) + usize::from(right_width > 0);
            if right_width + comment_needed <= max_right_width {
                if right_width > 0 {
                    right_spans.push(Span::raw(" "));
                }
                right_spans.push(Span::styled(
                    comment_badge,
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ));
                right_width += comment_needed;
            }
        }
        if commit_has_tag(row) {
            let tag_badge = if self.nerd_fonts {
                "".to_owned()
            } else {
                "tag".to_owned()
            };
            let tag_needed = display_width(&tag_badge) + usize::from(right_width > 0);
            if right_width + tag_needed <= max_right_width {
                if right_width > 0 {
                    right_spans.push(Span::raw(" "));
                }
                right_spans.push(Span::styled(tag_badge, decoration_style));
                right_width += tag_needed;
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
            right_spans.push(Span::styled(age.clone(), age_style));
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
        let mut spans = vec![Span::styled(left_render, commit_text_style)];
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

fn compact_ref_metadata_tokens(decorations: &[CommitDecoration], nerd_fonts: bool) -> Vec<String> {
    let mut head_refs = Vec::<String>::new();
    let mut local_refs = Vec::<String>::new();
    let mut remote_refs = Vec::<String>::new();
    let mut tag_refs = Vec::<String>::new();
    for item in decorations {
        let label = sanitize_terminal_text(&item.label);
        match item.kind {
            CommitDecorationKind::Head => head_refs.push(label),
            CommitDecorationKind::LocalBranch => local_refs.push(label),
            CommitDecorationKind::RemoteBranch => {
                if !label.ends_with("/HEAD") {
                    remote_refs.push(label);
                }
            }
            CommitDecorationKind::Tag => {
                tag_refs.push(label.trim_start_matches("tag: ").to_owned());
            }
        }
    }

    let head_bases = head_refs
        .iter()
        .filter_map(|label| label.strip_suffix('*').map(str::to_owned))
        .collect::<BTreeSet<_>>();
    local_refs.retain(|label| !head_bases.contains(label));

    let local_head_refs = dedupe_in_order(head_refs.into_iter().chain(local_refs).collect());
    let remote_refs = dedupe_in_order(remote_refs);
    let tag_refs = dedupe_in_order(tag_refs);

    let mut tokens = Vec::new();
    if let Some(token) = summarize_ref_group("refs ", &local_head_refs, 2) {
        tokens.push(token);
    }
    if let Some(token) = summarize_ref_group("", &remote_refs, 1) {
        tokens.push(token);
    }
    if let Some(token) = summarize_ref_group(if nerd_fonts { " " } else { "tag " }, &tag_refs, 2)
    {
        tokens.push(token);
    }
    tokens
}

fn compact_commit_line_decoration_label(
    decorations: &[CommitDecoration],
    nerd_fonts: bool,
) -> String {
    let mut head_refs = Vec::<String>::new();
    let mut local_refs = Vec::<String>::new();
    let mut remote_refs = Vec::<String>::new();
    for item in decorations {
        let label = sanitize_terminal_text(&item.label);
        match item.kind {
            CommitDecorationKind::Head => head_refs.push(label),
            CommitDecorationKind::LocalBranch => local_refs.push(label),
            CommitDecorationKind::RemoteBranch => {
                if !label.ends_with("/HEAD") {
                    remote_refs.push(label);
                }
            }
            CommitDecorationKind::Tag => {}
        }
    }

    let head_bases = head_refs
        .iter()
        .filter_map(|label| label.strip_suffix('*').map(str::to_owned))
        .collect::<BTreeSet<_>>();
    local_refs.retain(|label| !head_bases.contains(label));

    let local_head_refs = dedupe_in_order(head_refs.into_iter().chain(local_refs).collect());
    let remote_refs = dedupe_in_order(remote_refs);

    let mut labels = Vec::<String>::new();
    let shown_locals = local_head_refs.iter().take(2).cloned().collect::<Vec<_>>();
    let local_overflow = local_head_refs.len().saturating_sub(shown_locals.len());
    labels.extend(shown_locals);
    if local_overflow > 0 {
        labels.push(format!("+{local_overflow}"));
    }

    if let Some(remote) = remote_refs.first() {
        labels.push(format!("@{remote}"));
        if remote_refs.len() > 1 {
            labels.push(format!("+{}", remote_refs.len() - 1));
        }
    }

    if labels.is_empty() {
        return String::new();
    }

    if nerd_fonts {
        format!(" {}", labels.join(","))
    } else {
        format!("refs:{}", labels.join(","))
    }
}

fn summarize_ref_group(prefix: &str, refs: &[String], max_items: usize) -> Option<String> {
    if refs.is_empty() {
        return None;
    }

    let shown = refs.iter().take(max_items).cloned().collect::<Vec<_>>();
    let overflow = refs.len().saturating_sub(shown.len());
    let value = if overflow == 0 {
        shown.join(",")
    } else {
        format!("{},+{overflow}", shown.join(","))
    };
    Some(format!("{prefix}{value}"))
}

fn dedupe_in_order(labels: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::<String>::new();
    let mut deduped = Vec::new();
    for label in labels {
        if seen.insert(label.clone()) {
            deduped.push(label);
        }
    }
    deduped
}

fn commit_has_tag(row: &CommitRow) -> bool {
    row.info
        .decorations
        .iter()
        .any(|item| item.kind == CommitDecorationKind::Tag)
}

fn commit_decoration_style(selected: bool, theme: &UiTheme) -> Style {
    if selected {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.accent)
    }
}

fn pad_min_width(value: String, min_width: usize) -> String {
    let width = display_width(&value);
    if width >= min_width {
        return value;
    }

    format!("{}{}", " ".repeat(min_width - width), value)
}

pub(in crate::app) fn focused_commit_metadata_summary(
    row: Option<&CommitRow>,
    nerd_fonts: bool,
) -> String {
    match row {
        Some(row) if row.is_uncommitted => "worktree/index draft snapshot".to_owned(),
        Some(row) => {
            let mut meta_parts = vec![
                sanitize_terminal_text(&row.info.short_id),
                format_commit_datetime(row.info.timestamp),
                sanitize_terminal_text(row.info.author.trim()),
                if row.info.unpushed {
                    "unpushed".to_owned()
                } else {
                    "pushed".to_owned()
                },
            ];
            let refs = compact_ref_metadata_tokens(&row.info.decorations, nerd_fonts);
            if refs.is_empty() {
                meta_parts.push("refs -".to_owned());
            } else {
                meta_parts.extend(refs);
            }
            meta_parts.join("  ")
        }
        None => "no commit selected".to_owned(),
    }
}

fn format_commit_datetime(timestamp: i64) -> String {
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "?".to_owned())
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

fn commit_selection_background(theme: &UiTheme) -> Color {
    theme.commit_selected_bg
}

fn subdued_pushed_chain_color(theme: &UiTheme) -> Color {
    blend_colors(theme.unpushed, theme.muted, 110)
}

#[cfg(test)]
mod tests {
    use crate::app::ThemeMode;
    use crate::app::*;

    #[test]
    fn commit_status_count_spans_toggle_separator_by_font_mode() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let counts = [
            (ReviewStatus::Unreviewed, 9),
            (ReviewStatus::Reviewed, 8),
            (ReviewStatus::IssueFound, 7),
        ];
        let nerd_text = super::commit_status_count_spans((9, 8, 7), &theme, true)
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let ascii_text = super::commit_status_count_spans((9, 8, 7), &theme, false)
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
        assert_eq!(super::format_commit_count_chip(165, 165, 0), "165/165");
        assert_eq!(super::format_commit_count_chip(165, 165, 6), "165/165(6)");
    }

    #[test]
    fn selected_commit_row_text_is_bold() {
        let theme = UiTheme::from_mode(ThemeMode::Light);
        let presenter = super::ListLinePresenter::new(120, 1_710_000_000, &theme, false);
        let row = CommitRow {
            info: crate::model::CommitInfo {
                id: "abc123".to_owned(),
                short_id: "abc123".to_owned(),
                summary: "Make selected commits bold".to_owned(),
                author: "dev".to_owned(),
                timestamp: 1_709_999_000,
                unpushed: true,
                decorations: Vec::new(),
            },
            selected: true,
            status: ReviewStatus::Unreviewed,
            is_uncommitted: false,
        };

        let line = presenter.commit_row_line(&row);
        assert!(
            line.spans
                .first()
                .is_some_and(|span| span.style.add_modifier.contains(Modifier::BOLD))
        );
    }

    #[test]
    fn selected_commit_row_tag_marker_and_age_are_bold() {
        let theme = UiTheme::from_mode(ThemeMode::Light);
        let presenter = super::ListLinePresenter::new(120, 1_710_000_000, &theme, false);
        let row = CommitRow {
            info: crate::model::CommitInfo {
                id: "def456".to_owned(),
                short_id: "def456".to_owned(),
                summary: "Style selected metadata".to_owned(),
                author: "dev".to_owned(),
                timestamp: 1_709_999_000,
                unpushed: false,
                decorations: vec![crate::model::CommitDecoration {
                    kind: crate::model::CommitDecorationKind::Tag,
                    label: "v1.0.0".to_owned(),
                }],
            },
            selected: true,
            status: ReviewStatus::Reviewed,
            is_uncommitted: false,
        };

        let line = presenter.commit_row_line_with_push_chain(&row, None, false);
        assert!(line.spans.iter().any(|span| {
            span.content.contains("tag")
                && span.style.add_modifier.contains(Modifier::BOLD)
                && span.style.fg == Some(theme.accent)
        }));
        assert!(
            !line
                .spans
                .iter()
                .any(|span| span.content.contains("refs:") || span.content.contains("v1.0.0"))
        );
        assert!(line.spans.iter().any(|span| {
            span.style.fg == Some(theme.dimmed)
                && !span.content.trim().is_empty()
                && span.style.add_modifier.contains(Modifier::BOLD)
        }));
    }

    #[test]
    fn focused_commit_metadata_uses_selected_commit_context() {
        let row = CommitRow {
            info: crate::model::CommitInfo {
                id: "abc123".to_owned(),
                short_id: "abc1234".to_owned(),
                summary: "Render focused metadata".to_owned(),
                author: "dev".to_owned(),
                timestamp: 1_709_999_000,
                unpushed: true,
                decorations: vec![crate::model::CommitDecoration {
                    kind: crate::model::CommitDecorationKind::LocalBranch,
                    label: "main".to_owned(),
                }],
            },
            selected: false,
            status: ReviewStatus::Unreviewed,
            is_uncommitted: false,
        };

        let metadata = super::focused_commit_metadata_summary(Some(&row), false);
        assert!(!metadata.contains("meta "));
        assert!(metadata.contains("abc1234"));
        assert!(metadata.contains("2024-03-09 15:43"));
        assert!(metadata.contains("dev"));
        assert!(metadata.contains("unpushed"));
        assert!(metadata.contains("refs main"));
        assert!(!metadata.contains("review"));
        assert!(!metadata.contains("id:"));
        assert!(!metadata.contains("author:"));
        assert!(!metadata.contains("state:"));
        assert!(metadata.find("2024-03-09 15:43") < metadata.find("dev"));
    }

    #[test]
    fn metadata_tokens_compress_multiple_refs() {
        let decorations = vec![
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::Head,
                label: "main*".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::LocalBranch,
                label: "main".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::LocalBranch,
                label: "release".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::LocalBranch,
                label: "hotfix".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::RemoteBranch,
                label: "origin/main".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::RemoteBranch,
                label: "origin/release".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::Tag,
                label: "v1.0.0".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::Tag,
                label: "v1.1.0".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::Tag,
                label: "v1.2.0".to_owned(),
            },
        ];

        let tokens = super::compact_ref_metadata_tokens(&decorations, false);
        assert!(tokens.contains(&"refs main*,release,+1".to_owned()));
        assert!(tokens.contains(&"origin/main,+1".to_owned()));
        assert!(tokens.contains(&"tag v1.0.0,v1.1.0,+1".to_owned()));
    }

    #[test]
    fn metadata_tokens_use_spaced_nerd_tag_prefix() {
        let decorations = vec![crate::model::CommitDecoration {
            kind: crate::model::CommitDecorationKind::Tag,
            label: "v1.2.3".to_owned(),
        }];

        let tokens = super::compact_ref_metadata_tokens(&decorations, true);
        assert_eq!(tokens, vec![" v1.2.3".to_owned()]);
    }

    #[test]
    fn commit_line_decorations_stay_concise_and_skip_tags() {
        let decorations = vec![
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::Head,
                label: "main*".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::LocalBranch,
                label: "main".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::LocalBranch,
                label: "release".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::RemoteBranch,
                label: "origin/main".to_owned(),
            },
            crate::model::CommitDecoration {
                kind: crate::model::CommitDecorationKind::Tag,
                label: "v1.0.0".to_owned(),
            },
        ];

        let label = super::compact_commit_line_decoration_label(&decorations, false);
        assert_eq!(label, "refs:main*,release,@origin/main");
        assert!(!label.contains("v1.0.0"));
    }
}
