//! Keyboard/input-mode handlers for the app lifecycle.
use super::input::{modal_controller, panes};
use super::services::comment_workflow;
use super::*;

impl App {
    pub(super) fn handle_non_normal_input(&mut self, key: KeyEvent) {
        modal_controller::dispatch_modal_key(self, key);
    }

    pub(super) fn handle_comment_input(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
        {
            self.submit_comment_input();
            return;
        }

        match key.code {
            KeyCode::Esc => self.cancel_comment_input(),
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                );
                insert_char_at_cursor(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    '\n',
                );
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Enter => self.submit_comment_input(),
            KeyCode::Backspace
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                ) {
                    delete_prev_word(
                        &mut self.ui.comment_editor.buffer,
                        &mut self.ui.comment_editor.cursor,
                    );
                }
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Backspace => {
                if !delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                ) {
                    delete_prev_char(
                        &mut self.ui.comment_editor.buffer,
                        &mut self.ui.comment_editor.cursor,
                    );
                }
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Delete
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                ) {
                    delete_next_word(
                        &mut self.ui.comment_editor.buffer,
                        &mut self.ui.comment_editor.cursor,
                    );
                }
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Delete => {
                if !delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                ) {
                    delete_next_char(
                        &mut self.ui.comment_editor.buffer,
                        &mut self.ui.comment_editor.cursor,
                    );
                }
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Left
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.ui.comment_editor.cursor = prev_word_boundary(
                    &self.ui.comment_editor.buffer,
                    self.ui.comment_editor.cursor,
                );
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Left => {
                self.ui.comment_editor.cursor = prev_char_boundary(
                    &self.ui.comment_editor.buffer,
                    clamp_char_boundary(
                        &self.ui.comment_editor.buffer,
                        self.ui.comment_editor.cursor,
                    ),
                );
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Right
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.ui.comment_editor.cursor = next_word_boundary(
                    &self.ui.comment_editor.buffer,
                    self.ui.comment_editor.cursor,
                );
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Right => {
                self.ui.comment_editor.cursor = next_char_boundary(
                    &self.ui.comment_editor.buffer,
                    clamp_char_boundary(
                        &self.ui.comment_editor.buffer,
                        self.ui.comment_editor.cursor,
                    ),
                );
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Up => {
                self.ui.comment_editor.cursor = move_cursor_up(
                    &self.ui.comment_editor.buffer,
                    self.ui.comment_editor.cursor,
                );
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Down => {
                self.ui.comment_editor.cursor = move_cursor_down(
                    &self.ui.comment_editor.buffer,
                    self.ui.comment_editor.cursor,
                );
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Home => {
                self.ui.comment_editor.cursor = 0;
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::End => {
                self.ui.comment_editor.cursor = self.ui.comment_editor.buffer.len();
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('a') | KeyCode::Char('A')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.ui.comment_editor.cursor = 0;
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('e') | KeyCode::Char('E')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.ui.comment_editor.cursor = self.ui.comment_editor.buffer.len();
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('w') | KeyCode::Char('W')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                ) {
                    delete_prev_word(
                        &mut self.ui.comment_editor.buffer,
                        &mut self.ui.comment_editor.cursor,
                    );
                }
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('u') | KeyCode::Char('U')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                ) {
                    delete_to_line_start(
                        &mut self.ui.comment_editor.buffer,
                        &mut self.ui.comment_editor.cursor,
                    );
                }
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('k') | KeyCode::Char('K')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                ) {
                    delete_to_line_end(
                        &mut self.ui.comment_editor.buffer,
                        &mut self.ui.comment_editor.cursor,
                    );
                }
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('b') | KeyCode::Char('B')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.ui.comment_editor.cursor = prev_word_boundary(
                    &self.ui.comment_editor.buffer,
                    self.ui.comment_editor.cursor,
                );
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('f') | KeyCode::Char('F')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.ui.comment_editor.cursor = next_word_boundary(
                    &self.ui.comment_editor.buffer,
                    self.ui.comment_editor.cursor,
                );
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('d') | KeyCode::Char('D')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                ) {
                    delete_next_word(
                        &mut self.ui.comment_editor.buffer,
                        &mut self.ui.comment_editor.cursor,
                    );
                }
                self.ui.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                delete_selection_range(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    &mut self.ui.comment_editor.selection,
                );
                insert_char_at_cursor(
                    &mut self.ui.comment_editor.buffer,
                    &mut self.ui.comment_editor.cursor,
                    c,
                );
                self.ui.comment_editor.mouse_anchor = None;
            }
            _ => {}
        }
    }

    pub(super) fn refresh_comment_create_target_cache(&mut self) {
        self.ui.comment_editor.create_target_cache =
            Some(match self.comment_target_from_selection() {
                Ok(target) => CommentCreateTargetCache::Ready(Box::new(target)),
                Err(err) => CommentCreateTargetCache::Error(format!("{err:#}")),
            });
    }

    pub(in crate::app) fn resolve_comment_create_target(
        &mut self,
    ) -> Result<Option<CommentTarget>, String> {
        if self.ui.comment_editor.create_target_cache.is_none() {
            self.refresh_comment_create_target_cache();
        }
        match self.ui.comment_editor.create_target_cache.as_ref() {
            Some(CommentCreateTargetCache::Ready(target)) => Ok(target.as_ref().clone()),
            Some(CommentCreateTargetCache::Error(err)) => Err(err.clone()),
            None => Ok(None),
        }
    }

    pub(in crate::app) fn reset_comment_editor_state(&mut self) {
        self.ui.comment_editor.buffer.clear();
        self.ui.comment_editor.cursor = 0;
        self.ui.comment_editor.selection = None;
        self.ui.comment_editor.mouse_anchor = None;
        self.ui.comment_editor.rect = None;
        self.ui.comment_editor.line_ranges.clear();
        self.ui.comment_editor.view_start = 0;
        self.ui.comment_editor.text_offset = 0;
        self.ui.comment_editor.create_target_cache = None;
    }

    fn cancel_comment_input(&mut self) {
        self.ui.preferences.input_mode = InputMode::Normal;
        self.clear_diff_visual_selection();
        self.reset_comment_editor_state();
        self.runtime.status = "Comment canceled".to_owned();
    }

    fn submit_comment_input(&mut self) {
        if self.ui.comment_editor.buffer.trim().is_empty() {
            self.runtime.status = "Comment is empty".to_owned();
            return;
        }

        let close_editor = comment_workflow::submit_comment_from_editor(self);
        if close_editor {
            self.ui.preferences.input_mode = InputMode::Normal;
            self.clear_diff_visual_selection();
            self.reset_comment_editor_state();
        }
    }

    pub(super) fn handle_diff_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_diff_search_input(),
            KeyCode::Enter => {
                let query = self.ui.search.diff_buffer.trim().to_owned();
                self.ui.preferences.input_mode = InputMode::Normal;
                self.ui.search.diff_buffer.clear();
                self.ui.search.diff_cursor = 0;
                if query.is_empty() {
                    self.runtime.status = "Diff search canceled".to_owned();
                    return;
                }
                self.execute_diff_search(&query, true);
            }
            KeyCode::Backspace if self.ui.search.diff_buffer.is_empty() => {
                self.cancel_diff_search_input();
            }
            _ => {
                let edit = apply_single_line_edit_key(
                    &mut self.ui.search.diff_buffer,
                    &mut self.ui.search.diff_cursor,
                    key,
                );
                if !matches!(edit, SingleLineEditOutcome::NotHandled) {
                    self.runtime.status = format!("/{}", self.ui.search.diff_buffer);
                }
            }
        }
    }

    fn cancel_diff_search_input(&mut self) {
        self.ui.preferences.input_mode = InputMode::Normal;
        let cleared = self.clear_diff_search();
        self.runtime.status = if cleared {
            "Diff search cleared".to_owned()
        } else {
            "Diff search canceled".to_owned()
        };
    }

    fn cancel_list_search_input(
        &mut self,
        pane: FocusPane,
        preferred_commit_id: Option<&str>,
        fallback_visible_idx: Option<usize>,
    ) {
        self.ui.preferences.input_mode = InputMode::Normal;
        match pane {
            FocusPane::Commits => {
                self.ui.search.commit_query.clear();
                self.ui.search.commit_cursor = 0;
                self.sync_commit_cursor_for_filters(preferred_commit_id, fallback_visible_idx);
                self.runtime.status = "Commit search cleared".to_owned();
            }
            FocusPane::Files => {
                self.ui.search.file_query.clear();
                self.ui.search.file_cursor = 0;
                self.sync_file_cursor_for_filters();
                self.runtime.status = "File search cleared".to_owned();
            }
            FocusPane::Diff => {}
        }
    }

    pub(super) fn handle_list_search_input(&mut self, pane: FocusPane, key: KeyEvent) {
        let preferred_commit_id = (pane == FocusPane::Commits)
            .then(|| self.selected_commit_id())
            .flatten();
        let fallback_visible_idx = self.ui.commit_ui.list_state.selected();

        if matches!(key.code, KeyCode::Esc) {
            self.cancel_list_search_input(
                pane,
                preferred_commit_id.as_deref(),
                fallback_visible_idx,
            );
            return;
        }

        if matches!(key.code, KeyCode::Backspace) {
            let is_empty = match pane {
                FocusPane::Commits => self.ui.search.commit_query.is_empty(),
                FocusPane::Files => self.ui.search.file_query.is_empty(),
                FocusPane::Diff => false,
            };
            if is_empty {
                self.cancel_list_search_input(
                    pane,
                    preferred_commit_id.as_deref(),
                    fallback_visible_idx,
                );
                return;
            }
        }

        match key.code {
            KeyCode::Enter => {
                self.ui.preferences.input_mode = InputMode::Normal;
                let query = match pane {
                    FocusPane::Commits => self.ui.search.commit_query.trim(),
                    FocusPane::Files => self.ui.search.file_query.trim(),
                    FocusPane::Diff => "",
                };
                self.runtime.status = if query.is_empty() {
                    match pane {
                        FocusPane::Commits => {
                            format!(
                                "Commit search off ({})",
                                self.ui.commit_ui.status_filter.label()
                            )
                        }
                        FocusPane::Files => "File search off".to_owned(),
                        FocusPane::Diff => "Search off".to_owned(),
                    }
                } else {
                    match pane {
                        FocusPane::Commits => format!(
                            "Commit filter: /{} ({})",
                            query,
                            self.ui.commit_ui.status_filter.label()
                        ),
                        FocusPane::Files => format!("File filter: /{query}"),
                        FocusPane::Diff => format!("/{query}"),
                    }
                };
            }
            _ => {
                let edit = match pane {
                    FocusPane::Commits => apply_single_line_edit_key(
                        &mut self.ui.search.commit_query,
                        &mut self.ui.search.commit_cursor,
                        key,
                    ),
                    FocusPane::Files => apply_single_line_edit_key(
                        &mut self.ui.search.file_query,
                        &mut self.ui.search.file_cursor,
                        key,
                    ),
                    FocusPane::Diff => SingleLineEditOutcome::NotHandled,
                };
                match (pane, edit) {
                    (FocusPane::Commits, SingleLineEditOutcome::BufferChanged) => {
                        self.sync_commit_cursor_for_filters(
                            preferred_commit_id.as_deref(),
                            fallback_visible_idx,
                        );
                        self.runtime.status = format!("/{}", self.ui.search.commit_query);
                    }
                    (FocusPane::Files, SingleLineEditOutcome::BufferChanged) => {
                        self.sync_file_cursor_for_filters();
                        self.runtime.status = format!("/{}", self.ui.search.file_query);
                    }
                    (FocusPane::Commits, SingleLineEditOutcome::CursorMoved) => {
                        self.runtime.status = format!("/{}", self.ui.search.commit_query);
                    }
                    (FocusPane::Files, SingleLineEditOutcome::CursorMoved) => {
                        self.runtime.status = format!("/{}", self.ui.search.file_query);
                    }
                    _ => {}
                }
            }
        }
    }

    pub(super) fn execute_diff_search(&mut self, query: &str, forward: bool) {
        let normalized = query.trim();
        if normalized.is_empty() {
            self.runtime.status = "Empty diff search query".to_owned();
            return;
        }

        self.ui.search.diff_query = Some(normalized.to_owned());
        if let Some(found) = find_diff_match_from_cursor(
            &self.domain.rendered_diff,
            normalized,
            forward,
            self.domain.diff_position.cursor,
            self.ui.diff_ui.block_cursor_col,
        ) {
            let idx = found.line_index;
            let crossed_viewport_boundary = !self.diff_row_visible_in_viewport(idx);
            self.set_diff_cursor(idx);
            self.set_diff_block_cursor_col(found.char_col);
            if crossed_viewport_boundary {
                self.center_diff_cursor_in_viewport();
            }
            self.runtime.status = format!("/{normalized} -> line {}", idx.saturating_add(1));
        } else {
            self.runtime.status = format!("/{normalized} -> no match");
        }
    }

    pub(super) fn repeat_diff_search(&mut self, forward: bool) {
        let Some(query) = self.ui.search.diff_query.clone() else {
            self.runtime.status = "No previous diff search".to_owned();
            return;
        };
        self.execute_diff_search(&query, forward);
    }

    pub(super) fn start_comment_edit_mode(&mut self) {
        let Some(id) = self.current_comment_id() else {
            self.runtime.status = "No comment under cursor to edit".to_owned();
            return;
        };
        let Some(comment) = self.deps.comments.comment_by_id(id) else {
            self.runtime.status = format!("Comment #{} missing", id);
            return;
        };
        let comment_text = comment.text.clone();
        self.ui.preferences.input_mode = InputMode::CommentEdit(id);
        self.reset_comment_editor_state();
        self.ui.comment_editor.buffer = comment_text;
        self.ui.comment_editor.cursor = self.ui.comment_editor.buffer.len();
        self.runtime.status = format!(
            "Editing comment #{}: Enter save, Ctrl-s save, Esc cancel",
            id
        );
    }

    pub(super) fn delete_current_comment(&mut self) {
        let Some(id) = self.current_comment_id() else {
            self.runtime.status = "No comment under cursor to delete".to_owned();
            return;
        };
        match self.deps.comments.delete_comment(id) {
            Ok(true) => {
                self.capture_pending_diff_view_anchor();
                self.invalidate_diff_cache();
                if let Err(err) = self.sync_comment_report() {
                    self.runtime.status = format!(
                        "Comment #{} deleted, but review tasks sync failed: {err:#}",
                        id
                    );
                    return;
                }
                self.runtime.status = format!("Comment #{} deleted", id);
            }
            Ok(false) => {
                self.runtime.status = format!("Comment #{} not found", id);
            }
            Err(err) => {
                self.runtime.status = format!("Failed to delete comment #{}: {err:#}", id);
            }
        }
    }

    pub(super) fn dispatch_focus_key(&mut self, key: KeyEvent) {
        if self.ui.preferences.focused != FocusPane::Diff {
            self.ui.diff_ui.pending_op = None;
        }
        panes::dispatch_pane_key(self, key);
    }
}

pub(super) fn clear_commit_visual_anchor(visual_anchor: &mut Option<usize>) -> bool {
    visual_anchor.take().is_some()
}

pub(super) fn clear_commit_selection(
    rows: &mut [CommitRow],
    visual_anchor: &mut Option<usize>,
    selection_anchor: &mut Option<usize>,
) -> bool {
    let mut changed = false;
    for row in rows {
        changed |= row.selected;
        row.selected = false;
    }
    changed |= visual_anchor.take().is_some();
    changed |= selection_anchor.take().is_some();
    changed
}

#[cfg(test)]
pub(super) fn diff_search_repeat_direction(key: KeyEvent) -> Option<bool> {
    panes::diff::diff_search_repeat_direction(key)
}
