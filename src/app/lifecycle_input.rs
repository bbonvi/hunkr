//! Keyboard/input-mode handlers for the app lifecycle.
use super::input::{modal_controller, panes};
use crate::app::*;

impl App {
    pub(super) fn handle_non_normal_input(&mut self, key: KeyEvent) {
        modal_controller::dispatch_modal_key(self, key);
    }

    pub(super) fn handle_diff_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_diff_search_input(),
            KeyCode::Enter => {
                let query = self.ui.search.diff_buffer.trim().to_owned();
                if query.is_empty() {
                    self.cancel_diff_search_input();
                    return;
                }
                self.ui.preferences.input_mode = InputMode::Normal;
                self.ui.search.diff_buffer.clear();
                self.ui.search.diff_cursor = 0;
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
        self.clear_diff_search();
        self.runtime.status = "Diff search cleared".to_owned();
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
                self.runtime.status = "Commit filter cleared".to_owned();
            }
            FocusPane::Files => {
                self.ui.search.file_query.clear();
                self.ui.search.file_cursor = 0;
                self.sync_file_cursor_for_filters();
                self.runtime.status = "File filter cleared".to_owned();
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
                    FocusPane::Commits => {
                        let trimmed = self.ui.search.commit_query.trim().to_owned();
                        self.ui.search.commit_query = trimmed.clone();
                        self.ui.search.commit_cursor = self.ui.search.commit_query.len();
                        self.sync_commit_cursor_for_filters(
                            preferred_commit_id.as_deref(),
                            fallback_visible_idx,
                        );
                        trimmed
                    }
                    FocusPane::Files => {
                        let trimmed = self.ui.search.file_query.trim().to_owned();
                        self.ui.search.file_query = trimmed.clone();
                        self.ui.search.file_cursor = self.ui.search.file_query.len();
                        self.sync_file_cursor_for_filters();
                        trimmed
                    }
                    FocusPane::Diff => String::new(),
                };
                self.runtime.status = if query.is_empty() {
                    match pane {
                        FocusPane::Commits => {
                            format!(
                                "Commit filter off ({})",
                                self.ui.commit_ui.status_filter.label()
                            )
                        }
                        FocusPane::Files => "File filter off".to_owned(),
                        FocusPane::Diff => "Search off".to_owned(),
                    }
                } else {
                    match pane {
                        FocusPane::Commits => format!(
                            "Commit filter: /{} ({})",
                            query.as_str(),
                            self.ui.commit_ui.status_filter.label()
                        ),
                        FocusPane::Files => format!("File filter: /{}", query.as_str()),
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
