use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::model::ReviewStatus;

use super::super::{UiTheme, blend_colors, display_width, truncate};

/// Determines how cursor background interacts with an existing selection background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum CursorSelectionPolicy {
    /// Cursor background fully replaces selection background.
    CursorWins,
    /// Selection background is preserved when cursor is inside a visual selection.
    SelectionWins,
    /// Cursor background is blended over selection background.
    BlendCursorOverSelection { weight: u8 },
}

/// Resolves a row background from selection/cursor state using one shared policy.
pub(in crate::app) fn resolve_row_background(
    in_selection: bool,
    is_cursor: bool,
    selection_bg: Color,
    cursor_bg: Color,
    policy: CursorSelectionPolicy,
) -> Option<Color> {
    if !is_cursor {
        return in_selection.then_some(selection_bg);
    }
    Some(match (in_selection, policy) {
        (true, CursorSelectionPolicy::SelectionWins) => selection_bg,
        (true, CursorSelectionPolicy::BlendCursorOverSelection { weight }) => {
            blend_colors(selection_bg, cursor_bg, weight)
        }
        _ => cursor_bg,
    })
}

/// Applies shared row highlight semantics and optional cursor-row width padding.
pub(in crate::app) fn apply_row_highlight(
    line: &Line<'static>,
    line_width: u16,
    in_selection: bool,
    is_cursor: bool,
    selection_bg: Color,
    cursor_bg: Color,
    policy: CursorSelectionPolicy,
) -> Line<'static> {
    let row_bg = resolve_row_background(in_selection, is_cursor, selection_bg, cursor_bg, policy);
    let mut highlighted = match row_bg {
        Some(bg) => tint_line_background(line, bg, false),
        None => line.clone(),
    };
    if is_cursor {
        let pad_bg = row_bg.unwrap_or(cursor_bg);
        highlighted = pad_line_to_width(&highlighted, line_width, Style::default().bg(pad_bg));
    }
    highlighted
}

/// Applies a background tint to all spans in a rendered line.
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

pub(in crate::app) fn status_style(status: ReviewStatus, theme: &UiTheme) -> Style {
    match status {
        ReviewStatus::Unreviewed => Style::default()
            .fg(theme.unreviewed)
            .add_modifier(Modifier::BOLD),
        ReviewStatus::Reviewed => Style::default().fg(theme.reviewed),
        ReviewStatus::IssueFound => Style::default()
            .fg(theme.issue)
            .add_modifier(Modifier::BOLD),
        ReviewStatus::Resolved => Style::default().fg(theme.resolved),
    }
}

pub(in crate::app) fn line_with_right(
    left: String,
    left_style: Style,
    right: String,
    right_style: Style,
    width: usize,
) -> Line<'static> {
    if right.is_empty() {
        return Line::from(Span::styled(truncate(&left, width.max(1)), left_style));
    }
    let right_width = display_width(&right);
    if right_width + 1 >= width {
        return Line::from(Span::styled(truncate(&right, width.max(1)), right_style));
    }

    let max_left = width - right_width - 1;
    let left_render = truncate(&left, max_left.max(1));
    let left_width = display_width(&left_render);
    let spaces = if left_width + right_width + 1 >= width {
        " ".to_owned()
    } else {
        " ".repeat(width - left_width - right_width)
    };

    Line::from(vec![
        Span::styled(left_render, left_style),
        Span::raw(spaces),
        Span::styled(right, right_style),
    ])
}

/// Pads a rendered line to the viewport width so background highlights cover the full row.
pub(in crate::app) fn pad_line_to_width(
    line: &Line<'static>,
    width: u16,
    fallback_style: Style,
) -> Line<'static> {
    let target_width = width as usize;
    if target_width == 0 {
        return line.clone();
    }

    let rendered_width = line
        .spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum::<usize>();
    if rendered_width >= target_width {
        return line.clone();
    }

    let mut padded = line.clone();
    let mut padding_style = padded
        .spans
        .last()
        .map(|span| span.style)
        .unwrap_or(fallback_style);
    if padding_style.bg.is_none() && fallback_style.bg.is_some() {
        padding_style = padding_style.patch(fallback_style);
    }
    padded.spans.push(Span::styled(
        " ".repeat(target_width.saturating_sub(rendered_width)),
        padding_style,
    ));
    padded
}

pub(in crate::app) fn list_content_width(rect_width: u16, highlight_symbol_width: u16) -> usize {
    rect_width.saturating_sub(2 + highlight_symbol_width).max(1) as usize
}
