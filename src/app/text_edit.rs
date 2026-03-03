//! UTF-8-safe text editing primitives shared across inline text inputs.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

fn classify_vim_char(ch: char, big_word: bool) -> WordClass {
    if ch.is_whitespace() {
        WordClass::Whitespace
    } else if big_word {
        WordClass::Word
    } else {
        classify_char(ch)
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

/// Returns the last valid visual character column for the line.
pub(super) fn line_last_char_column(text: &str) -> Option<usize> {
    text.chars().count().checked_sub(1)
}

/// Returns the first non-whitespace visual character column.
///
/// For all-whitespace lines, returns column 0 to match Vim's `^` fallback.
pub(super) fn line_first_non_whitespace_column(text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    for (idx, ch) in text.chars().enumerate() {
        if !ch.is_whitespace() {
            return Some(idx);
        }
    }
    Some(0)
}

/// Vim-like `w`/`W`: move to start of next word/WORD.
pub(super) fn vim_next_word_start_column(
    text: &str,
    cursor_col: usize,
    big_word: bool,
) -> Option<usize> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return None;
    }
    let start = cursor_col.min(chars.len() - 1);
    let mut idx = start;
    let cls = classify_vim_char(chars[idx], big_word);
    if cls == WordClass::Whitespace {
        while idx < chars.len() && classify_vim_char(chars[idx], big_word) == WordClass::Whitespace
        {
            idx += 1;
        }
    } else {
        while idx < chars.len() && classify_vim_char(chars[idx], big_word) == cls {
            idx += 1;
        }
        while idx < chars.len() && classify_vim_char(chars[idx], big_word) == WordClass::Whitespace
        {
            idx += 1;
        }
    }
    Some(if idx < chars.len() { idx } else { start })
}

/// Vim-like `b`/`B`: move to start of previous word/WORD.
pub(super) fn vim_prev_word_start_column(
    text: &str,
    cursor_col: usize,
    big_word: bool,
) -> Option<usize> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return None;
    }
    let start = cursor_col.min(chars.len() - 1);
    if start == 0 {
        return Some(0);
    }

    let mut idx = start - 1;
    while idx > 0 && classify_vim_char(chars[idx], big_word) == WordClass::Whitespace {
        idx -= 1;
    }
    let cls = classify_vim_char(chars[idx], big_word);
    while idx > 0 && classify_vim_char(chars[idx - 1], big_word) == cls {
        idx -= 1;
    }
    Some(idx)
}

/// Vim-like `e`/`E`: move to end of current/next word/WORD.
pub(super) fn vim_next_word_end_column(
    text: &str,
    cursor_col: usize,
    big_word: bool,
) -> Option<usize> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return None;
    }
    let start = cursor_col.min(chars.len() - 1);
    let mut idx = start;
    let mut cls = classify_vim_char(chars[idx], big_word);

    if cls == WordClass::Whitespace {
        while idx < chars.len() && classify_vim_char(chars[idx], big_word) == WordClass::Whitespace
        {
            idx += 1;
        }
        if idx >= chars.len() {
            return Some(start);
        }
        cls = classify_vim_char(chars[idx], big_word);
        while idx + 1 < chars.len() && classify_vim_char(chars[idx + 1], big_word) == cls {
            idx += 1;
        }
        return Some(idx);
    }

    while idx + 1 < chars.len() && classify_vim_char(chars[idx + 1], big_word) == cls {
        idx += 1;
    }
    if idx > start {
        return Some(idx);
    }

    let mut seek = idx + 1;
    while seek < chars.len() && classify_vim_char(chars[seek], big_word) == WordClass::Whitespace {
        seek += 1;
    }
    if seek >= chars.len() {
        return Some(start);
    }

    cls = classify_vim_char(chars[seek], big_word);
    idx = seek;
    while idx + 1 < chars.len() && classify_vim_char(chars[idx + 1], big_word) == cls {
        idx += 1;
    }
    Some(idx)
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

/// Outcome when applying one keypress to a single-line editor buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SingleLineEditOutcome {
    NotHandled,
    CursorMoved,
    BufferChanged,
}

/// Applies one shell-like editing keypress to a single-line buffer.
pub(super) fn apply_single_line_edit_key(
    text: &mut String,
    cursor: &mut usize,
    key: KeyEvent,
) -> SingleLineEditOutcome {
    let start_len = text.len();
    let start_cursor = clamp_char_boundary(text, (*cursor).min(start_len));
    *cursor = start_cursor;

    let handled = match key.code {
        KeyCode::Home => {
            *cursor = 0;
            true
        }
        KeyCode::End => {
            *cursor = text.len();
            true
        }
        KeyCode::Left
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            *cursor = prev_word_boundary(text, *cursor);
            true
        }
        KeyCode::Left => {
            *cursor = prev_char_boundary(text, *cursor);
            true
        }
        KeyCode::Right
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            *cursor = next_word_boundary(text, *cursor);
            true
        }
        KeyCode::Right => {
            *cursor = next_char_boundary(text, *cursor);
            true
        }
        KeyCode::Backspace
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            delete_prev_word(text, cursor);
            true
        }
        KeyCode::Backspace => {
            delete_prev_char(text, cursor);
            true
        }
        KeyCode::Delete
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            delete_next_word(text, cursor);
            true
        }
        KeyCode::Delete => {
            delete_next_char(text, cursor);
            true
        }
        KeyCode::Char('a') | KeyCode::Char('A')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            *cursor = 0;
            true
        }
        KeyCode::Char('e') | KeyCode::Char('E')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            *cursor = text.len();
            true
        }
        KeyCode::Char('u') | KeyCode::Char('U')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            delete_to_line_start(text, cursor);
            true
        }
        KeyCode::Char('k') | KeyCode::Char('K')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            delete_to_line_end(text, cursor);
            true
        }
        KeyCode::Char('w') | KeyCode::Char('W')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            delete_prev_word(text, cursor);
            true
        }
        KeyCode::Char('b') | KeyCode::Char('B') if key.modifiers.contains(KeyModifiers::ALT) => {
            *cursor = prev_word_boundary(text, *cursor);
            true
        }
        KeyCode::Char('f') | KeyCode::Char('F') if key.modifiers.contains(KeyModifiers::ALT) => {
            *cursor = next_word_boundary(text, *cursor);
            true
        }
        KeyCode::Char('d') | KeyCode::Char('D') if key.modifiers.contains(KeyModifiers::ALT) => {
            delete_next_word(text, cursor);
            true
        }
        KeyCode::Char(c)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            insert_char_at_cursor(text, cursor, c);
            true
        }
        _ => false,
    };

    if !handled {
        return SingleLineEditOutcome::NotHandled;
    }

    if text.len() != start_len {
        SingleLineEditOutcome::BufferChanged
    } else if *cursor != start_cursor {
        SingleLineEditOutcome::CursorMoved
    } else {
        SingleLineEditOutcome::NotHandled
    }
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
