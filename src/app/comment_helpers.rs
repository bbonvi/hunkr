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
    viewport_cols: usize,
    theme: &UiTheme,
) -> CommentModalView {
    let line_ranges = comment_line_ranges(text);
    let clamped_cursor = clamp_char_boundary(text, cursor);
    let selected = normalize_selection_range(text, selection);
    let rows = viewport_rows.max(1);
    let gutter_digits = comment_gutter_digits(line_ranges.len());
    let text_offset = (gutter_digits + 3) as u16;
    let payload_cols = viewport_cols.saturating_sub(text_offset as usize).max(1);

    let mut wrapped_rows = Vec::new();
    for (line_idx, (line_start, line_end)) in line_ranges.iter().copied().enumerate() {
        if line_start == line_end {
            wrapped_rows.push(WrappedCommentRow {
                line_idx,
                start: line_start,
                end: line_end,
                continuation: false,
                last_segment: true,
            });
            continue;
        }

        let mut seg_start = line_start;
        let mut continuation = false;
        loop {
            let seg_end = wrap_segment_end(text, seg_start, line_end, payload_cols);
            let last_segment = seg_end >= line_end;
            wrapped_rows.push(WrappedCommentRow {
                line_idx,
                start: seg_start,
                end: seg_end,
                continuation,
                last_segment,
            });
            if last_segment {
                break;
            }
            seg_start = seg_end;
            continuation = true;
        }
    }

    let cursor_row_idx = wrapped_rows
        .iter()
        .enumerate()
        .find_map(|(idx, row)| {
            let includes_cursor = (clamped_cursor >= row.start && clamped_cursor < row.end)
                || (row.start == row.end && clamped_cursor == row.start)
                || (row.last_segment && clamped_cursor == row.end);
            includes_cursor.then_some(idx)
        })
        .unwrap_or_default();

    let max_start = wrapped_rows.len().saturating_sub(rows);
    let mut view_start = cursor_row_idx.saturating_sub(rows / 2);
    view_start = view_start.min(max_start);
    let view_end = (view_start + rows).min(wrapped_rows.len());

    let mut lines = Vec::new();
    let mut visible_ranges = Vec::new();
    for (row_idx, row) in wrapped_rows
        .iter()
        .enumerate()
        .skip(view_start)
        .take(view_end.saturating_sub(view_start))
    {
        visible_ranges.push((row.start, row.end));
        let gutter = Span::styled(
            if row.continuation {
                format!("{:width$} ", "", width = gutter_digits)
            } else {
                format!("{:>width$} ", row.line_idx + 1, width = gutter_digits)
            },
            Style::default().fg(theme.dimmed),
        );
        let mut spans = vec![
            gutter,
            Span::styled("│ ", Style::default().fg(theme.border)),
        ];

        let cursor_on_line = row_idx == cursor_row_idx;
        let mut idx = row.start;
        while idx < row.end {
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
        if cursor_on_line && clamped_cursor == row.end {
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
        line_ranges: visible_ranges,
        view_start,
        text_offset,
    }
}

struct WrappedCommentRow {
    line_idx: usize,
    start: usize,
    end: usize,
    continuation: bool,
    last_segment: bool,
}

fn wrap_segment_end(text: &str, start: usize, line_end: usize, max_cols: usize) -> usize {
    let mut idx = start;
    let mut cols = 0usize;
    while idx < line_end && cols < max_cols {
        idx = next_char_boundary(text, idx);
        cols += 1;
    }
    if idx == start {
        next_char_boundary(text, start)
    } else {
        idx.min(line_end)
    }
}
