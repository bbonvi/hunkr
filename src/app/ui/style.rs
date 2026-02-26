use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::model::ReviewStatus;

use super::super::{UiTheme, blend_colors, display_width, truncate};

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

pub(in crate::app) fn list_row_style(
    selected: bool,
    cursor: bool,
    cursor_focused: bool,
    selected_bg: Option<Color>,
    theme: &UiTheme,
) -> Style {
    let selected_bg = selected_bg.unwrap_or(theme.cursor_bg);
    let cursor_bg = if cursor_focused {
        theme.visual_bg
    } else {
        theme.cursor_bg
    };

    if cursor {
        if selected {
            return Style::default()
                .bg(blend_colors(selected_bg, cursor_bg, 170))
                .add_modifier(Modifier::BOLD);
        }
        return Style::default().bg(cursor_bg).add_modifier(Modifier::BOLD);
    }
    if selected {
        return Style::default().bg(selected_bg);
    }
    Style::default()
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

pub(in crate::app) fn list_content_width(rect_width: u16, highlight_symbol_width: u16) -> usize {
    rect_width.saturating_sub(2 + highlight_symbol_width).max(1) as usize
}
