use super::super::super::*;

impl App {
    pub(in crate::app) fn handle_diff_key(&mut self, key: KeyEvent) {
        if let Some(op) = self.ui.diff_ui.pending_op {
            if key.modifiers == KeyModifiers::NONE {
                match (op, key.code) {
                    (DiffPendingOp::Z, KeyCode::Char('z')) => {
                        self.ui.diff_ui.pending_op = None;
                        self.align_diff_cursor_middle();
                        return;
                    }
                    (DiffPendingOp::Z, KeyCode::Char('t')) => {
                        self.ui.diff_ui.pending_op = None;
                        self.align_diff_cursor_top();
                        return;
                    }
                    (DiffPendingOp::Z, KeyCode::Char('b')) => {
                        self.ui.diff_ui.pending_op = None;
                        self.align_diff_cursor_bottom();
                        return;
                    }
                    _ => {}
                }
            }
            self.ui.diff_ui.pending_op = None;
        }

        if let Some(forward) = diff_search_repeat_direction(key) {
            self.repeat_diff_search(forward);
            return;
        }

        if let Some(target) = absolute_nav_target(key.code) {
            match target {
                AbsoluteNavTarget::Start => {
                    self.domain.diff_position.cursor = 0;
                    self.ensure_cursor_visible();
                }
                AbsoluteNavTarget::End => {
                    if !self.domain.rendered_diff.is_empty() {
                        self.domain.diff_position.cursor = self.domain.rendered_diff.len() - 1;
                        self.ensure_cursor_visible();
                    }
                }
            }
            return;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_diff_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_diff_cursor(-1),
            KeyCode::Char('h') if key.modifiers == KeyModifiers::NONE => {
                self.move_diff_block_cursor(-1)
            }
            KeyCode::Char('l') if key.modifiers == KeyModifiers::NONE => {
                self.move_diff_block_cursor(1)
            }
            KeyCode::Char('0') if key.modifiers == KeyModifiers::NONE => {
                self.set_diff_block_cursor_col(0)
            }
            KeyCode::Char('^') if plain_or_shift(key.modifiers) => {
                self.set_diff_block_cursor_to_line_first_non_whitespace()
            }
            KeyCode::Char('$') if plain_or_shift(key.modifiers) => {
                self.set_diff_block_cursor_to_line_end()
            }
            KeyCode::Char('w') if key.modifiers == KeyModifiers::NONE => {
                self.move_diff_block_cursor_next_word_start(false)
            }
            KeyCode::Char('W') if plain_or_shift(key.modifiers) => {
                self.move_diff_block_cursor_next_word_start(true)
            }
            KeyCode::Char('b') if key.modifiers == KeyModifiers::NONE => {
                self.move_diff_block_cursor_prev_word_start(false)
            }
            KeyCode::Char('B') if plain_or_shift(key.modifiers) => {
                self.move_diff_block_cursor_prev_word_start(true)
            }
            KeyCode::Char('e') if key.modifiers == KeyModifiers::NONE => {
                self.move_diff_block_cursor_next_word_end(false)
            }
            KeyCode::Char('E') if plain_or_shift(key.modifiers) => {
                self.move_diff_block_cursor_next_word_end(true)
            }
            KeyCode::Char('H') if plain_or_shift(key.modifiers) => {
                self.set_diff_block_cursor_to_line_first_non_whitespace()
            }
            KeyCode::Char('L') if plain_or_shift(key.modifiers) => {
                self.set_diff_block_cursor_to_line_end()
            }
            KeyCode::Esc => {
                let had_visual = self.ui.diff_ui.visual_selection.is_some();
                let had_search = self.clear_diff_search();
                if had_visual {
                    self.clear_diff_visual_selection();
                }
                if had_visual || had_search {
                    self.runtime.status = match (had_visual, had_search) {
                        (true, true) => "Diff visual range and search cleared".to_owned(),
                        (true, false) => "Diff visual range off".to_owned(),
                        (false, true) => "Diff search cleared".to_owned(),
                        (false, false) => unreachable!("guarded above"),
                    };
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_diff(-0.5)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_diff(0.5)
            }
            KeyCode::PageUp => self.page_diff(-1.0),
            KeyCode::PageDown => self.page_diff(1.0),
            KeyCode::Char('z') if key.modifiers == KeyModifiers::NONE => {
                self.ui.diff_ui.pending_op = Some(DiffPendingOp::Z);
            }
            KeyCode::Char('[') if key.modifiers == KeyModifiers::NONE => self.move_prev_hunk(),
            KeyCode::Char(']') if key.modifiers == KeyModifiers::NONE => self.move_next_hunk(),
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.ui.preferences.input_mode = InputMode::DiffSearch;
                self.ui.search.diff_buffer.clear();
                self.ui.search.diff_cursor = 0;
                self.ui.diff_ui.pending_op = None;
                self.runtime.status = "/".to_owned();
            }
            KeyCode::Char('*')
                if key.modifiers == KeyModifiers::SHIFT || key.modifiers == KeyModifiers::NONE =>
            {
                self.search_word_under_diff_cursor();
            }
            KeyCode::Char('#')
                if key.modifiers == KeyModifiers::SHIFT || key.modifiers == KeyModifiers::NONE =>
            {
                self.search_word_under_diff_cursor();
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                if self.domain.rendered_diff.is_empty() {
                    return;
                }
                if self.ui.diff_ui.visual_selection.is_some() {
                    self.clear_diff_visual_selection();
                    self.runtime.status = "Diff visual range off".to_owned();
                } else {
                    self.ui.diff_ui.mouse_anchor = None;
                    self.ui.diff_ui.visual_selection = Some(DiffVisualSelection {
                        anchor: self.domain.diff_position.cursor,
                        origin: DiffVisualOrigin::Keyboard,
                    });
                    self.runtime.status = "Diff visual range on".to_owned();
                }
            }
            KeyCode::Char('y') if key.modifiers == KeyModifiers::NONE => {
                self.copy_diff_visual_selection();
            }
            KeyCode::Enter if self.toggle_deleted_file_content_under_cursor() => {}
            KeyCode::Enter if self.ui.diff_ui.visual_selection.is_some() => {
                self.copy_diff_visual_selection();
            }
            _ => {}
        }
    }

    fn search_word_under_diff_cursor(&mut self) {
        let Some(line) = self
            .domain
            .rendered_diff
            .get(self.domain.diff_position.cursor)
        else {
            self.runtime.status = "No diff line under cursor".to_owned();
            return;
        };
        let line_text = diff_line_coord_text(line);
        let Some(word) = word_at_char_column(line_text, self.ui.diff_ui.block_cursor_col)
        else {
            self.runtime.status = "No searchable word under diff block cursor".to_owned();
            return;
        };
        self.ui.search.diff_query = Some(word.clone());
        self.runtime.status = format!("/{word}");
    }

    pub(in crate::app) fn clear_diff_search(&mut self) -> bool {
        self.ui.search.diff_buffer.clear();
        self.ui.search.diff_cursor = 0;
        self.ui.search.diff_query.take().is_some()
    }
}

pub(in crate::app) fn diff_search_repeat_direction(key: KeyEvent) -> Option<bool> {
    match key.code {
        KeyCode::Char('n') if key.modifiers == KeyModifiers::NONE => Some(true),
        KeyCode::Char('N') if key.modifiers == KeyModifiers::SHIFT => Some(false),
        KeyCode::Char('N') if key.modifiers == KeyModifiers::NONE => Some(false),
        KeyCode::Char('n') if key.modifiers == KeyModifiers::SHIFT => Some(false),
        _ => None,
    }
}

fn plain_or_shift(modifiers: KeyModifiers) -> bool {
    modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT
}
