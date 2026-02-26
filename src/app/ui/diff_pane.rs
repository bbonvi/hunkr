use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use super::super::{
    DiffPosition, FocusPane, RenderedDiffLine, UiTheme, blend_colors, is_commit_anchor,
};

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
        selected_file: Option<&str>,
        selected_lines: usize,
        rendered_diff: &[RenderedDiffLine],
        diff_position: DiffPosition,
        visual_range: Option<(usize, usize)>,
        sticky_commit_idx: Option<usize>,
    ) {
        let border_style = if self.focused == FocusPane::Diff {
            Style::default().fg(self.theme.focus_border)
        } else {
            Style::default().fg(self.theme.border)
        };

        let file_label = selected_file.unwrap_or("(no file selected)");
        let title = Line::from(vec![
            Span::styled(
                " 3 DIFF ",
                Style::default()
                    .fg(self.theme.panel_title_fg)
                    .bg(self.theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(file_label, Style::default().fg(self.theme.text)),
            Span::raw(" "),
            Span::styled(
                format!("{selected_lines} line(s) selected"),
                Style::default().fg(self.theme.muted),
            ),
        ]);

        let mut lines = Vec::with_capacity(rendered_diff.len());
        for (idx, rendered) in rendered_diff.iter().enumerate() {
            let mut line = rendered.line.clone();

            if let Some((start, end)) = visual_range
                && idx >= start
                && idx <= end
            {
                line = tint_line_background(&line, self.theme.visual_bg, false);
            }

            if idx == diff_position.cursor && self.focused == FocusPane::Diff {
                line = tint_line_background(&line, self.theme.cursor_bg, true);
            }
            lines.push(line);
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No selected commits or no textual diff for this file",
                Style::default().fg(self.theme.muted),
            )));
        }

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

        let sticky_rows = usize::from(sticky_commit_idx.is_some() && inner.height > 1);
        if sticky_rows == 1 {
            let sticky_idx = sticky_commit_idx.expect("sticky row requires banner index");
            let sticky_line = lines
                .get(sticky_idx)
                .cloned()
                .unwrap_or_else(|| Line::from(""));
            frame.render_widget(
                Paragraph::new(vec![sticky_line]),
                ratatui::layout::Rect {
                    x: inner.x,
                    y: inner.y,
                    width: inner.width,
                    height: 1,
                },
            );
        }

        let body_height = inner.height.saturating_sub(sticky_rows as u16);
        if body_height > 0 {
            frame.render_widget(
                Paragraph::new(lines).scroll((diff_position.scroll as u16, 0)),
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
            rendered_diff.len(),
            diff_position.scroll,
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

pub(in crate::app) fn tint_line_background(
    line: &Line<'static>,
    tint: Color,
    blend_existing: bool,
) -> Line<'static> {
    let mut patched = line.clone();
    for span in &mut patched.spans {
        let bg = if blend_existing {
            span.style
                .bg
                .map(|existing| blend_colors(existing, tint, 170))
                .unwrap_or(tint)
        } else {
            tint
        };
        span.style = span.style.patch(Style::default().bg(bg));
    }
    patched
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
        for idx in current.saturating_add(1)..lines.len() {
            if lines[idx].raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
        for idx in 0..=current {
            if lines[idx].raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
    } else {
        for idx in (0..current).rev() {
            if lines[idx].raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
        for idx in (current..lines.len()).rev() {
            if lines[idx].raw_text.to_ascii_lowercase().contains(&query) {
                return Some(idx);
            }
        }
    }
    None
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
