//! UTF-8-safe text editing primitives for inline comment editing.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Whitespace,
    Word,
    Symbol,
}

fn classify_char(ch: char) -> WordClass {
    if ch.is_whitespace() {
        WordClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        WordClass::Word
    } else {
        WordClass::Symbol
    }
}

pub(super) fn clamp_char_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = cursor.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

pub(super) fn prev_char_boundary(text: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

pub(super) fn next_char_boundary(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    let Some(ch) = text[cursor..].chars().next() else {
        return text.len();
    };
    cursor + ch.len_utf8()
}

pub(super) fn prev_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_char_boundary(text, cursor);
    while idx > 0 {
        let prev = prev_char_boundary(text, idx);
        let ch = text[prev..idx].chars().next().expect("char at boundary");
        if classify_char(ch) == WordClass::Whitespace {
            idx = prev;
        } else {
            break;
        }
    }
    if idx == 0 {
        return 0;
    }
    let prev = prev_char_boundary(text, idx);
    let cls = classify_char(text[prev..idx].chars().next().expect("char at boundary"));
    while idx > 0 {
        let next = prev_char_boundary(text, idx);
        let ch_cls = classify_char(text[next..idx].chars().next().expect("char at boundary"));
        if ch_cls == cls {
            idx = next;
        } else {
            break;
        }
    }
    idx
}

pub(super) fn next_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_char_boundary(text, cursor);
    while idx < text.len() {
        let next = next_char_boundary(text, idx);
        let ch = text[idx..next].chars().next().expect("char at boundary");
        if classify_char(ch) == WordClass::Whitespace {
            idx = next;
        } else {
            break;
        }
    }
    if idx >= text.len() {
        return text.len();
    }
    let next = next_char_boundary(text, idx);
    let cls = classify_char(text[idx..next].chars().next().expect("char at boundary"));
    while idx < text.len() {
        let tail = next_char_boundary(text, idx);
        let ch_cls = classify_char(text[idx..tail].chars().next().expect("char at boundary"));
        if ch_cls == cls {
            idx = tail;
        } else {
            break;
        }
    }
    idx
}

/// Returns the word (`[[:alnum:]_]`) under the provided visual char column.
pub(super) fn word_at_char_column(text: &str, char_column: usize) -> Option<String> {
    let (start, end) = word_byte_range_at_char_column(text, char_column)?;
    Some(text[start..end].to_owned())
}

fn word_byte_range_at_char_column(text: &str, char_column: usize) -> Option<(usize, usize)> {
    if text.is_empty() {
        return None;
    }

    let (mut start, mut end, ch) = char_at_column_or_last(text, char_column)?;
    if classify_char(ch) != WordClass::Word {
        return None;
    }

    while start > 0 {
        let prev = prev_char_boundary(text, start);
        let prev_ch = text[prev..start].chars().next().expect("char at boundary");
        if classify_char(prev_ch) != WordClass::Word {
            break;
        }
        start = prev;
    }
    while end < text.len() {
        let next = next_char_boundary(text, end);
        let next_ch = text[end..next].chars().next().expect("char at boundary");
        if classify_char(next_ch) != WordClass::Word {
            break;
        }
        end = next;
    }

    Some((start, end))
}

fn char_at_column_or_last(text: &str, char_column: usize) -> Option<(usize, usize, char)> {
    let mut last = None;
    for (col, (start, ch)) in text.char_indices().enumerate() {
        let end = start + ch.len_utf8();
        let entry = (start, end, ch);
        if col == char_column {
            return Some(entry);
        }
        last = Some(entry);
    }
    last
}

pub(super) fn insert_char_at_cursor(text: &mut String, cursor: &mut usize, ch: char) {
    let idx = clamp_char_boundary(text, *cursor);
    text.insert(idx, ch);
    *cursor = idx + ch.len_utf8();
}

pub(super) fn delete_prev_char(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    if idx == 0 {
        *cursor = 0;
        return;
    }
    let start = prev_char_boundary(text, idx);
    text.replace_range(start..idx, "");
    *cursor = start;
}

pub(super) fn delete_next_char(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    if idx >= text.len() {
        *cursor = text.len();
        return;
    }
    let end = next_char_boundary(text, idx);
    text.replace_range(idx..end, "");
    *cursor = idx;
}

pub(super) fn delete_prev_word(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    let start = prev_word_boundary(text, idx);
    if start == idx {
        *cursor = idx;
        return;
    }
    text.replace_range(start..idx, "");
    *cursor = start;
}

pub(super) fn delete_next_word(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    let end = next_word_boundary(text, idx);
    if end == idx {
        *cursor = idx;
        return;
    }
    text.replace_range(idx..end, "");
    *cursor = idx;
}

pub(super) fn line_start_boundary(text: &str, cursor: usize) -> usize {
    let idx = clamp_char_boundary(text, cursor);
    text[..idx].rfind('\n').map(|pos| pos + 1).unwrap_or(0)
}

pub(super) fn line_end_boundary(text: &str, cursor: usize) -> usize {
    let idx = clamp_char_boundary(text, cursor);
    text[idx..]
        .find('\n')
        .map(|pos| idx + pos)
        .unwrap_or(text.len())
}

pub(super) fn line_char_count(text: &str, line_start: usize, line_end: usize) -> usize {
    text[line_start..line_end].chars().count()
}

pub(super) fn line_cursor_with_column(
    text: &str,
    line_start: usize,
    line_end: usize,
    column: usize,
) -> usize {
    let mut idx = line_start;
    let mut col = 0usize;
    while idx < line_end && col < column {
        let next = next_char_boundary(text, idx);
        if next <= idx || next > line_end {
            break;
        }
        idx = next;
        col += 1;
    }
    idx
}

pub(super) fn move_cursor_up(text: &str, cursor: usize) -> usize {
    let idx = clamp_char_boundary(text, cursor);
    let current_start = line_start_boundary(text, idx);
    if current_start == 0 {
        return idx;
    }
    let current_col = line_char_count(text, current_start, idx);
    let prev_end = current_start.saturating_sub(1);
    let prev_start = line_start_boundary(text, prev_end);
    let prev_len = line_char_count(text, prev_start, prev_end);
    line_cursor_with_column(text, prev_start, prev_end, current_col.min(prev_len))
}

pub(super) fn move_cursor_down(text: &str, cursor: usize) -> usize {
    let idx = clamp_char_boundary(text, cursor);
    let current_start = line_start_boundary(text, idx);
    let current_end = line_end_boundary(text, idx);
    if current_end >= text.len() {
        return idx;
    }
    let current_col = line_char_count(text, current_start, idx);
    let next_start = current_end + 1;
    let next_end = line_end_boundary(text, next_start);
    let next_len = line_char_count(text, next_start, next_end);
    line_cursor_with_column(text, next_start, next_end, current_col.min(next_len))
}

pub(super) fn delete_to_line_start(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    let start = line_start_boundary(text, idx);
    if start == idx {
        *cursor = idx;
        return;
    }
    text.replace_range(start..idx, "");
    *cursor = start;
}

pub(super) fn delete_to_line_end(text: &mut String, cursor: &mut usize) {
    let idx = clamp_char_boundary(text, *cursor);
    let end = line_end_boundary(text, idx);
    if idx >= end {
        *cursor = idx;
        return;
    }
    text.replace_range(idx..end, "");
    *cursor = idx;
}

pub(super) fn normalize_selection_range(
    text: &str,
    selection: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    let (raw_start, raw_end) = selection?;
    let start = clamp_char_boundary(text, raw_start);
    let end = clamp_char_boundary(text, raw_end);
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    (lo < hi).then_some((lo, hi))
}

pub(super) fn delete_selection_range(
    text: &mut String,
    cursor: &mut usize,
    selection: &mut Option<(usize, usize)>,
) -> bool {
    let Some((start, end)) = normalize_selection_range(text, *selection) else {
        *selection = None;
        return false;
    };
    text.replace_range(start..end, "");
    *cursor = start;
    *selection = None;
    true
}

pub(super) fn comment_line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::<(usize, usize)>::new();
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            ranges.push((start, idx));
            start = idx + ch.len_utf8();
        }
    }
    ranges.push((start, text.len()));
    ranges
}

pub(super) fn comment_cursor_line_col(text: &str, cursor: usize) -> (usize, usize) {
    let idx = clamp_char_boundary(text, cursor);
    let line = text[..idx].chars().filter(|ch| *ch == '\n').count() + 1;
    let line_start = text[..idx].rfind('\n').map(|pos| pos + 1).unwrap_or(0);
    let col = text[line_start..idx].chars().count() + 1;
    (line, col)
}
