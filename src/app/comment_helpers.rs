//! Comment modal presentation helpers and derived view-model state.
use crate::app::*;

pub(super) struct CommentModalView {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) line_ranges: Vec<(usize, usize)>,
    pub(super) view_start: usize,
    pub(super) text_offset: u16,
}

pub(super) fn comment_gutter_digits(total_lines: usize) -> usize {
    total_lines.max(1).to_string().len().max(2)
}

pub(super) fn comment_modal_lines(
    text: &str,
    cursor: usize,
    selection: Option<(usize, usize)>,
    viewport_rows: usize,
    theme: &UiTheme,
) -> CommentModalView {
    let ranges = comment_line_ranges(text);
    let clamped_cursor = clamp_char_boundary(text, cursor);
    let cursor_line_idx = text[..clamped_cursor]
        .chars()
        .filter(|ch| *ch == '\n')
        .count();
    let selected = normalize_selection_range(text, selection);
    let rows = viewport_rows.max(1);
    let max_start = ranges.len().saturating_sub(rows);
    let mut view_start = cursor_line_idx.saturating_sub(rows / 2);
    view_start = view_start.min(max_start);
    let view_end = (view_start + rows).min(ranges.len());
    let gutter_digits = comment_gutter_digits(ranges.len());
    let text_offset = (gutter_digits + 3) as u16;

    let mut lines = Vec::new();
    for (line_idx, (line_start, line_end)) in ranges
        .iter()
        .enumerate()
        .skip(view_start)
        .take(view_end.saturating_sub(view_start))
    {
        let gutter = Span::styled(
            format!("{:>width$} ", line_idx + 1, width = gutter_digits),
            Style::default().fg(theme.dimmed),
        );
        let mut spans = vec![
            gutter,
            Span::styled("│ ", Style::default().fg(theme.border)),
        ];

        let cursor_on_line = line_idx == cursor_line_idx;
        let mut idx = *line_start;
        while idx < *line_end {
            let next = next_char_boundary(text, idx);
            let fragment = &text[idx..next];
            let is_selected = selected.is_some_and(|(start, end)| idx >= start && idx < end);
            let is_cursor = cursor_on_line && idx == clamped_cursor;
            if is_cursor {
                spans.push(sanitized_span(
                    fragment,
                    Some(
                        Style::default()
                            .fg(theme.modal_cursor_fg)
                            .bg(theme.modal_cursor_bg),
                    ),
                ));
            } else if is_selected {
                spans.push(sanitized_span(
                    fragment,
                    Some(Style::default().bg(theme.visual_bg)),
                ));
            } else {
                spans.push(sanitized_span(fragment, None));
            }
            idx = next;
        }
        if cursor_on_line && clamped_cursor == *line_end {
            spans.push(Span::styled(
                " ",
                Style::default()
                    .fg(theme.modal_cursor_fg)
                    .bg(theme.modal_cursor_bg),
            ));
        }
        lines.push(Line::from(spans));
    }

    if lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            " ",
            Style::default()
                .fg(theme.modal_cursor_fg)
                .bg(theme.modal_cursor_bg),
        )]));
    }

    CommentModalView {
        lines,
        line_ranges: ranges,
        view_start,
        text_offset,
    }
}
