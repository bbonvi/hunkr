use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use unicode_width::UnicodeWidthChar;

use super::super::{
    DiffLineAnchor, DiffPosition, DiffVisibleRow, FocusPane, NerdFontTheme, RenderedDiffLine,
    UiTheme, blend_colors, diff_line_anchor_matches, diff_line_coord_text, format_path_with_icon,
    is_commit_line_anchor, is_diff_code_line, render_diff_line, sanitized_span,
};
use super::style::{CursorSelectionPolicy, apply_row_highlight, tint_line_background};

#[derive(Debug, Clone)]
pub(in crate::app) struct PendingDiffViewAnchor {
    pub cursor_line: DiffLineLocator,
    pub top_line: DiffLineLocator,
    pub cursor_to_top_offset: usize,
}

#[derive(Debug, Clone)]
pub(in crate::app) struct DiffLineLocator {
    anchor: Option<DiffLineAnchor>,
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
    pub visible_lines: &'a [Line<'static>],
    pub empty_state_message: Option<&'a str>,
    pub rendered_len: usize,
    pub scroll: usize,
    pub sticky_rows: usize,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct SelectionRenderContext<'a> {
    visual_range: Option<(usize, usize)>,
    cursor: usize,
    block_cursor_col: usize,
    search_query: Option<&'a str>,
    focused_diff: bool,
    theme: &'a UiTheme,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::app) struct DiffViewportBuildInput<'a> {
    pub rendered_diff: &'a [RenderedDiffLine],
    pub diff_position: DiffPosition,
    pub block_cursor_col: usize,
    pub search_query: Option<&'a str>,
    pub visual_range: Option<(usize, usize)>,
    pub sticky_banner_indexes: &'a [usize],
    pub viewport_rows: usize,
    pub inner_width: u16,
    pub focused_diff: bool,
}

#[derive(Debug, Clone)]
pub(in crate::app) struct DiffViewportRows {
    pub lines: Vec<Line<'static>>,
    pub visible_rows: Vec<DiffVisibleRow>,
    pub sticky_rows: usize,
}

/// Builds only the currently visible diff rows (including sticky banners) with
/// all cursor/search/visual styling applied. Callers can reuse `visible_rows`
/// for mouse hit-testing and render `lines` directly without a second pass.
pub(in crate::app) fn build_diff_viewport_rows<F>(
    input: DiffViewportBuildInput<'_>,
    theme: &UiTheme,
    mut line_override: F,
) -> DiffViewportRows
where
    F: FnMut(usize, &RenderedDiffLine) -> Option<Line<'static>>,
{
    let max_sticky_rows = input.viewport_rows.saturating_sub(1);
    let sticky_rows = input.sticky_banner_indexes.len().min(max_sticky_rows);
    let mut lines = Vec::with_capacity(input.viewport_rows);
    let mut visible_rows = Vec::with_capacity(input.viewport_rows);
    let selection = SelectionRenderContext {
        visual_range: input.visual_range,
        cursor: input.diff_position.cursor,
        block_cursor_col: input.block_cursor_col,
        search_query: input.search_query,
        focused_diff: input.focused_diff,
        theme,
    };

    for sticky_idx in input
        .sticky_banner_indexes
        .iter()
        .take(sticky_rows)
        .copied()
    {
        let sticky_line = input
            .rendered_diff
            .get(sticky_idx)
            .map(|line| {
                display_line_with_selection(
                    line,
                    line_override(sticky_idx, line),
                    sticky_idx,
                    input.inner_width,
                    selection,
                )
            })
            .unwrap_or_else(|| Line::from(""));
        lines.push(sticky_line);
        visible_rows.push(DiffVisibleRow {
            line_index: sticky_idx,
            wrapped_row_offset: 0,
        });
    }

    let target_rows = input.viewport_rows;
    let mut line_idx = input.diff_position.scroll;
    while lines.len() < target_rows {
        let Some(line) = input.rendered_diff.get(line_idx) else {
            break;
        };
        let display_line = display_line_with_selection(
            line,
            line_override(line_idx, line),
            line_idx,
            input.inner_width,
            selection,
        );
        for (wrapped_row_offset, wrapped) in
            hard_wrap_line(&display_line, input.inner_width as usize)
                .into_iter()
                .enumerate()
        {
            if lines.len() >= target_rows {
                break;
            }
            lines.push(wrapped);
            visible_rows.push(DiffVisibleRow {
                line_index: line_idx,
                wrapped_row_offset,
            });
        }
        line_idx += 1;
    }

    DiffViewportRows {
        lines,
        visible_rows,
        sticky_rows,
    }
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
        let content = if body.visible_lines.is_empty() {
            vec![Line::from(Span::styled(
                body.empty_state_message
                    .unwrap_or("No selected commits or no textual diff for this range"),
                Style::default().fg(self.theme.muted),
            ))]
        } else {
            body.visible_lines.to_vec()
        };
        frame.render_widget(Paragraph::new(content), inner);

        self.render_diff_scrollbar(
            frame,
            rect,
            body.rendered_len,
            body.scroll,
            body.sticky_rows,
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
            .is_some_and(|anchor| !is_commit_line_anchor(anchor))
}

pub(in crate::app) fn find_diff_match_from_cursor(
    lines: &[RenderedDiffLine],
    query: &str,
    forward: bool,
    cursor: usize,
    cursor_col: usize,
) -> Option<DiffSearchMatch> {
    if lines.is_empty() {
        return None;
    }
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let all_matches = collect_diff_search_matches(lines, query);
    if all_matches.is_empty() {
        return None;
    }

    let current_line = cursor.min(lines.len().saturating_sub(1));
    if forward {
        all_matches
            .iter()
            .copied()
            .find(|entry| {
                entry.line_index > current_line
                    || (entry.line_index == current_line && entry.char_col > cursor_col)
            })
            .or_else(|| all_matches.first().copied())
    } else {
        all_matches
            .iter()
            .rev()
            .copied()
            .find(|entry| {
                entry.line_index < current_line
                    || (entry.line_index == current_line && entry.char_col < cursor_col)
            })
            .or_else(|| all_matches.last().copied())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) struct DiffSearchMatch {
    /// Absolute rendered diff row index.
    pub line_index: usize,
    /// Character-column start of the matched occurrence in `raw_text`.
    pub char_col: usize,
}

fn collect_diff_search_matches(lines: &[RenderedDiffLine], query: &str) -> Vec<DiffSearchMatch> {
    let mut matches = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let coord_text = diff_line_coord_text(line);
        for (start, _) in find_case_insensitive_ranges(coord_text, query) {
            matches.push(DiffSearchMatch {
                line_index,
                char_col: coord_text[..start].chars().count(),
            });
        }
    }
    matches
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
                    diff_line_anchor_matches(actual, expected_anchor).then_some(idx)
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
        raw_text: line.raw_text.to_string(),
        raw_text_occurrence: raw_text_occurrence_at_index(lines, idx, &line.raw_text),
    }
}

fn raw_text_occurrence_at_index(lines: &[RenderedDiffLine], idx: usize, raw_text: &str) -> usize {
    lines[..=idx]
        .iter()
        .filter(|line| line.raw_text.as_ref() == raw_text)
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
        if line.raw_text.as_ref() == raw_text {
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
        if line.raw_text.as_ref() == raw_text {
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
    override_line: Option<Line<'static>>,
    idx: usize,
    line_width: u16,
    selection: SelectionRenderContext<'_>,
) -> Line<'static> {
    let line = override_line.unwrap_or_else(|| render_diff_line(rendered, selection.theme));
    let display_text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let layout = diff_payload_layout(rendered, &line);
    let coord_text = if layout.highlight_without_line_numbers {
        diff_line_coord_text(rendered)
    } else {
        display_text.as_str()
    };
    let in_visual = selection
        .visual_range
        .is_some_and(|(start, end)| idx >= start && idx <= end);
    let is_cursor = idx == selection.cursor && selection.focused_diff;
    let mut highlighted = if layout.highlight_without_line_numbers {
        apply_row_highlight_without_line_numbers(
            &line,
            line_width,
            in_visual,
            is_cursor,
            selection.theme,
        )
    } else {
        apply_row_highlight_with_visual_overlay(
            &line,
            line_width,
            in_visual,
            is_cursor,
            selection.theme,
        )
    };

    if let Some(query) = selection.search_query {
        highlighted = apply_search_highlights(
            &highlighted,
            coord_text,
            query,
            is_cursor,
            selection.block_cursor_col,
            selection.theme,
            layout,
        );
    }
    if is_cursor && layout.highlight_without_line_numbers {
        highlighted = apply_block_cursor_highlight(
            &highlighted,
            coord_text,
            selection.block_cursor_col,
            selection.theme,
            layout,
        );
    }

    highlighted
}

#[derive(Debug, Clone, Copy)]
struct DiffPayloadLayout {
    display_byte_offset: usize,
    highlight_without_line_numbers: bool,
}

fn diff_payload_layout(rendered: &RenderedDiffLine, line: &Line<'static>) -> DiffPayloadLayout {
    let looks_like_code_line = is_diff_code_line(Some(rendered)) && line.spans.len() >= 2;
    if !looks_like_code_line {
        return DiffPayloadLayout {
            display_byte_offset: 0,
            highlight_without_line_numbers: false,
        };
    }

    let prefix = line
        .spans
        .first()
        .map(|span| span.content.as_ref())
        .unwrap_or("");
    DiffPayloadLayout {
        display_byte_offset: prefix.len(),
        highlight_without_line_numbers: true,
    }
}

fn apply_row_highlight_without_line_numbers(
    line: &Line<'static>,
    line_width: u16,
    in_visual: bool,
    is_cursor: bool,
    theme: &UiTheme,
) -> Line<'static> {
    let cursor_line = apply_row_highlight(
        line,
        line_width,
        false,
        is_cursor,
        theme.visual_bg,
        theme.focused_cursor_bg,
        CursorSelectionPolicy::CursorWins,
    );
    if !in_visual {
        return cursor_line;
    }

    let Some(prefix_span) = line.spans.first() else {
        return cursor_line;
    };
    let payload_len = line
        .spans
        .iter()
        .skip(1)
        .map(|span| span.content.len())
        .sum::<usize>();
    if payload_len == 0 {
        return cursor_line;
    }

    let payload_start = prefix_span.content.len();
    let payload_end = payload_start.saturating_add(payload_len);
    let payload_bg = if is_cursor {
        cursor_visual_overlap_bg(theme)
    } else {
        theme.visual_bg
    };

    patch_line_byte_range(
        &cursor_line,
        payload_start,
        payload_end,
        Style::default().bg(payload_bg),
    )
}

fn apply_row_highlight_with_visual_overlay(
    line: &Line<'static>,
    line_width: u16,
    in_visual: bool,
    is_cursor: bool,
    theme: &UiTheme,
) -> Line<'static> {
    let cursor_line = apply_row_highlight(
        line,
        line_width,
        false,
        is_cursor,
        theme.visual_bg,
        theme.focused_cursor_bg,
        CursorSelectionPolicy::CursorWins,
    );
    if !in_visual {
        return cursor_line;
    }

    if is_cursor {
        return tint_line_background(&cursor_line, cursor_visual_overlap_bg(theme), false);
    }

    // Keep syntax/diff coloration while applying visual selection tint.
    tint_line_background(&cursor_line, theme.visual_bg, true)
}

fn cursor_visual_overlap_bg(theme: &UiTheme) -> ratatui::style::Color {
    blend_colors(
        theme.focused_cursor_bg,
        theme.visual_bg,
        theme.cursor_visual_overlap_weight,
    )
}

fn apply_search_highlights(
    line: &Line<'static>,
    coord_text: &str,
    query: &str,
    cursor_on_line: bool,
    block_cursor_col: usize,
    theme: &UiTheme,
    layout: DiffPayloadLayout,
) -> Line<'static> {
    let ranges = find_case_insensitive_ranges(coord_text, query);
    if ranges.is_empty() {
        return line.clone();
    }

    let cursor_range = cursor_on_line.then(|| {
        let cursor_byte = byte_index_for_char_column(coord_text, block_cursor_col)?;
        ranges
            .iter()
            .copied()
            .find(|(start, end)| cursor_byte >= *start && cursor_byte < *end)
    });

    let mut patched = line.clone();
    for (start, end) in ranges {
        let style = if cursor_range.flatten() == Some((start, end)) {
            Style::default()
                .fg(theme.search_current_fg)
                .bg(theme.search_current_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.search_match_fg)
                .bg(theme.search_match_bg)
        };
        let (display_start, display_end) = map_coord_range_to_display((start, end), layout);
        patched = patch_line_byte_range(&patched, display_start, display_end, style);
    }
    patched
}

fn apply_block_cursor_highlight(
    line: &Line<'static>,
    coord_text: &str,
    char_col: usize,
    theme: &UiTheme,
    layout: DiffPayloadLayout,
) -> Line<'static> {
    let Some((start, end)) = byte_range_for_char_column(coord_text, char_col) else {
        return line.clone();
    };
    let (display_start, display_end) = map_coord_range_to_display((start, end), layout);

    patch_line_byte_range(
        line,
        display_start,
        display_end,
        Style::default()
            .fg(theme.block_cursor_fg)
            .bg(theme.block_cursor_bg)
            .add_modifier(Modifier::BOLD),
    )
}

fn map_coord_range_to_display(range: (usize, usize), layout: DiffPayloadLayout) -> (usize, usize) {
    (
        layout.display_byte_offset + range.0,
        layout.display_byte_offset + range.1,
    )
}

fn find_case_insensitive_ranges(text: &str, query: &str) -> Vec<(usize, usize)> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    let lower_text = text.to_ascii_lowercase();
    let lower_query = query.to_ascii_lowercase();
    let mut ranges = Vec::new();
    let mut search_from = 0usize;
    while search_from < lower_text.len() {
        let Some(found) = lower_text[search_from..].find(&lower_query) else {
            break;
        };
        let start = search_from + found;
        let end = start + lower_query.len();
        if text.is_char_boundary(start) && text.is_char_boundary(end) {
            ranges.push((start, end));
            search_from = end.max(search_from.saturating_add(1));
        } else {
            search_from = start.saturating_add(1);
        }
    }
    ranges
}

fn byte_index_for_char_column(text: &str, char_col: usize) -> Option<usize> {
    byte_range_for_char_column(text, char_col).map(|(start, _)| start)
}

fn byte_range_for_char_column(text: &str, char_col: usize) -> Option<(usize, usize)> {
    let mut last = None;
    for (col, (idx, ch)) in text.char_indices().enumerate() {
        let end = idx + ch.len_utf8();
        if col == char_col {
            return Some((idx, end));
        }
        last = Some((idx, end));
    }
    last
}

fn patch_line_byte_range(
    line: &Line<'static>,
    start: usize,
    end: usize,
    style_patch: Style,
) -> Line<'static> {
    if start >= end {
        return line.clone();
    }

    let mut out = Vec::with_capacity(line.spans.len().saturating_add(2));
    let mut offset = 0usize;
    for span in &line.spans {
        let text = span.content.as_ref();
        let span_start = offset;
        let span_end = span_start + text.len();
        offset = span_end;

        if end <= span_start || start >= span_end {
            out.push(span.clone());
            continue;
        }

        let mut local_start = start.saturating_sub(span_start).min(text.len());
        let mut local_end = end.saturating_sub(span_start).min(text.len());
        local_start = floor_char_boundary(text, local_start);
        local_end = floor_char_boundary(text, local_end);
        if local_end <= local_start {
            out.push(span.clone());
            continue;
        }

        if local_start > 0 {
            out.push(Span::styled(text[..local_start].to_owned(), span.style));
        }
        if local_end > local_start {
            out.push(Span::styled(
                text[local_start..local_end].to_owned(),
                span.style.patch(style_patch),
            ));
        }
        if local_end < text.len() {
            out.push(Span::styled(text[local_end..].to_owned(), span.style));
        }
    }
    Line::from(out)
}

fn hard_wrap_line(line: &Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![line.clone()];
    }

    let mut rows = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0usize;

    for span in &line.spans {
        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if current_width > 0 && current_width + ch_width > max_width {
                rows.push(Line::from(std::mem::take(&mut current)));
                current_width = 0;
            }

            push_char_styled(&mut current, ch, span.style);
            current_width = current_width.saturating_add(ch_width);
        }
    }

    if !current.is_empty() {
        rows.push(Line::from(current));
    }
    if rows.is_empty() {
        rows.push(Line::from(""));
    }
    rows
}

fn push_char_styled(spans: &mut Vec<Span<'static>>, ch: char, style: Style) {
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content.to_mut().push(ch);
        return;
    }

    spans.push(Span::styled(ch.to_string(), style));
}

fn floor_char_boundary(text: &str, mut idx: usize) -> usize {
    idx = idx.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::{
        DiffViewportBuildInput, SelectionRenderContext, build_diff_viewport_rows,
        display_line_with_selection, patch_line_byte_range,
    };
    use crate::app::{
        DiffPosition, DiffVisibleRow, RenderedDiffLine, ThemeMode, UiTheme, blend_colors,
    };
    use crate::model::DiffLineAnchor;
    use ratatui::{
        style::{Color, Style},
        text::{Line, Span},
    };

    #[test]
    fn patch_line_byte_range_never_panics_on_non_boundary_offsets() {
        let line = Line::from(" src/app/ui/diff_pane.rs");
        let patched = patch_line_byte_range(&line, 1, 4, Style::default().bg(Color::Yellow));

        assert!(!patched.spans.is_empty());
    }

    #[test]
    fn cursor_block_is_not_rendered_on_file_header_rows() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let rendered = RenderedDiffLine {
            line: Line::from("==== file 1/12: src/app/ui/diff_pane.rs ===="),
            raw_text: "==== file 1/12: src/app/ui/diff_pane.rs ===="
                .to_owned()
                .into(),
            anchor: None,
        };
        let selection = SelectionRenderContext {
            visual_range: None,
            cursor: 0,
            block_cursor_col: 5,
            search_query: None,
            focused_diff: true,
            theme: &theme,
        };

        let highlighted = display_line_with_selection(&rendered, None, 0, 120, selection);
        assert!(
            highlighted
                .spans
                .iter()
                .all(|span| span.style.bg != Some(theme.block_cursor_bg)),
            "header rows should not render block cursor cell",
        );
    }

    #[test]
    fn visual_selection_overlays_cursor_line_highlight() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let rendered = RenderedDiffLine {
            line: Line::from(vec![
                Span::styled("  43   43 ", Style::default()),
                Span::styled("+", Style::default()),
                Span::raw(" "),
                Span::styled("pub block_cursor_col: usize,", Style::default()),
            ]),
            raw_text: "+ pub block_cursor_col: usize,".to_owned().into(),
            anchor: Some(DiffLineAnchor::new(
                "head",
                "summary",
                "src/app/ui/diff_pane.rs",
                "@@ -1,1 +1,1 @@",
                Some(43),
                Some(43),
            )),
        };
        let selection = SelectionRenderContext {
            visual_range: Some((0, 0)),
            cursor: 0,
            block_cursor_col: 200,
            search_query: None,
            focused_diff: true,
            theme: &theme,
        };

        let highlighted = display_line_with_selection(&rendered, None, 0, 120, selection);
        let layered_bg = blend_colors(
            theme.focused_cursor_bg,
            theme.visual_bg,
            theme.cursor_visual_overlap_weight,
        );
        assert!(
            highlighted
                .spans
                .iter()
                .skip(1)
                .any(|span| span.style.bg == Some(layered_bg)),
            "visual selection should be blended over cursor line tint",
        );
        assert!(
            highlighted
                .spans
                .iter()
                .any(|span| span.style.bg == Some(theme.focused_cursor_bg)),
            "cursor row should include focused cursor tint",
        );
    }

    #[test]
    fn cursor_line_uses_plain_cursor_tint_when_visual_mode_is_off() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let rendered = RenderedDiffLine {
            line: Line::from(vec![
                Span::styled("  43   43 ", Style::default()),
                Span::styled("+", Style::default()),
                Span::raw(" "),
                Span::styled("pub block_cursor_col: usize,", Style::default()),
            ]),
            raw_text: "+ pub block_cursor_col: usize,".to_owned().into(),
            anchor: Some(DiffLineAnchor::new(
                "head",
                "summary",
                "src/app/ui/diff_pane.rs",
                "@@ -1,1 +1,1 @@",
                Some(43),
                Some(43),
            )),
        };
        let selection = SelectionRenderContext {
            visual_range: None,
            cursor: 0,
            block_cursor_col: 200,
            search_query: None,
            focused_diff: true,
            theme: &theme,
        };

        let highlighted = display_line_with_selection(&rendered, None, 0, 120, selection);
        assert!(
            highlighted
                .spans
                .iter()
                .skip(1)
                .any(|span| span.style.bg == Some(theme.focused_cursor_bg)),
            "cursor row should keep cursor tint when visual mode is disabled",
        );
        assert!(
            highlighted
                .spans
                .iter()
                .skip(1)
                .all(|span| span.style.bg != Some(theme.visual_bg)),
            "visual tint should not appear when visual mode is disabled",
        );
    }

    #[test]
    fn block_cursor_on_context_prefix_space_is_single_cell() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let rendered = RenderedDiffLine {
            line: Line::from(vec![
                Span::styled("   9    9 ", Style::default()),
                Span::styled(" ", Style::default()),
                Span::raw(" "),
                Span::styled(
                    "  pub(in crate::app) struct DiffPaneBody<'a> {",
                    Style::default(),
                ),
            ]),
            raw_text: "   pub(in crate::app) struct DiffPaneBody<'a> {"
                .to_owned()
                .into(),
            anchor: Some(DiffLineAnchor::new(
                "head",
                "summary",
                "src/app/ui/diff_pane.rs",
                "@@ -1,1 +1,1 @@",
                Some(9),
                Some(9),
            )),
        };
        let selection = SelectionRenderContext {
            visual_range: None,
            cursor: 0,
            block_cursor_col: 0,
            search_query: None,
            focused_diff: true,
            theme: &theme,
        };

        let highlighted = display_line_with_selection(&rendered, None, 0, 120, selection);
        let cursor_cells = highlighted
            .spans
            .iter()
            .filter(|span| span.style.bg == Some(theme.block_cursor_bg))
            .map(|span| span.content.chars().count())
            .sum::<usize>();
        assert_eq!(
            cursor_cells, 1,
            "block cursor should render as one cell on context-line prefix",
        );
    }

    #[test]
    fn viewport_builder_materializes_only_rows_in_view() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let rendered = vec![
            RenderedDiffLine {
                line: Line::from("aaaaaaaaaa"),
                raw_text: "aaaaaaaaaa".to_owned().into(),
                anchor: None,
            },
            RenderedDiffLine {
                line: Line::from("bbbbbbbbbb"),
                raw_text: "bbbbbbbbbb".to_owned().into(),
                anchor: None,
            },
        ];
        let mut built_indexes = Vec::new();
        let viewport = build_diff_viewport_rows(
            DiffViewportBuildInput {
                rendered_diff: &rendered,
                diff_position: DiffPosition::default(),
                block_cursor_col: 0,
                search_query: None,
                visual_range: None,
                sticky_banner_indexes: &[],
                viewport_rows: 3,
                inner_width: 4,
                focused_diff: false,
            },
            &theme,
            |idx, _| {
                built_indexes.push(idx);
                None
            },
        );

        assert_eq!(built_indexes, vec![0]);
        assert_eq!(
            viewport.visible_rows,
            vec![
                DiffVisibleRow {
                    line_index: 0,
                    wrapped_row_offset: 0,
                },
                DiffVisibleRow {
                    line_index: 0,
                    wrapped_row_offset: 1,
                },
                DiffVisibleRow {
                    line_index: 0,
                    wrapped_row_offset: 2,
                },
            ],
        );
        assert_eq!(viewport.lines.len(), 3);
    }

    #[test]
    fn viewport_builder_keeps_sticky_rows_and_wrap_offsets() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let rendered = vec![
            RenderedDiffLine {
                line: Line::from("==== file 1/1: src/main.rs ===="),
                raw_text: "==== file 1/1: src/main.rs ====".to_owned().into(),
                anchor: None,
            },
            RenderedDiffLine {
                line: Line::from("123456789"),
                raw_text: "123456789".to_owned().into(),
                anchor: None,
            },
        ];

        let viewport = build_diff_viewport_rows(
            DiffViewportBuildInput {
                rendered_diff: &rendered,
                diff_position: DiffPosition {
                    scroll: 1,
                    cursor: 1,
                },
                block_cursor_col: 0,
                search_query: None,
                visual_range: None,
                sticky_banner_indexes: &[0],
                viewport_rows: 4,
                inner_width: 3,
                focused_diff: false,
            },
            &theme,
            |_, _| None,
        );

        assert_eq!(viewport.sticky_rows, 1);
        assert_eq!(
            viewport.visible_rows,
            vec![
                DiffVisibleRow {
                    line_index: 0,
                    wrapped_row_offset: 0,
                },
                DiffVisibleRow {
                    line_index: 1,
                    wrapped_row_offset: 0,
                },
                DiffVisibleRow {
                    line_index: 1,
                    wrapped_row_offset: 1,
                },
                DiffVisibleRow {
                    line_index: 1,
                    wrapped_row_offset: 2,
                },
            ],
        );
        assert_eq!(viewport.lines.len(), 4);
    }

    #[test]
    fn viewport_builder_composes_search_cursor_and_visual_styles() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        let rendered = vec![RenderedDiffLine {
            line: Line::from(vec![
                Span::styled("   9    9 ", Style::default()),
                Span::styled("+", Style::default()),
                Span::raw(" "),
                Span::styled("alpha target omega", Style::default()),
            ]),
            raw_text: "+ alpha target omega".to_owned().into(),
            anchor: Some(DiffLineAnchor::new(
                "head",
                "summary",
                "src/main.rs",
                "@@ -1,1 +1,1 @@",
                Some(9),
                Some(9),
            )),
        }];

        let viewport = build_diff_viewport_rows(
            DiffViewportBuildInput {
                rendered_diff: &rendered,
                diff_position: DiffPosition::default(),
                block_cursor_col: 8,
                search_query: Some("target"),
                visual_range: Some((0, 0)),
                sticky_banner_indexes: &[],
                viewport_rows: 1,
                inner_width: 80,
                focused_diff: true,
            },
            &theme,
            |_, _| None,
        );

        let line = &viewport.lines[0];
        let layered_bg = blend_colors(
            theme.focused_cursor_bg,
            theme.visual_bg,
            theme.cursor_visual_overlap_weight,
        );
        assert!(
            line.spans.iter().any(|span| {
                matches!(
                    span.style.bg,
                    Some(color) if color == theme.search_match_bg || color == theme.search_current_bg
                )
            }),
            "search highlight should be present in virtualized viewport rows",
        );
        assert!(
            line.spans
                .iter()
                .any(|span| span.style.bg == Some(layered_bg)),
            "cursor+visual blended tint should survive virtualization path",
        );
    }
}
