use chrono::Utc;
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState},
};

use super::super::{
    CommitRow, FocusPane, TreeRow, UiTheme, commit_selection_marker, display_width,
    format_relative_time, list_highlight_symbol, list_highlight_symbol_width, status_short_label,
    truncate, uncommitted_badge, unpushed_marker,
};
use super::style::{line_with_right, list_content_width, list_row_style, status_style};

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
    pub search_query: &'a str,
    pub file_list_state: &'a mut ListState,
}

/// Render payload for the commits pane.
pub(in crate::app) struct CommitPaneModel<'a> {
    pub commits: &'a [CommitRow],
    pub status_counts: (usize, usize, usize, usize),
    pub selected_total: usize,
    pub shown_commits: usize,
    pub total_commits: usize,
    pub status_filter: &'a str,
    pub search_query: &'a str,
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
            search_query,
            file_list_state,
        } = model;
        let search_badge = if search_query.trim().is_empty() {
            String::new()
        } else {
            format!("  /{}", search_query.trim())
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
                format!("{shown_files}/{changed_files} shown{search_badge}"),
                Style::default().fg(self.theme.muted),
            ),
        ]);
        let border_style = if self.focused == FocusPane::Files {
            Style::default().fg(self.theme.focus_border)
        } else {
            Style::default().fg(self.theme.border)
        };

        let highlight_symbol = list_highlight_symbol(self.nerd_fonts);
        let width = list_content_width(rect.width, list_highlight_symbol_width(self.nerd_fonts));
        let cursor_idx = file_list_state.selected();
        let presenter = ListLinePresenter::new(width, self.now_ts, self.theme, self.nerd_fonts);

        let items: Vec<ListItem<'static>> = file_rows
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let line = presenter.file_row_line(row);
                let is_cursor = cursor_idx == Some(idx);
                ListItem::new(line).style(list_row_style(
                    false,
                    is_cursor,
                    self.focused == FocusPane::Files,
                    None,
                    self.theme,
                ))
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
            search_query,
            commit_list_state,
        } = model;
        let (unreviewed, reviewed, issue_found, resolved) = status_counts;
        let search_badge = if search_query.trim().is_empty() {
            String::new()
        } else {
            format!(" /{}", search_query.trim())
        };
        let title = Line::from(vec![
            Span::styled(
                " 1 COMMITS ",
                Style::default()
                    .fg(self.theme.panel_title_fg)
                    .bg(self.theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!(
                    "sel:{selected_total}  U:{unreviewed} R:{reviewed} I:{issue_found} Z:{resolved}  {shown_commits}/{total_commits}  [{status_filter}]{search_badge}",
                ),
                Style::default().fg(self.theme.muted),
            ),
        ]);
        let border_style = if self.focused == FocusPane::Commits {
            Style::default().fg(self.theme.focus_border)
        } else {
            Style::default().fg(self.theme.border)
        };

        let highlight_symbol = list_highlight_symbol(self.nerd_fonts);
        let width = list_content_width(rect.width, list_highlight_symbol_width(self.nerd_fonts));
        let cursor_idx = commit_list_state.selected();
        let presenter = ListLinePresenter::new(width, self.now_ts, self.theme, self.nerd_fonts);
        let items: Vec<ListItem<'static>> = commits
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let line = presenter.commit_row_line(row);
                let is_cursor = cursor_idx == Some(idx);
                ListItem::new(line).style(list_row_style(
                    row.selected,
                    is_cursor,
                    self.focused == FocusPane::Commits,
                    Some(self.theme.cursor_bg),
                    self.theme,
                ))
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

/// Presenter for composing list pane rows with shared truncation and age columns.
pub(in crate::app) struct ListLinePresenter<'a> {
    width: usize,
    now_ts: i64,
    theme: &'a UiTheme,
    nerd_fonts: bool,
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
        }
    }

    pub(in crate::app) fn file_row_line(&self, row: &TreeRow) -> Line<'static> {
        if row.selectable {
            let right = row
                .modified_ts
                .map(|ts| format_relative_time(ts, self.now_ts))
                .unwrap_or_default();
            line_with_right(
                row.label.clone(),
                Style::default().fg(self.theme.text),
                right,
                Style::default().fg(self.theme.dimmed),
                self.width,
            )
        } else {
            Line::from(Span::styled(
                row.label.clone(),
                Style::default()
                    .fg(self.theme.dir)
                    .add_modifier(Modifier::BOLD),
            ))
        }
    }

    pub(in crate::app) fn commit_row_line(&self, row: &CommitRow) -> Line<'static> {
        if row.is_uncommitted {
            let marker = commit_selection_marker(row.selected, self.nerd_fonts);
            let left = format!("{marker} {} {}", row.info.short_id, row.info.summary);
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
        let left = format!("{} {} {}", marker, row.info.short_id, row.info.summary);
        let status_label = format!("[{}]", status_short_label(row.status));
        let unpushed = if row.info.unpushed {
            unpushed_marker(self.nerd_fonts)
        } else {
            ""
        };
        let right = format_relative_time(row.info.timestamp, self.now_ts);
        let reserved =
            1 + display_width(&status_label) + display_width(unpushed) + 1 + display_width(&right);
        let max_left = self.width.saturating_sub(reserved).max(1);
        let left_render = truncate(&left, max_left);
        let static_used = display_width(&left_render)
            + display_width(&status_label)
            + display_width(unpushed)
            + display_width(&right)
            + 1;
        let spaces = if static_used >= self.width {
            " ".to_owned()
        } else {
            " ".repeat(self.width - static_used)
        };

        Line::from(vec![
            Span::styled(left_render, Style::default().fg(self.theme.text)),
            Span::raw(" "),
            Span::styled(status_label, status_style(row.status, self.theme)),
            Span::styled(
                unpushed.to_owned(),
                Style::default().fg(self.theme.unpushed),
            ),
            Span::raw(spaces),
            Span::styled(right, Style::default().fg(self.theme.dimmed)),
        ])
    }
}
