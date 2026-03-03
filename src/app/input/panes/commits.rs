use super::super::super::lifecycle_input::{clear_commit_selection, clear_commit_visual_anchor};
use super::super::super::*;

impl App {
    pub(in crate::app) fn handle_commits_key(&mut self, key: KeyEvent) {
        if let Some(target) = absolute_nav_target(key.code) {
            match target {
                AbsoluteNavTarget::Start => self.select_first_commit(),
                AbsoluteNavTarget::End => self.select_last_commit(),
            }
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('x') => {
                if clear_commit_selection(
                    &mut self.domain.commits,
                    &mut self.ui.commit_ui.visual_anchor,
                    &mut self.ui.commit_ui.selection_anchor,
                ) {
                    self.runtime.status = "Cleared commit selection".to_owned();
                    self.on_selection_changed();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_commit_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_commit_cursor(-1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_commits(0.5)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_commits(-0.5)
            }
            KeyCode::PageDown => self.page_commits(1.0),
            KeyCode::PageUp => self.page_commits(-1.0),
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.ui.preferences.input_mode = InputMode::ListSearch(FocusPane::Commits);
                self.ui.search.commit_cursor = self.ui.search.commit_query.len();
                self.runtime.status = format!("/{}", self.ui.search.commit_query);
            }
            KeyCode::Char('v') => {
                if clear_commit_visual_anchor(&mut self.ui.commit_ui.visual_anchor) {
                    self.runtime.status = "Commit visual range off".to_owned();
                } else {
                    self.ui.commit_ui.visual_anchor = self.selected_commit_full_index();
                    self.runtime.status = "Commit visual range on".to_owned();
                    self.apply_commit_visual_range();
                }
            }
            KeyCode::Char(' ') => {
                if let Some(cursor) = self.selected_commit_full_index() {
                    let anchor = range_anchor_for_space(
                        &self.domain.commits,
                        self.ui.commit_ui.selection_anchor,
                        cursor,
                    );
                    apply_range_selection(&mut self.domain.commits, anchor, cursor);
                    self.ui.commit_ui.selection_anchor = Some(cursor);
                }
                self.ui.commit_ui.visual_anchor = None;
                self.on_selection_changed();
            }
            KeyCode::Enter => {
                if clear_commit_visual_anchor(&mut self.ui.commit_ui.visual_anchor) {
                    self.runtime.status = "Commit visual range off".to_owned();
                } else if let Some(idx) = self.selected_commit_full_index() {
                    select_only_index(&mut self.domain.commits, idx);
                    self.ui.commit_ui.selection_anchor = Some(idx);
                    self.on_selection_changed();
                }
            }
            KeyCode::Char('e') if key.modifiers == KeyModifiers::NONE => {
                self.cycle_commit_status_filter();
            }
            KeyCode::Char('u') => self.set_contextual_commit_status(ReviewStatus::Unreviewed),
            KeyCode::Char('r') => self.set_contextual_commit_status(ReviewStatus::Reviewed),
            KeyCode::Char('i') => self.set_contextual_commit_status(ReviewStatus::IssueFound),
            _ => {}
        }
    }
}
