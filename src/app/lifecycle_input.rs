//! Keyboard/input-mode handlers for the app lifecycle.
use super::*;

impl App {
    pub(super) fn handle_non_normal_input(&mut self, key: KeyEvent) {
        match self.ui.preferences.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => self.handle_comment_input(key),
            InputMode::ShellCommand => self.handle_shell_command_input(key),
            InputMode::WorktreeSwitch => self.handle_worktree_switch_input(key),
            InputMode::DiffSearch => self.handle_diff_search_input(key),
            InputMode::ListSearch(pane) => self.handle_list_search_input(pane, key),
            InputMode::Normal => {}
        }
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

    fn resolve_comment_create_target(&mut self) -> Result<Option<CommentTarget>, String> {
        if self.ui.comment_editor.create_target_cache.is_none() {
            self.refresh_comment_create_target_cache();
        }
        match self.ui.comment_editor.create_target_cache.as_ref() {
            Some(CommentCreateTargetCache::Ready(target)) => Ok(target.as_ref().clone()),
            Some(CommentCreateTargetCache::Error(err)) => Err(err.clone()),
            None => Ok(None),
        }
    }

    fn reset_comment_editor_state(&mut self) {
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

        let mut close_editor = false;
        match self.ui.preferences.input_mode {
            InputMode::CommentCreate => match self.resolve_comment_create_target() {
                Ok(Some(target)) => {
                    let result = self
                        .deps
                        .comments
                        .add_comment(&target, &self.ui.comment_editor.buffer);
                    match result {
                        Ok(id) => {
                            self.capture_pending_diff_view_anchor();
                            self.set_status_for_ids(&target.commits, ReviewStatus::IssueFound);
                            self.invalidate_diff_cache();
                            if let Err(err) = self.sync_comment_report() {
                                self.runtime.status = format!(
                                    "Comment #{} added, but review tasks sync failed: {err:#}",
                                    id
                                );
                                close_editor = true;
                            } else {
                                self.runtime.status = format!(
                                    "Comment #{} added -> {} ({} commit(s) marked ISSUE_FOUND)",
                                    id,
                                    self.deps.comments.report_path().display(),
                                    target.commits.len()
                                );
                                close_editor = true;
                            }
                        }
                        Err(err) => {
                            self.runtime.status = format!("Failed to save comment: {err:#}");
                        }
                    }
                }
                Ok(None) => {
                    self.runtime.status = if self.diff_selection_spans_multiple_files() {
                        "Comment range must stay within a single file".to_owned()
                    } else {
                        "No hunk/line anchor at cursor or selected range".to_owned()
                    };
                    close_editor = true;
                }
                Err(err) => {
                    self.runtime.status =
                        format!("Failed to resolve affected commits for comment: {err}");
                    close_editor = true;
                }
            },
            InputMode::CommentEdit(id) => {
                match self
                    .deps
                    .comments
                    .update_comment(id, &self.ui.comment_editor.buffer)
                {
                    Ok(true) => {
                        self.capture_pending_diff_view_anchor();
                        self.invalidate_diff_cache();
                        if let Err(err) = self.sync_comment_report() {
                            self.runtime.status = format!(
                                "Comment #{} updated, but review tasks sync failed: {err:#}",
                                id
                            );
                        } else {
                            self.runtime.status = format!("Comment #{} updated", id);
                        }
                        close_editor = true;
                    }
                    Ok(false) => {
                        self.runtime.status = format!("Comment #{} not found", id);
                        close_editor = true;
                    }
                    Err(err) => {
                        self.runtime.status = format!("Failed to update comment #{}: {err:#}", id);
                    }
                }
            }
            InputMode::ShellCommand
            | InputMode::WorktreeSwitch
            | InputMode::DiffSearch
            | InputMode::ListSearch(_)
            | InputMode::Normal => {}
        }

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
        match self.ui.preferences.focused {
            FocusPane::Files => self.handle_files_key(key),
            FocusPane::Commits => self.handle_commits_key(key),
            FocusPane::Diff => self.handle_diff_key(key),
        }
    }

    pub(super) fn handle_files_key(&mut self, key: KeyEvent) {
        if let Some(target) = absolute_nav_target(key.code) {
            match target {
                AbsoluteNavTarget::Start => self.select_first_file(),
                AbsoluteNavTarget::End => self.select_last_file(),
            }
            return;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_file_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_file_cursor(-1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_files(0.5)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_files(-0.5)
            }
            KeyCode::PageDown => self.page_files(1.0),
            KeyCode::PageUp => self.page_files(-1.0),
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.ui.preferences.input_mode = InputMode::ListSearch(FocusPane::Files);
                self.ui.search.file_cursor = self.ui.search.file_query.len();
                self.runtime.status = format!("/{}", self.ui.search.file_query);
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.set_focus(FocusPane::Diff),
            _ => {}
        }
    }

    pub(super) fn handle_commits_key(&mut self, key: KeyEvent) {
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
                if let Some(idx) = self.selected_commit_full_index()
                    && let Some(row) = self.domain.commits.get_mut(idx)
                {
                    row.selected = !row.selected;
                    self.ui.commit_ui.selection_anchor = Some(idx);
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
            KeyCode::Char('s') => self.set_contextual_commit_status(ReviewStatus::Resolved),
            _ => {}
        }
    }

    pub(super) fn handle_diff_key(&mut self, key: KeyEvent) {
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

        if let Some(forward) = diff_comment_jump_direction(key) {
            if forward {
                self.move_next_comment();
            } else {
                self.move_prev_comment();
            }
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
            KeyCode::Char('m') => {
                if self.uncommitted_selected() {
                    self.runtime.status =
                        "Comments are disabled for uncommitted changes".to_owned();
                    return;
                }
                self.ui.preferences.input_mode = InputMode::CommentCreate;
                self.reset_comment_editor_state();
                self.refresh_comment_create_target_cache();
                self.ui.diff_ui.pending_op = None;
                self.runtime.status =
                    "Comment mode: Enter save, Alt+Enter newline, Esc cancel".to_owned();
            }
            KeyCode::Char('e') | KeyCode::Char('E')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if self.uncommitted_selected() {
                    self.runtime.status =
                        "Comments are disabled for uncommitted changes".to_owned();
                    return;
                }
                self.start_comment_edit_mode();
            }
            KeyCode::Char('y') if key.modifiers == KeyModifiers::NONE => {
                self.copy_diff_visual_selection();
            }
            KeyCode::Enter if self.toggle_deleted_file_content_under_cursor() => {}
            KeyCode::Enter if self.ui.diff_ui.visual_selection.is_some() => {
                self.copy_diff_visual_selection();
            }
            KeyCode::Char('Y')
                if key.modifiers == KeyModifiers::SHIFT || key.modifiers == KeyModifiers::NONE =>
            {
                self.copy_review_tasks_path();
            }
            KeyCode::Char('D') => {
                if self.uncommitted_selected() {
                    self.runtime.status =
                        "Comments are disabled for uncommitted changes".to_owned();
                    return;
                }
                self.delete_current_comment();
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
        let Some(word) = word_at_char_column(&line.raw_text, self.ui.diff_ui.block_cursor_col)
        else {
            self.runtime.status = "No searchable word under diff block cursor".to_owned();
            return;
        };
        self.ui.search.diff_query = Some(word.clone());
        self.runtime.status = format!("/{word}");
    }

    fn clear_diff_search(&mut self) -> bool {
        self.ui.search.diff_buffer.clear();
        self.ui.search.diff_cursor = 0;
        self.ui.search.diff_query.take().is_some()
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

pub(super) fn diff_search_repeat_direction(key: KeyEvent) -> Option<bool> {
    match key.code {
        KeyCode::Char('n') if key.modifiers == KeyModifiers::NONE => Some(true),
        KeyCode::Char('N') if key.modifiers == KeyModifiers::SHIFT => Some(false),
        KeyCode::Char('N') if key.modifiers == KeyModifiers::NONE => Some(false),
        KeyCode::Char('n') if key.modifiers == KeyModifiers::SHIFT => Some(false),
        _ => None,
    }
}

/// Resolves `p/P` comment-jump keys with tolerant Shift encoding handling.
pub(super) fn diff_comment_jump_direction(key: KeyEvent) -> Option<bool> {
    match key.code {
        KeyCode::Char('p') if key.modifiers == KeyModifiers::NONE => Some(true),
        KeyCode::Char('P') if key.modifiers == KeyModifiers::SHIFT => Some(false),
        KeyCode::Char('P') if key.modifiers == KeyModifiers::NONE => Some(false),
        KeyCode::Char('p') if key.modifiers == KeyModifiers::SHIFT => Some(false),
        _ => None,
    }
}

fn plain_or_shift(modifiers: KeyModifiers) -> bool {
    modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT
}
