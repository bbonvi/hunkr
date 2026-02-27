use std::collections::HashMap;

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use super::super::{
    CommentAnchor, CursorSelectionPolicy, DiffPosition, FocusPane, NerdFontTheme,
    RenderedDiffLine, UiTheme, apply_row_highlight, comment_anchor_matches,
    format_path_with_icon, is_commit_anchor, sanitized_span,
};

#[derive(Debug, Clone)]
pub(in crate::app) struct PendingDiffViewAnchor {
    pub cursor_line: DiffLineLocator,
    pub top_line: DiffLineLocator,
    pub cursor_to_top_offset: usize,
}

#[derive(Debug, Clone)]
pub(in crate::app) struct DiffLineLocator {
    anchor: Option<CommentAnchor>,
    raw_text: String,
    raw_text_occurrence: usize,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct DiffPaneTitle<'a> {
    pub selected_file: Option<&'a str>,
    pub selected_file_progress: Option<(usize, usize)>,
    pub nerd_fonts: bool,
    pub nerd_font_theme: &'a NerdFontTheme,
    pub selected_lines: usize,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct DiffPaneBody<'a> {
    pub rendered_diff: &'a [RenderedDiffLine],
    pub diff_position: DiffPosition,
    pub visual_range: Option<(usize, usize)>,
    pub sticky_banner_indexes: &'a [usize],
    pub empty_state_message: Option<&'a str>,
    pub line_overrides: &'a HashMap<usize, Line<'static>>,
}

/// Renders the diff pane so App can focus on orchestration/state transitions.
pub(in crate::app) struct DiffPaneRenderer<'a> {
    theme: &'a UiTheme,
    focused: FocusPane,
}

impl<'a> DiffPaneRenderer<'a> {
    pub(in crate::app) fn new(theme: &'a UiTheme, focused: FocusPane) -> Self {
        Self { theme, focused }
    }

    pub(in crate::app) fn render(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        title: DiffPaneTitle<'_>,
        body: DiffPaneBody<'_>,
    ) {
        let border_style = if self.focused == FocusPane::Diff {
            Style::default().fg(self.theme.focus_border)
        } else {
            Style::default().fg(self.theme.border)
        };

        let file_label = match (title.selected_file, title.selected_file_progress) {
            (Some(path), Some((index, total))) => {
                format!(
                    "{} ({index}/{total})",
                    format_path_with_icon(path, title.nerd_fonts, title.nerd_font_theme)
                )
            }
            (Some(path), None) => {
                format_path_with_icon(path, title.nerd_fonts, title.nerd_font_theme)
            }
            (None, _) => "(no file selected)".to_owned(),
        };
        let title = Line::from(vec![
            Span::styled(
                " 3 DIFF ",
                Style::default()
                    .fg(self.theme.panel_title_fg)
                    .bg(self.theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            sanitized_span(&file_label, Some(Style::default().fg(self.theme.text))),
            Span::raw(" "),
            Span::styled(
                format!("{} line(s) selected", title.selected_lines),
                Style::default().fg(self.theme.muted),
            ),
        ]);

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style);
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let max_sticky_rows = inner.height.saturating_sub(1) as usize;
        let sticky_rows = body.sticky_banner_indexes.len().min(max_sticky_rows);
        for (row, sticky_idx) in body
            .sticky_banner_indexes
            .iter()
            .take(sticky_rows)
            .enumerate()
        {
            let sticky_line = body
                .rendered_diff
                .get(*sticky_idx)
                .map(|line| {
                    display_line_with_selection(
                        line,
                        body.line_overrides.get(sticky_idx),
                        *sticky_idx,
                        inner.width,
                        body.visual_range,
                        body.diff_position.cursor,
                        self.focused == FocusPane::Diff,
                        self.theme,
                    )
                })
                .unwrap_or_else(|| Line::from(""));
            frame.render_widget(
                Paragraph::new(vec![sticky_line]),
                ratatui::layout::Rect {
                    x: inner.x,
                    y: inner.y + row as u16,
                    width: inner.width,
                    height: 1,
                },
            );
        }

        let body_height = inner.height.saturating_sub(sticky_rows as u16);
        if body_height > 0 {
            let mut body_lines = Vec::with_capacity(body_height as usize);
            for row in 0..body_height as usize {
                let line_idx = body.diff_position.scroll.saturating_add(row);
                let Some(line) = body.rendered_diff.get(line_idx) else {
                    break;
                };
                body_lines.push(display_line_with_selection(
                    line,
                    body.line_overrides.get(&line_idx),
                    line_idx,
                    inner.width,
                    body.visual_range,
                    body.diff_position.cursor,
                    self.focused == FocusPane::Diff,
                    self.theme,
                ));
            }

            if body_lines.is_empty() {
                let empty_state = body
                    .empty_state_message
                    .unwrap_or("No selected commits or no textual diff for this range");
                body_lines.push(Line::from(Span::styled(
                    empty_state,
                    Style::default().fg(self.theme.muted),
                )));
            }

            frame.render_widget(
                Paragraph::new(body_lines),
                ratatui::layout::Rect {
                    x: inner.x,
                    y: inner.y + sticky_rows as u16,
                    width: inner.width,
                    height: body_height,
                },
            );
        }

        self.render_diff_scrollbar(
            frame,
            rect,
            body.rendered_diff.len(),
            body.diff_position.scroll,
            sticky_rows,
        );
    }

    fn render_diff_scrollbar(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        rendered_len: usize,
        scroll: usize,
        sticky_rows: usize,
    ) {
        if rect.width < 3 || rect.height < 3 {
            return;
        }

        let inner_height = rect.height.saturating_sub(2) as usize;
        if inner_height == 0 {
            return;
        }
        let sticky_rows = sticky_rows.min(inner_height.saturating_sub(1));
        let viewport_height = inner_height.saturating_sub(sticky_rows);
        if viewport_height == 0 {
            return;
        }

        let total = rendered_len.max(1);
        let (thumb_start, thumb_len) = scrollbar_thumb(total, viewport_height, scroll);

        let x = rect.x.saturating_add(rect.width.saturating_sub(2));
        let y = rect.y.saturating_add(1 + sticky_rows as u16);
        let track_style = Style::default().fg(self.theme.dimmed);
        let thumb_style = Style::default()
            .fg(self.theme.muted)
            .add_modifier(Modifier::BOLD);

        let buffer = frame.buffer_mut();
        for row in 0..viewport_height {
            buffer.set_string(x, y + row as u16, "│", track_style);
        }
        for row in thumb_start..thumb_start.saturating_add(thumb_len) {
            if row < viewport_height {
                buffer.set_string(x, y + row as u16, "█", thumb_style);
            }
        }
    }
}

pub(in crate::app) fn is_hunk_header_line(line: &RenderedDiffLine) -> bool {
    line.raw_text.starts_with("@@ ")
        && line
            .anchor
            .as_ref()
            .is_some_and(|anchor| !is_commit_anchor(anchor))
}

pub(in crate::app) fn find_diff_match_from_cursor(
    lines: &[RenderedDiffLine],
    query: &str,
    forward: bool,
    cursor: usize,
) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    let query = query.to_ascii_lowercase();
    if query.is_empty() {
        return None;
    }

    let current = cursor.min(lines.len().saturating_sub(1));
    if forward {
        for (idx, line) in lines.iter().enumerate().skip(current.saturating_add(1)) {
            if line.raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
        for (idx, line) in lines.iter().enumerate().take(current + 1) {
            if line.raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
    } else {
        for (idx, line) in lines.iter().enumerate().take(current).rev() {
            if line.raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
        for (idx, line) in lines.iter().enumerate().skip(current).rev() {
            if line.raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
    }
    None
}

pub(in crate::app) fn capture_pending_diff_view_anchor(
    lines: &[RenderedDiffLine],
    diff_position: DiffPosition,
) -> Option<PendingDiffViewAnchor> {
    if lines.is_empty() {
        return None;
    }

    let cursor_idx = diff_position.cursor.min(lines.len() - 1);
    let top_idx = diff_position.scroll.min(lines.len() - 1);
    let cursor_line = diff_line_locator_for_index(lines, cursor_idx);
    let top_line = diff_line_locator_for_index(lines, top_idx);

    Some(PendingDiffViewAnchor {
        cursor_line,
        top_line,
        cursor_to_top_offset: cursor_idx.saturating_sub(top_idx),
    })
}

pub(in crate::app) fn find_index_for_line_locator(
    lines: &[RenderedDiffLine],
    locator: &DiffLineLocator,
) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }

    if let Some(expected_anchor) = &locator.anchor {
        let anchor_matches = lines
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| {
                line.anchor.as_ref().and_then(|actual| {
                    comment_anchor_matches(actual, expected_anchor).then_some(idx)
                })
            })
            .collect::<Vec<_>>();

        if anchor_matches.len() == 1 {
            return anchor_matches.first().copied();
        }
        if !anchor_matches.is_empty() {
            if let Some(idx) = find_index_for_raw_text_occurrence_in_candidates(
                lines,
                &anchor_matches,
                &locator.raw_text,
                locator.raw_text_occurrence,
            ) {
                return Some(idx);
            }
            return anchor_matches.last().copied();
        }
    }

    find_index_for_raw_text_occurrence(lines, &locator.raw_text, locator.raw_text_occurrence)
}

pub(in crate::app) fn scrollbar_thumb(
    total: usize,
    viewport: usize,
    scroll: usize,
) -> (usize, usize) {
    if viewport == 0 {
        return (0, 0);
    }
    if total <= viewport {
        return (0, viewport);
    }

    let max_scroll = total - viewport;
    let clamped_scroll = scroll.min(max_scroll);
    let thumb_len = ((viewport * viewport) / total).clamp(1, viewport);
    let track_len = viewport - thumb_len;
    let thumb_start = if max_scroll == 0 {
        0
    } else {
        (clamped_scroll * track_len) / max_scroll
    };
    (thumb_start, thumb_len)
}

fn diff_line_locator_for_index(lines: &[RenderedDiffLine], idx: usize) -> DiffLineLocator {
    let idx = idx.min(lines.len().saturating_sub(1));
    let line = &lines[idx];
    DiffLineLocator {
        anchor: line.anchor.clone(),
        raw_text: line.raw_text.clone(),
        raw_text_occurrence: raw_text_occurrence_at_index(lines, idx, &line.raw_text),
    }
}

fn raw_text_occurrence_at_index(lines: &[RenderedDiffLine], idx: usize, raw_text: &str) -> usize {
    lines[..=idx]
        .iter()
        .filter(|line| line.raw_text == raw_text)
        .count()
        .saturating_sub(1)
}

fn find_index_for_raw_text_occurrence(
    lines: &[RenderedDiffLine],
    raw_text: &str,
    occurrence: usize,
) -> Option<usize> {
    let mut seen = 0usize;
    let mut last_match = None;
    for (idx, line) in lines.iter().enumerate() {
        if line.raw_text == raw_text {
            if seen == occurrence {
                return Some(idx);
            }
            seen = seen.saturating_add(1);
            last_match = Some(idx);
        }
    }
    last_match
}

fn find_index_for_raw_text_occurrence_in_candidates(
    lines: &[RenderedDiffLine],
    candidate_indexes: &[usize],
    raw_text: &str,
    occurrence: usize,
) -> Option<usize> {
    let mut seen = 0usize;
    let mut last_match = None;
    for idx in candidate_indexes {
        let Some(line) = lines.get(*idx) else {
            continue;
        };
        if line.raw_text == raw_text {
            if seen == occurrence {
                return Some(*idx);
            }
            seen = seen.saturating_add(1);
            last_match = Some(*idx);
        }
    }
    last_match
}

fn display_line_with_selection(
    rendered: &RenderedDiffLine,
    override_line: Option<&Line<'static>>,
    idx: usize,
    line_width: u16,
    visual_range: Option<(usize, usize)>,
    cursor: usize,
    focused_diff: bool,
    theme: &UiTheme,
) -> Line<'static> {
    let line = override_line
        .cloned()
        .unwrap_or_else(|| rendered.line.clone());
    let in_visual = visual_range.is_some_and(|(start, end)| idx >= start && idx <= end);
    let is_cursor = idx == cursor && focused_diff;
    apply_row_highlight(
        &line,
        line_width,
        in_visual,
        is_cursor,
        theme.visual_bg,
        theme.cursor_bg,
        CursorSelectionPolicy::CursorWins,
    )
}
