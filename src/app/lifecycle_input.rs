//! Keyboard/input-mode handlers for the app lifecycle.
use super::*;

impl App {
    pub(super) fn handle_non_normal_input(&mut self, key: KeyEvent) {
        match self.preferences.input_mode {
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
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                );
                insert_char_at_cursor(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    '\n',
                );
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Enter => self.submit_comment_input(),
            KeyCode::Backspace
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                ) {
                    delete_prev_word(
                        &mut self.comment_editor.buffer,
                        &mut self.comment_editor.cursor,
                    );
                }
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Backspace => {
                if !delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                ) {
                    delete_prev_char(
                        &mut self.comment_editor.buffer,
                        &mut self.comment_editor.cursor,
                    );
                }
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Delete
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                ) {
                    delete_next_word(
                        &mut self.comment_editor.buffer,
                        &mut self.comment_editor.cursor,
                    );
                }
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Delete => {
                if !delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                ) {
                    delete_next_char(
                        &mut self.comment_editor.buffer,
                        &mut self.comment_editor.cursor,
                    );
                }
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Left
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.comment_editor.cursor =
                    prev_word_boundary(&self.comment_editor.buffer, self.comment_editor.cursor);
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Left => {
                self.comment_editor.cursor = prev_char_boundary(
                    &self.comment_editor.buffer,
                    clamp_char_boundary(&self.comment_editor.buffer, self.comment_editor.cursor),
                );
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Right
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.comment_editor.cursor =
                    next_word_boundary(&self.comment_editor.buffer, self.comment_editor.cursor);
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Right => {
                self.comment_editor.cursor = next_char_boundary(
                    &self.comment_editor.buffer,
                    clamp_char_boundary(&self.comment_editor.buffer, self.comment_editor.cursor),
                );
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Up => {
                self.comment_editor.cursor =
                    move_cursor_up(&self.comment_editor.buffer, self.comment_editor.cursor);
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Down => {
                self.comment_editor.cursor =
                    move_cursor_down(&self.comment_editor.buffer, self.comment_editor.cursor);
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Home => {
                self.comment_editor.cursor = 0;
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::End => {
                self.comment_editor.cursor = self.comment_editor.buffer.len();
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('a') | KeyCode::Char('A')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.comment_editor.cursor = 0;
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('e') | KeyCode::Char('E')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.comment_editor.cursor = self.comment_editor.buffer.len();
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('w') | KeyCode::Char('W')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                ) {
                    delete_prev_word(
                        &mut self.comment_editor.buffer,
                        &mut self.comment_editor.cursor,
                    );
                }
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('u') | KeyCode::Char('U')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                ) {
                    delete_to_line_start(
                        &mut self.comment_editor.buffer,
                        &mut self.comment_editor.cursor,
                    );
                }
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('k') | KeyCode::Char('K')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if !delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                ) {
                    delete_to_line_end(
                        &mut self.comment_editor.buffer,
                        &mut self.comment_editor.cursor,
                    );
                }
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('b') | KeyCode::Char('B')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.comment_editor.cursor =
                    prev_word_boundary(&self.comment_editor.buffer, self.comment_editor.cursor);
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('f') | KeyCode::Char('F')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.comment_editor.cursor =
                    next_word_boundary(&self.comment_editor.buffer, self.comment_editor.cursor);
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char('d') | KeyCode::Char('D')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if !delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                ) {
                    delete_next_word(
                        &mut self.comment_editor.buffer,
                        &mut self.comment_editor.cursor,
                    );
                }
                self.comment_editor.mouse_anchor = None;
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                delete_selection_range(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    &mut self.comment_editor.selection,
                );
                insert_char_at_cursor(
                    &mut self.comment_editor.buffer,
                    &mut self.comment_editor.cursor,
                    c,
                );
                self.comment_editor.mouse_anchor = None;
            }
            _ => {}
        }
    }

    fn cancel_comment_input(&mut self) {
        self.preferences.input_mode = InputMode::Normal;
        self.clear_diff_visual_selection();
        self.comment_editor.buffer.clear();
        self.comment_editor.cursor = 0;
        self.comment_editor.selection = None;
        self.comment_editor.mouse_anchor = None;
        self.comment_editor.rect = None;
        self.comment_editor.line_ranges.clear();
        self.comment_editor.view_start = 0;
        self.comment_editor.text_offset = 0;
        self.runtime.status = "Comment canceled".to_owned();
    }

    fn submit_comment_input(&mut self) {
        if self.comment_editor.buffer.trim().is_empty() {
            self.runtime.status = "Comment is empty".to_owned();
            return;
        }

        let mut close_editor = false;
        match self.preferences.input_mode {
            InputMode::CommentCreate => match self.comment_target_from_selection() {
                Ok(Some(target)) => {
                    let result = self
                        .comments
                        .add_comment(&target, &self.comment_editor.buffer);
                    match result {
                        Ok(id) => {
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
                                    self.comments.report_path().display(),
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
                        format!("Failed to resolve affected commits for comment: {err:#}");
                    close_editor = true;
                }
            },
            InputMode::CommentEdit(id) => {
                match self
                    .comments
                    .update_comment(id, &self.comment_editor.buffer)
                {
                    Ok(true) => {
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
            self.preferences.input_mode = InputMode::Normal;
            self.clear_diff_visual_selection();
            self.comment_editor.buffer.clear();
            self.comment_editor.cursor = 0;
            self.comment_editor.selection = None;
            self.comment_editor.mouse_anchor = None;
            self.comment_editor.rect = None;
            self.comment_editor.line_ranges.clear();
            self.comment_editor.view_start = 0;
            self.comment_editor.text_offset = 0;
        }
    }

    pub(super) fn handle_diff_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.preferences.input_mode = InputMode::Normal;
                self.search.diff_buffer.clear();
                let cleared = self.clear_diff_search();
                self.runtime.status = if cleared {
                    "Diff search cleared".to_owned()
                } else {
                    "Diff search canceled".to_owned()
                };
            }
            KeyCode::Enter => {
                let query = self.search.diff_buffer.trim().to_owned();
                self.preferences.input_mode = InputMode::Normal;
                self.search.diff_buffer.clear();
                if query.is_empty() {
                    self.runtime.status = "Diff search canceled".to_owned();
                    return;
                }
                self.execute_diff_search(&query, true);
            }
            KeyCode::Backspace => {
                self.search.diff_buffer.pop();
                self.runtime.status = format!("/{}", self.search.diff_buffer);
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                self.search.diff_buffer.push(c);
                self.runtime.status = format!("/{}", self.search.diff_buffer);
            }
            _ => {}
        }
    }

    pub(super) fn handle_list_search_input(&mut self, pane: FocusPane, key: KeyEvent) {
        let preferred_commit_id = (pane == FocusPane::Commits)
            .then(|| self.selected_commit_id())
            .flatten();
        let fallback_visible_idx = self.commit_ui.list_state.selected();

        match key.code {
            KeyCode::Esc => {
                self.preferences.input_mode = InputMode::Normal;
                match pane {
                    FocusPane::Commits => {
                        self.search.commit_query.clear();
                        self.sync_commit_cursor_for_filters(
                            preferred_commit_id.as_deref(),
                            fallback_visible_idx,
                        );
                        self.runtime.status = "Commit search cleared".to_owned();
                    }
                    FocusPane::Files => {
                        self.search.file_query.clear();
                        self.sync_file_cursor_for_filters();
                        self.runtime.status = "File search cleared".to_owned();
                    }
                    FocusPane::Diff => {}
                }
            }
            KeyCode::Enter => {
                self.preferences.input_mode = InputMode::Normal;
                let query = match pane {
                    FocusPane::Commits => self.search.commit_query.trim(),
                    FocusPane::Files => self.search.file_query.trim(),
                    FocusPane::Diff => "",
                };
                self.runtime.status = if query.is_empty() {
                    match pane {
                        FocusPane::Commits => {
                            format!(
                                "Commit search off ({})",
                                self.commit_ui.status_filter.label()
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
                            self.commit_ui.status_filter.label()
                        ),
                        FocusPane::Files => format!("File filter: /{query}"),
                        FocusPane::Diff => format!("/{query}"),
                    }
                };
            }
            KeyCode::Backspace => match pane {
                FocusPane::Commits => {
                    self.search.commit_query.pop();
                    self.sync_commit_cursor_for_filters(
                        preferred_commit_id.as_deref(),
                        fallback_visible_idx,
                    );
                    self.runtime.status = format!("/{}", self.search.commit_query);
                }
                FocusPane::Files => {
                    self.search.file_query.pop();
                    self.sync_file_cursor_for_filters();
                    self.runtime.status = format!("/{}", self.search.file_query);
                }
                FocusPane::Diff => {}
            },
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                match pane {
                    FocusPane::Commits => {
                        self.search.commit_query.push(c);
                        self.sync_commit_cursor_for_filters(
                            preferred_commit_id.as_deref(),
                            fallback_visible_idx,
                        );
                        self.runtime.status = format!("/{}", self.search.commit_query);
                    }
                    FocusPane::Files => {
                        self.search.file_query.push(c);
                        self.sync_file_cursor_for_filters();
                        self.runtime.status = format!("/{}", self.search.file_query);
                    }
                    FocusPane::Diff => {}
                }
            }
            _ => {}
        }
    }

    pub(super) fn execute_diff_search(&mut self, query: &str, forward: bool) {
        let normalized = query.trim();
        if normalized.is_empty() {
            self.runtime.status = "Empty diff search query".to_owned();
            return;
        }

        self.search.diff_query = Some(normalized.to_owned());
        if let Some(idx) = find_diff_match_from_cursor(
            &self.rendered_diff,
            normalized,
            forward,
            self.diff_position.cursor,
        ) {
            self.set_diff_cursor(idx);
            if let Some(line) = self.rendered_diff.get(idx)
                && let Some(col) =
                    first_diff_match_char_column(&line_plain_text(&line.line), normalized)
            {
                self.set_diff_block_cursor_col(col);
            }
            self.runtime.status = format!("/{normalized} -> line {}", idx.saturating_add(1));
        } else {
            self.runtime.status = format!("/{normalized} -> no match");
        }
    }

    pub(super) fn repeat_diff_search(&mut self, forward: bool) {
        let Some(query) = self.search.diff_query.clone() else {
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
        let Some(comment) = self.comments.comment_by_id(id) else {
            self.runtime.status = format!("Comment #{} missing", id);
            return;
        };
        self.preferences.input_mode = InputMode::CommentEdit(id);
        self.comment_editor.buffer = comment.text.clone();
        self.comment_editor.cursor = self.comment_editor.buffer.len();
        self.comment_editor.selection = None;
        self.comment_editor.mouse_anchor = None;
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
        match self.comments.delete_comment(id) {
            Ok(true) => {
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
        if self.preferences.focused != FocusPane::Diff {
            self.diff_ui.pending_op = None;
        }
        match self.preferences.focused {
            FocusPane::Files => self.handle_files_key(key),
            FocusPane::Commits => self.handle_commits_key(key),
            FocusPane::Diff => self.handle_diff_key(key),
        }
    }

    pub(super) fn handle_files_key(&mut self, key: KeyEvent) {
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
            KeyCode::Char('g') => self.select_first_file(),
            KeyCode::Char('G') => self.select_last_file(),
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.preferences.input_mode = InputMode::ListSearch(FocusPane::Files);
                self.runtime.status = format!("/{}", self.search.file_query);
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.set_focus(FocusPane::Diff),
            _ => {}
        }
    }

    pub(super) fn handle_commits_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if clear_commit_visual_anchor(&mut self.commit_ui.visual_anchor) {
                    self.runtime.status = "Commit visual range off".to_owned();
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
            KeyCode::Char('g') => self.select_first_commit(),
            KeyCode::Char('G') => self.select_last_commit(),
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.preferences.input_mode = InputMode::ListSearch(FocusPane::Commits);
                self.runtime.status = format!("/{}", self.search.commit_query);
            }
            KeyCode::Char('v') => {
                if clear_commit_visual_anchor(&mut self.commit_ui.visual_anchor) {
                    self.runtime.status = "Commit visual range off".to_owned();
                } else {
                    self.commit_ui.visual_anchor = self.selected_commit_full_index();
                    self.runtime.status = "Commit visual range on".to_owned();
                    self.apply_commit_visual_range();
                }
            }
            KeyCode::Char('x') => {
                for row in &mut self.commits {
                    row.selected = false;
                }
                self.commit_ui.visual_anchor = None;
                self.commit_ui.selection_anchor = None;
                self.on_selection_changed();
            }
            KeyCode::Char(' ') => {
                if let Some(idx) = self.selected_commit_full_index()
                    && let Some(row) = self.commits.get_mut(idx)
                {
                    row.selected = !row.selected;
                    self.commit_ui.selection_anchor = Some(idx);
                }
                self.commit_ui.visual_anchor = None;
                self.on_selection_changed();
            }
            KeyCode::Enter => {
                if clear_commit_visual_anchor(&mut self.commit_ui.visual_anchor) {
                    self.runtime.status = "Commit visual range off".to_owned();
                } else if let Some(idx) = self.selected_commit_full_index() {
                    select_only_index(&mut self.commits, idx);
                    self.commit_ui.selection_anchor = Some(idx);
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
        if let Some(op) = self.diff_ui.pending_op {
            if key.modifiers == KeyModifiers::NONE {
                match (op, key.code) {
                    (DiffPendingOp::Z, KeyCode::Char('z')) => {
                        self.diff_ui.pending_op = None;
                        self.align_diff_cursor_middle();
                        return;
                    }
                    (DiffPendingOp::Z, KeyCode::Char('t')) => {
                        self.diff_ui.pending_op = None;
                        self.align_diff_cursor_top();
                        return;
                    }
                    (DiffPendingOp::Z, KeyCode::Char('b')) => {
                        self.diff_ui.pending_op = None;
                        self.align_diff_cursor_bottom();
                        return;
                    }
                    _ => {}
                }
            }
            self.diff_ui.pending_op = None;
        }

        if let Some(forward) = diff_search_repeat_direction(key) {
            self.repeat_diff_search(forward);
            return;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_diff_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_diff_cursor(-1),
            KeyCode::Left => self.move_diff_block_cursor(-1),
            KeyCode::Right => self.move_diff_block_cursor(1),
            KeyCode::Esc => {
                let had_visual = self.diff_ui.visual_selection.is_some();
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
            KeyCode::Char('g') => {
                self.diff_position.cursor = 0;
                self.ensure_cursor_visible();
            }
            KeyCode::Char('G') => {
                if !self.rendered_diff.is_empty() {
                    self.diff_position.cursor = self.rendered_diff.len() - 1;
                    self.ensure_cursor_visible();
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
                self.diff_ui.pending_op = Some(DiffPendingOp::Z);
            }
            KeyCode::Char('[') if key.modifiers == KeyModifiers::NONE => self.move_prev_hunk(),
            KeyCode::Char(']') if key.modifiers == KeyModifiers::NONE => self.move_next_hunk(),
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.preferences.input_mode = InputMode::DiffSearch;
                self.search.diff_buffer.clear();
                self.diff_ui.pending_op = None;
                self.runtime.status = "/".to_owned();
            }
            KeyCode::Char('*')
                if key.modifiers == KeyModifiers::SHIFT || key.modifiers == KeyModifiers::NONE =>
            {
                self.search_word_under_diff_cursor(true);
            }
            KeyCode::Char('#')
                if key.modifiers == KeyModifiers::SHIFT || key.modifiers == KeyModifiers::NONE =>
            {
                self.search_word_under_diff_cursor(false);
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                if self.rendered_diff.is_empty() {
                    return;
                }
                if self.diff_ui.visual_selection.is_some() {
                    self.clear_diff_visual_selection();
                    self.runtime.status = "Diff visual range off".to_owned();
                } else {
                    self.diff_ui.mouse_anchor = None;
                    self.diff_ui.visual_selection = Some(DiffVisualSelection {
                        anchor: self.diff_position.cursor,
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
                self.preferences.input_mode = InputMode::CommentCreate;
                self.comment_editor.buffer.clear();
                self.comment_editor.cursor = 0;
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = None;
                self.diff_ui.pending_op = None;
                self.runtime.status =
                    "Comment mode: Enter save, Alt+Enter newline, Esc cancel".to_owned();
            }
            KeyCode::Char('e') => {
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

    fn search_word_under_diff_cursor(&mut self, forward: bool) {
        let Some(line) = self.rendered_diff.get(self.diff_position.cursor) else {
            self.runtime.status = "No diff line under cursor".to_owned();
            return;
        };
        let display_text = line_plain_text(&line.line);
        let Some(word) = word_at_char_column(&display_text, self.diff_ui.block_cursor_col) else {
            self.runtime.status = "No searchable word under diff block cursor".to_owned();
            return;
        };
        self.execute_diff_search(&word, forward);
    }

    fn clear_diff_search(&mut self) -> bool {
        self.search.diff_buffer.clear();
        self.search.diff_query.take().is_some()
    }
}

pub(super) fn clear_commit_visual_anchor(visual_anchor: &mut Option<usize>) -> bool {
    visual_anchor.take().is_some()
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
