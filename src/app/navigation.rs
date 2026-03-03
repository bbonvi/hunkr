use super::services::status_workflow;
use crate::app::*;

impl App {
    pub(super) fn scroll_diff_viewport(&mut self, delta: isize) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }

        let max_idx = self.domain.rendered_diff.len() - 1;
        let next = scrolled_diff_position_preserving_offset(
            self.domain.diff_position,
            delta,
            self.max_diff_scroll(),
            max_idx,
        );
        self.set_diff_scroll(next.scroll);
        self.domain.diff_position.cursor = next.cursor.min(max_idx);
        self.sync_diff_visual_bounds();
        self.ensure_cursor_visible();
    }

    pub(super) fn move_file_cursor(&mut self, delta: isize) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            return;
        }

        let current = self.ui.file_ui.list_state.selected().unwrap_or(0) as isize;
        let len = visible.len() as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        self.select_file_row(next);
    }

    pub(super) fn scroll_file_list_lines(&mut self, delta: isize) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            return;
        }

        let len = visible.len() as isize;
        let current = self.ui.file_ui.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        if next == current as usize {
            return;
        }

        self.select_file_row(next);
    }

    pub(super) fn page_files(&mut self, multiplier: f32) {
        let step = page_step(self.ui.diff_ui.pane_rects.files.height, multiplier);
        self.move_file_cursor(step);
    }

    pub(super) fn select_first_file(&mut self) {
        if !self.visible_file_indices().is_empty() {
            self.select_file_row(0);
        }
    }

    pub(super) fn select_last_file(&mut self) {
        let visible = self.visible_file_indices();
        if !visible.is_empty() {
            self.select_file_row(visible.len() - 1);
        }
    }

    pub(super) fn select_file_row(&mut self, visible_idx: usize) {
        let visible = self.visible_file_indices();
        let Some(full_idx) = visible.get(visible_idx).copied() else {
            return;
        };
        self.ui.file_ui.list_state.select(Some(visible_idx));
        let Some(path) = file_focus_target_path_for_visible_row(
            &self.domain.file_rows,
            &visible,
            visible_idx,
            full_idx,
        ) else {
            return;
        };
        if self.ui.diff_cache.selected_file.as_deref() == Some(path.as_str()) {
            return;
        }

        self.persist_selected_file_position();
        self.ui.diff_cache.selected_file = Some(path.clone());
        self.restore_diff_position(&path);
        self.sync_diff_cursor_to_content_bounds();
    }

    pub(super) fn move_commit_cursor(&mut self, delta: isize) {
        let visible = self.visible_commit_indices();
        if visible.is_empty() {
            return;
        }
        let len = visible.len() as isize;
        let current = self.ui.commit_ui.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        self.ui.commit_ui.list_state.select(Some(next));

        if self.ui.commit_ui.visual_anchor.is_some() {
            self.apply_commit_visual_range();
        }
    }

    pub(super) fn scroll_commit_list_lines(&mut self, delta: isize) {
        self.move_commit_cursor(delta);
    }

    pub(super) fn page_commits(&mut self, multiplier: f32) {
        let step = page_step(self.ui.diff_ui.pane_rects.commits.height, multiplier);
        self.move_commit_cursor(step);
    }

    pub(super) fn select_first_commit(&mut self) {
        if self.visible_commit_indices().is_empty() {
            return;
        }
        self.ui.commit_ui.list_state.select(Some(0));
        if self.ui.commit_ui.visual_anchor.is_some() {
            self.apply_commit_visual_range();
        }
    }

    pub(super) fn select_last_commit(&mut self) {
        let visible = self.visible_commit_indices();
        if visible.is_empty() {
            return;
        }
        self.ui.commit_ui.list_state.select(Some(visible.len() - 1));
        if self.ui.commit_ui.visual_anchor.is_some() {
            self.apply_commit_visual_range();
        }
    }

    pub(super) fn select_commit_row_with_mouse(
        &mut self,
        visible_idx: usize,
        modifiers: KeyModifiers,
    ) -> Option<usize> {
        let visible = self.visible_commit_indices();
        let full_idx = visible.get(visible_idx).copied()?;

        let prior_cursor_full_idx = self
            .ui
            .commit_ui
            .list_state
            .selected()
            .and_then(|idx| visible.get(idx).copied());
        self.ui.commit_ui.list_state.select(Some(visible_idx));

        match commit_mouse_selection_mode(modifiers) {
            CommitMouseSelectionMode::Replace => {
                select_only_index(&mut self.domain.commits, full_idx);
                self.ui.commit_ui.selection_anchor = Some(full_idx);
            }
            CommitMouseSelectionMode::Toggle => {
                if let Some(row) = self.domain.commits.get_mut(full_idx) {
                    row.selected = !row.selected;
                }
                self.ui.commit_ui.selection_anchor = Some(full_idx);
            }
            CommitMouseSelectionMode::Range => {
                let anchor = self
                    .ui
                    .commit_ui
                    .selection_anchor
                    .filter(|anchor| visible.contains(anchor))
                    .or(prior_cursor_full_idx.filter(|cursor| visible.contains(cursor)))
                    .unwrap_or(full_idx);
                apply_range_selection(&mut self.domain.commits, anchor, full_idx);
                self.ui.commit_ui.selection_anchor = Some(anchor);
            }
        }

        self.on_selection_changed();
        Some(full_idx)
    }

    pub(super) fn apply_commit_visual_range(&mut self) {
        let Some(anchor) = self.ui.commit_ui.visual_anchor else {
            return;
        };
        let Some(cursor) = self.selected_commit_full_index() else {
            return;
        };

        let start = min(anchor, cursor);
        let end = max(anchor, cursor);
        apply_range_selection(&mut self.domain.commits, start, end);
        self.on_selection_changed_debounced();
    }

    /// Apply a status update to the active commit selection, or to the cursor row when no
    /// explicit selection exists.
    pub(super) fn set_contextual_commit_status(&mut self, status: ReviewStatus) {
        let has_selection = self.domain.commits.iter().any(|row| row.selected);
        if has_selection {
            let ids = self
                .domain
                .commits
                .iter()
                .filter(|row| row.selected && !row.is_uncommitted)
                .map(|row| row.info.id.clone())
                .collect::<BTreeSet<_>>();
            if ids.is_empty() {
                self.runtime.status = "No selected committed revisions".to_owned();
                return;
            }
            self.set_status_for_ids(&ids, status);
            return;
        }

        let Some(idx) = self.selected_commit_full_index() else {
            return;
        };
        let Some(row) = self.domain.commits.get(idx) else {
            return;
        };
        if row.is_uncommitted {
            self.runtime.status = "Cannot set review status for uncommitted changes".to_owned();
            return;
        }
        let ids = BTreeSet::from([row.info.id.clone()]);
        self.set_status_for_ids(&ids, status);
    }

    pub(super) fn cycle_commit_status_filter(&mut self) {
        let prior_selected = self.ui.commit_ui.list_state.selected();
        let prior_top = self.ui.commit_ui.list_state.offset();
        let fallback_visible_idx = self.ui.commit_ui.list_state.selected();
        self.ui.commit_ui.status_filter = self.ui.commit_ui.status_filter.next();
        let deselected = deselect_rows_outside_status_filter(
            &mut self.domain.commits,
            self.ui.commit_ui.status_filter,
        );
        if deselected > 0 {
            self.ui.commit_ui.visual_anchor = None;
            if let Err(err) = self.rebuild_selection_dependent_views() {
                self.runtime.status = format!("failed to rebuild diff: {err:#}");
                return;
            }
        }
        self.sync_commit_cursor_for_filters(None, fallback_visible_idx);
        if let Some(next_top) = list_scroll_preserving_cursor_to_top_offset(
            prior_selected,
            prior_top,
            self.ui.commit_ui.list_state.selected(),
        ) {
            *self.ui.commit_ui.list_state.offset_mut() = next_top;
        }
        self.runtime.status = if deselected == 0 {
            format!("Status Filter: {}", self.ui.commit_ui.status_filter.label())
        } else {
            format!(
                "Status Filter: {} (deselected {} hidden commit(s))",
                self.ui.commit_ui.status_filter.label(),
                deselected
            )
        };
    }

    pub(super) fn set_status_for_ids(&mut self, ids: &BTreeSet<String>, status: ReviewStatus) {
        status_workflow::set_status_for_ids(self, ids, status);
    }

    pub(super) fn move_diff_cursor(&mut self, delta: isize) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        let len = self.domain.rendered_diff.len() as isize;
        let next = (self.domain.diff_position.cursor as isize + delta).clamp(0, len - 1) as usize;
        self.domain.diff_position.cursor = next;
        self.ensure_cursor_visible();
    }

    pub(super) fn move_diff_block_cursor(&mut self, delta: isize) {
        if self.domain.rendered_diff.is_empty() {
            self.reset_diff_block_cursor();
            return;
        }

        let line_len = self.current_diff_line_char_len();
        if line_len == 0 {
            self.reset_diff_block_cursor();
            return;
        }
        let max_col = line_len.saturating_sub(1) as isize;
        let next = (self.ui.diff_ui.block_cursor_col as isize + delta).clamp(0, max_col) as usize;
        self.ui.diff_ui.block_cursor_col = next;
        self.ui.diff_ui.block_cursor_goal = next;
    }

    pub(super) fn move_diff_block_cursor_next_word_start(&mut self, big_word: bool) {
        let current_col = self.ui.diff_ui.block_cursor_col;
        let Some(line) = self.current_diff_line_text() else {
            self.reset_diff_block_cursor();
            return;
        };
        if let Some(col) = vim_next_word_start_column(line, current_col, big_word)
            .filter(|col| *col != current_col)
        {
            self.set_diff_block_cursor_col(col);
            return;
        }

        let at_line_end = match line_last_char_column(line) {
            Some(last_col) => current_col >= last_col,
            None => true,
        };
        if !at_line_end {
            return;
        }
        let Some(next_row) = self.next_diff_row_with_content(self.domain.diff_position.cursor)
        else {
            return;
        };
        self.set_diff_cursor(next_row);
        let Some(next_line) = self.current_diff_line_text() else {
            self.reset_diff_block_cursor();
            return;
        };
        let next_col = next_line
            .chars()
            .position(|ch| !ch.is_whitespace())
            .unwrap_or(0);
        self.set_diff_block_cursor_col(next_col);
    }

    pub(super) fn move_diff_block_cursor_prev_word_start(&mut self, big_word: bool) {
        let Some(line) = self.current_diff_line_text() else {
            self.reset_diff_block_cursor();
            return;
        };
        if let Some(col) =
            vim_prev_word_start_column(line, self.ui.diff_ui.block_cursor_col, big_word)
        {
            self.set_diff_block_cursor_col(col);
        }
    }

    pub(super) fn move_diff_block_cursor_next_word_end(&mut self, big_word: bool) {
        let current_col = self.ui.diff_ui.block_cursor_col;
        let Some(line) = self.current_diff_line_text() else {
            self.reset_diff_block_cursor();
            return;
        };
        if let Some(col) =
            vim_next_word_end_column(line, current_col, big_word).filter(|col| *col != current_col)
        {
            self.set_diff_block_cursor_col(col);
            return;
        }

        let at_line_end = match line_last_char_column(line) {
            Some(last_col) => current_col >= last_col,
            None => true,
        };
        if !at_line_end {
            return;
        }
        let Some(next_row) = self.next_diff_row_with_content(self.domain.diff_position.cursor)
        else {
            return;
        };
        self.set_diff_cursor(next_row);
        let Some(next_line) = self.current_diff_line_text() else {
            self.reset_diff_block_cursor();
            return;
        };
        let start_col = next_line
            .chars()
            .position(|ch| !ch.is_whitespace())
            .unwrap_or(0);
        if let Some(col) = vim_next_word_end_column(next_line, start_col, big_word) {
            self.set_diff_block_cursor_col(col);
        }
    }

    pub(super) fn set_diff_block_cursor_to_line_first_non_whitespace(&mut self) {
        let Some(line) = self.current_diff_line_text() else {
            self.reset_diff_block_cursor();
            return;
        };
        if let Some(col) = line_first_non_whitespace_column(line) {
            self.set_diff_block_cursor_col(col);
        }
    }

    pub(super) fn set_diff_block_cursor_to_line_end(&mut self) {
        let Some(line) = self.current_diff_line_text() else {
            self.reset_diff_block_cursor();
            return;
        };
        if let Some(col) = line_last_char_column(line) {
            self.set_diff_block_cursor_col(col);
        }
    }

    pub(super) fn set_diff_block_cursor_col(&mut self, col: usize) {
        self.ui.diff_ui.block_cursor_goal = col;
        self.sync_diff_block_cursor_to_cursor_line();
    }

    pub(super) fn sync_diff_block_cursor_to_cursor_line(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            self.reset_diff_block_cursor();
            return;
        }

        let line_len = self.current_diff_line_char_len();
        if line_len == 0 {
            self.ui.diff_ui.block_cursor_col = 0;
            return;
        }

        let max_col = line_len.saturating_sub(1);
        self.ui.diff_ui.block_cursor_col = self.ui.diff_ui.block_cursor_goal.min(max_col);
    }

    pub(super) fn set_diff_cursor(&mut self, absolute_row: usize) {
        if self.domain.rendered_diff.is_empty() {
            self.domain.diff_position = DiffPosition::default();
            self.sync_diff_block_cursor_to_cursor_line();
            return;
        }
        self.domain.diff_position.cursor = absolute_row.min(self.domain.rendered_diff.len() - 1);
        self.ensure_cursor_visible();
    }

    pub(super) fn page_diff(&mut self, multiplier: f32) {
        let step = page_step(self.ui.diff_ui.pane_rects.diff.height, multiplier);
        self.move_diff_cursor(step);
    }

    pub(super) fn align_diff_cursor_top(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        self.set_diff_scroll(self.domain.diff_position.cursor);
        self.runtime.status = "zt".to_owned();
    }

    pub(super) fn align_diff_cursor_middle(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        self.center_diff_cursor_in_viewport();
        self.runtime.status = "zz".to_owned();
    }

    pub(super) fn align_diff_cursor_bottom(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        let visible = self.visible_diff_rows();
        let scroll = self
            .domain
            .diff_position
            .cursor
            .saturating_sub(visible.saturating_sub(1));
        self.set_diff_scroll(scroll);
        self.runtime.status = "zb".to_owned();
    }

    pub(super) fn move_prev_hunk(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        let Some(idx) =
            prev_hunk_header_index(&self.domain.rendered_diff, self.domain.diff_position.cursor)
        else {
            self.runtime.status = "No previous hunk".to_owned();
            return;
        };
        self.set_diff_cursor(idx);
        self.set_diff_scroll(self.domain.diff_position.cursor);
        self.runtime.status = format!("hunk {}", idx.saturating_add(1));
    }

    pub(super) fn move_next_hunk(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        let Some(idx) =
            next_hunk_header_index(&self.domain.rendered_diff, self.domain.diff_position.cursor)
        else {
            self.runtime.status = "No next hunk".to_owned();
            return;
        };
        self.set_diff_cursor(idx);
        self.set_diff_scroll(self.domain.diff_position.cursor);
        self.runtime.status = format!("hunk {}", idx.saturating_add(1));
    }

    pub(super) fn sticky_commit_banner_index_for_scroll(&self, scroll: usize) -> Option<usize> {
        if scroll == 0 || self.domain.rendered_diff.is_empty() {
            return None;
        }
        let top = scroll.min(self.domain.rendered_diff.len().saturating_sub(1));
        let file_range_idx = self.file_range_index_for_line(top)?;
        let file_range = self.ui.diff_cache.file_ranges.get(file_range_idx)?;
        for idx in (file_range.start..=top).rev() {
            let is_commit_banner = self.domain.rendered_diff[idx]
                .anchor
                .as_ref()
                .is_some_and(is_commit_line_anchor);
            if is_commit_banner {
                return (idx < top).then_some(idx);
            }
        }
        None
    }

    pub(super) fn sticky_file_banner_index_for_scroll(&self, scroll: usize) -> Option<usize> {
        if scroll == 0 || self.domain.rendered_diff.is_empty() {
            return None;
        }
        let top = scroll.min(self.domain.rendered_diff.len().saturating_sub(1));
        let file_range_idx = self.file_range_index_for_line(top)?;
        let file_range = self.ui.diff_cache.file_ranges.get(file_range_idx)?;
        (file_range.start < top).then_some(file_range.start)
    }

    pub(super) fn sticky_banner_indexes_for_scroll(
        &self,
        scroll: usize,
        viewport_rows: usize,
    ) -> Vec<usize> {
        compose_sticky_banner_indexes(
            self.sticky_file_banner_index_for_scroll(scroll),
            self.sticky_commit_banner_index_for_scroll(scroll),
            viewport_rows,
        )
    }

    pub(super) fn visible_diff_rows_for_scroll(&self, scroll: usize) -> usize {
        let viewport_rows = self
            .ui
            .diff_ui
            .pane_rects
            .diff
            .height
            .saturating_sub(2)
            .max(1) as usize;
        let sticky_rows = self
            .sticky_banner_indexes_for_scroll(scroll, viewport_rows)
            .len();
        viewport_rows.saturating_sub(sticky_rows).max(1)
    }

    pub(super) fn visible_diff_rows(&self) -> usize {
        self.visible_diff_rows_for_scroll(self.domain.diff_position.scroll)
    }

    pub(super) fn max_diff_scroll(&self) -> usize {
        let len = self.domain.rendered_diff.len();
        if len == 0 {
            return 0;
        }
        let base_rows = self.visible_diff_rows_for_scroll(0).min(len);
        let mut max_scroll = len.saturating_sub(base_rows);

        let end_rows = self
            .visible_diff_rows_for_scroll(len.saturating_sub(1))
            .min(len);
        let end_max_scroll = len.saturating_sub(end_rows);
        if end_max_scroll > max_scroll {
            max_scroll = end_max_scroll;
        }

        max_scroll
    }

    pub(super) fn set_diff_scroll(&mut self, scroll: usize) {
        self.domain.diff_position.scroll = scroll.min(self.max_diff_scroll());
    }

    pub(super) fn ensure_cursor_visible(&mut self) {
        let visible = self.visible_diff_rows();
        let next_scroll = diff_scroll_with_scrolloff(
            self.domain.diff_position.cursor,
            self.domain.diff_position.scroll,
            visible,
            DIFF_CURSOR_SCROLL_OFF_LINES,
        );
        self.set_diff_scroll(next_scroll);
        self.sync_diff_block_cursor_to_cursor_line();
        self.sync_selected_file_to_cursor();
    }

    /// Centers the current diff cursor row in the visible viewport when possible.
    pub(super) fn center_diff_cursor_in_viewport(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        let visible = self.visible_diff_rows();
        let centered_scroll = self.domain.diff_position.cursor.saturating_sub(visible / 2);
        self.set_diff_scroll(centered_scroll);
    }

    pub(super) fn restore_diff_position(&mut self, path: &str) {
        let Some((start, end)) = self.file_range_for_path(path) else {
            self.domain.diff_position = DiffPosition::default();
            return;
        };
        if end <= start {
            self.domain.diff_position = DiffPosition::default();
            return;
        }

        let local = self
            .ui
            .diff_cache
            .positions
            .get(path)
            .copied()
            .unwrap_or_default();
        let max_local = end - start - 1;
        self.domain.diff_position = DiffPosition {
            scroll: start + local.scroll.min(max_local),
            cursor: start + local.cursor.min(max_local),
        };
    }

    pub(super) fn persist_selected_file_position(&mut self) {
        let Some(path) = self.ui.diff_cache.selected_file.clone() else {
            return;
        };
        let Some((start, end)) = self.file_range_for_path(&path) else {
            return;
        };
        if end <= start {
            return;
        }

        let max_local = end - start - 1;
        self.ui.diff_cache.positions.insert(
            path,
            DiffPosition {
                scroll: self
                    .domain
                    .diff_position
                    .scroll
                    .saturating_sub(start)
                    .min(max_local),
                cursor: self
                    .domain
                    .diff_position
                    .cursor
                    .saturating_sub(start)
                    .min(max_local),
            },
        );
    }

    pub(super) fn sync_selected_file_to_cursor(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        let cursor = self
            .domain
            .diff_position
            .cursor
            .min(self.domain.rendered_diff.len() - 1);
        let Some(path) = self
            .file_path_for_line(cursor)
            .map(|value| value.to_owned())
        else {
            return;
        };

        if self.ui.diff_cache.selected_file.as_deref() != Some(path.as_str()) {
            self.persist_selected_file_position();
            self.ui.diff_cache.selected_file = Some(path.clone());
        }
        let visible_file_indices = self.visible_file_indices();
        if should_preserve_directory_row_focus(
            self.ui.preferences.focused,
            current_visible_file_row_selectable(
                &self.domain.file_rows,
                &visible_file_indices,
                self.ui.file_ui.list_state.selected(),
            ),
        ) {
            return;
        }
        self.select_file_row_for_path(&path);
    }

    pub(super) fn sync_diff_visual_bounds(&mut self) {
        let Some(visual) = self.ui.diff_ui.visual_selection else {
            return;
        };
        if self.domain.rendered_diff.is_empty() {
            return;
        }
        let max_idx = self.domain.rendered_diff.len() - 1;
        let clamped_anchor = visual.anchor.min(max_idx);
        if clamped_anchor != visual.anchor {
            self.ui.diff_ui.visual_selection = Some(DiffVisualSelection {
                anchor: clamped_anchor,
                origin: visual.origin,
            });
        }
    }

    pub(super) fn clear_diff_visual_selection(&mut self) {
        self.ui.diff_ui.visual_selection = None;
        self.ui.diff_ui.mouse_anchor = None;
    }

    pub(super) fn set_focus(&mut self, next: FocusPane) {
        if self.ui.preferences.focused == next {
            return;
        }

        let cleared_commit_visual = self.ui.commit_ui.visual_anchor.is_some();
        let cleared_diff_visual = self.ui.diff_ui.visual_selection.is_some();
        if self.ui.preferences.focused == FocusPane::Commits && next != FocusPane::Commits {
            self.flush_pending_selection_rebuild();
        }
        self.ui.preferences.focused = next;
        self.ui.commit_ui.visual_anchor = None;
        self.ui.commit_ui.mouse_anchor = None;
        self.ui.commit_ui.mouse_dragging = false;
        self.ui.commit_ui.mouse_drag_mode = None;
        self.ui.commit_ui.mouse_drag_baseline = None;
        self.clear_diff_visual_selection();
        self.ui.diff_ui.pending_op = None;
        if let Some(cleared) =
            focus_change_cleared_selection_note(cleared_commit_visual, cleared_diff_visual)
        {
            self.runtime.status = format!("Focus -> {} ({cleared})", focus_pane_label(next));
        }
    }

    pub(super) fn focus_next(&mut self) {
        let next = match self.ui.preferences.focused {
            FocusPane::Commits => FocusPane::Files,
            FocusPane::Files => FocusPane::Diff,
            FocusPane::Diff => FocusPane::Commits,
        };
        self.set_focus(next);
    }

    pub(super) fn focus_prev(&mut self) {
        let next = match self.ui.preferences.focused {
            FocusPane::Commits => FocusPane::Diff,
            FocusPane::Files => FocusPane::Commits,
            FocusPane::Diff => FocusPane::Files,
        };
        self.set_focus(next);
    }

    pub(super) fn diff_selected_range(&self) -> Option<(usize, usize)> {
        if self.domain.rendered_diff.is_empty() {
            return None;
        }
        let max_idx = self.domain.rendered_diff.len() - 1;
        let cursor = self.domain.diff_position.cursor.min(max_idx);

        if let Some(visual) = self.ui.diff_ui.visual_selection {
            let anchor = visual.anchor.min(max_idx);
            Some((min(anchor, cursor), max(anchor, cursor)))
        } else {
            Some((cursor, cursor))
        }
    }

    pub(super) fn status_counts(&self) -> (usize, usize, usize) {
        let mut unreviewed = 0;
        let mut reviewed = 0;
        let mut issue_found = 0;
        for row in &self.domain.commits {
            if row.is_uncommitted {
                continue;
            }
            match row.status {
                ReviewStatus::Unreviewed => unreviewed += 1,
                ReviewStatus::Reviewed => reviewed += 1,
                ReviewStatus::IssueFound => issue_found += 1,
            }
        }
        (unreviewed, reviewed, issue_found)
    }

    pub(super) fn uncommitted_selected(&self) -> bool {
        self.domain
            .commits
            .iter()
            .any(|row| row.is_uncommitted && row.selected)
    }

    /// Toggles visibility of deleted-file content when cursor is on the deleted-file toggle row.
    pub(super) fn toggle_deleted_file_content_under_cursor(&mut self) -> bool {
        let Some(line) = self
            .domain
            .rendered_diff
            .get(self.domain.diff_position.cursor)
        else {
            return false;
        };
        if line.raw_text != DELETED_FILE_TOGGLE_RAW_TEXT {
            return false;
        }
        let Some(path) = self
            .file_path_for_line(self.domain.diff_position.cursor)
            .map(str::to_owned)
        else {
            return false;
        };
        if !matches!(
            self.domain.aggregate.file_changes.get(&path),
            Some(change) if change.kind == FileChangeKind::Deleted
        ) {
            return false;
        }

        let now_visible = if self.domain.deleted_file_content_visible.contains(&path) {
            self.domain.deleted_file_content_visible.remove(&path);
            false
        } else {
            self.domain
                .deleted_file_content_visible
                .insert(path.clone());
            true
        };

        self.capture_pending_diff_view_anchor();
        self.ui
            .diff_cache
            .rendered_cache
            .retain(|(candidate_path, _), _| candidate_path != &path);
        self.ui.diff_cache.rendered_key = None;
        self.ui.diff_cache.file_ranges.clear();
        self.ui.diff_cache.file_range_by_path.clear();
        self.ensure_rendered_diff();
        self.select_file_row_for_path(&path);
        self.runtime.status = if now_visible {
            format!("Showing deleted content for {path}")
        } else {
            format!("Hiding deleted content for {path}")
        };
        true
    }

    pub(super) fn copy_diff_visual_selection(&mut self) {
        if self.ui.diff_ui.visual_selection.is_none() {
            self.runtime.status = "No diff visual range to copy".to_owned();
            return;
        }
        let Some((start, end)) = self.diff_selected_range() else {
            self.runtime.status = "No diff visual range to copy".to_owned();
            return;
        };

        let payload = self.domain.rendered_diff[start..=end]
            .iter()
            .map(|line| line.raw_text.clone())
            .collect::<Vec<_>>()
            .join("\n");

        self.runtime.status = clipboard_copy_status(
            crate::clipboard::copy_to_clipboard_with_fallbacks(&payload),
            format!("{} diff line(s)", end.saturating_sub(start) + 1),
            "diff selection",
        );

        let post_action = selection_copy_post_action(true, None);
        if matches!(post_action, SelectionCopyPostAction::ClearNow) {
            self.clear_diff_visual_selection();
        }
    }
}

/// Finds the previous hunk-header row, wrapping at boundaries.
pub(super) fn prev_hunk_header_index(lines: &[RenderedDiffLine], cursor: usize) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    let cursor = cursor.min(lines.len().saturating_sub(1));
    let hunk_indexes = hunk_header_indexes(lines);
    let Some(last_before) = hunk_indexes.iter().copied().rev().find(|&idx| idx < cursor) else {
        return hunk_indexes.last().copied();
    };
    Some(last_before)
}

/// Finds the next hunk-header row, wrapping at boundaries.
pub(super) fn next_hunk_header_index(lines: &[RenderedDiffLine], cursor: usize) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    let cursor = cursor.min(lines.len().saturating_sub(1));
    let hunk_indexes = hunk_header_indexes(lines);
    let Some(first_after) = hunk_indexes.iter().copied().find(|&idx| idx > cursor) else {
        return hunk_indexes.first().copied();
    };
    Some(first_after)
}

fn hunk_header_indexes(lines: &[RenderedDiffLine]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| is_hunk_header_line(line).then_some(idx))
        .collect()
}

impl App {
    pub(super) fn diff_row_visible_in_viewport(&self, row: usize) -> bool {
        if self.domain.rendered_diff.is_empty() {
            return false;
        }
        let viewport_rows = self
            .ui
            .diff_ui
            .pane_rects
            .diff
            .height
            .saturating_sub(2)
            .max(1) as usize;
        let sticky =
            self.sticky_banner_indexes_for_scroll(self.domain.diff_position.scroll, viewport_rows);
        if sticky.contains(&row) {
            return true;
        }
        let body_rows = viewport_rows
            .saturating_sub(sticky.len().min(viewport_rows.saturating_sub(1)))
            .max(1);
        row >= self.domain.diff_position.scroll
            && row < self.domain.diff_position.scroll.saturating_add(body_rows)
    }

    fn current_diff_line_text(&self) -> Option<&str> {
        self.domain
            .rendered_diff
            .get(self.domain.diff_position.cursor)
            .map(|line| line.raw_text.as_str())
    }

    fn next_diff_row_with_content(&self, current_row: usize) -> Option<usize> {
        let start = current_row.saturating_add(1);
        (start..self.domain.rendered_diff.len())
            .find(|idx| !self.domain.rendered_diff[*idx].raw_text.is_empty())
    }

    fn current_diff_line_char_len(&self) -> usize {
        self.current_diff_line_text()
            .map(|line| line.chars().count())
            .unwrap_or(0)
    }

    fn reset_diff_block_cursor(&mut self) {
        self.ui.diff_ui.block_cursor_col = 0;
        self.ui.diff_ui.block_cursor_goal = 0;
    }
}

fn file_focus_target_path_for_visible_row(
    file_rows: &[TreeRow],
    visible: &[usize],
    visible_idx: usize,
    full_idx: usize,
) -> Option<String> {
    let selected_row = file_rows.get(full_idx)?;
    if selected_row.selectable {
        return selected_row.path.clone();
    }

    let selected_depth = selected_row.depth;
    for descendant_full_idx in visible.iter().skip(visible_idx.saturating_add(1)) {
        let Some(descendant) = file_rows.get(*descendant_full_idx) else {
            continue;
        };
        if descendant.depth <= selected_depth {
            break;
        }
        if descendant.selectable {
            return descendant.path.clone();
        }
    }
    None
}

/// Resolves whether the currently selected visible file-tree row is selectable.
fn current_visible_file_row_selectable(
    file_rows: &[TreeRow],
    visible_file_indices: &[usize],
    selected_visible_idx: Option<usize>,
) -> Option<bool> {
    let selected_visible_idx = selected_visible_idx?;
    let full_idx = *visible_file_indices.get(selected_visible_idx)?;
    file_rows.get(full_idx).map(|row| row.selectable)
}

/// Keeps directory cursor focus stable while browsing the Files pane.
fn should_preserve_directory_row_focus(
    focused: FocusPane,
    selected_row_selectable: Option<bool>,
) -> bool {
    focused == FocusPane::Files && selected_row_selectable == Some(false)
}

fn focus_pane_label(pane: FocusPane) -> &'static str {
    match pane {
        FocusPane::Commits => "Commits",
        FocusPane::Files => "Files",
        FocusPane::Diff => "Diff",
    }
}

fn focus_change_cleared_selection_note(
    cleared_commit_visual: bool,
    cleared_diff_visual: bool,
) -> Option<&'static str> {
    match (cleared_commit_visual, cleared_diff_visual) {
        (true, true) => Some("cleared commit and diff visual ranges"),
        (true, false) => Some("cleared commit visual range"),
        (false, true) => Some("cleared diff visual range"),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FocusPane, TreeRow, current_visible_file_row_selectable,
        file_focus_target_path_for_visible_row, should_preserve_directory_row_focus,
    };

    fn row(path: Option<&str>, depth: usize, selectable: bool) -> TreeRow {
        TreeRow {
            label: String::new(),
            path: path.map(str::to_owned),
            depth,
            selectable,
            modified_ts: None,
            change: None,
        }
    }

    #[test]
    fn directory_focus_targets_first_visible_descendant_file() {
        let rows = vec![
            row(None, 0, false),
            row(None, 1, false),
            row(Some("src/app/main.rs"), 2, true),
            row(Some("src/lib.rs"), 1, true),
        ];
        let visible = vec![0, 1, 2, 3];

        assert_eq!(
            file_focus_target_path_for_visible_row(&rows, &visible, 0, 0).as_deref(),
            Some("src/app/main.rs")
        );
        assert_eq!(
            file_focus_target_path_for_visible_row(&rows, &visible, 1, 1).as_deref(),
            Some("src/app/main.rs")
        );
    }

    #[test]
    fn focus_target_returns_none_for_directory_without_visible_file_descendant() {
        let rows = vec![row(None, 0, false), row(Some("tests/mod.rs"), 0, true)];
        let visible = vec![0];

        assert!(file_focus_target_path_for_visible_row(&rows, &visible, 0, 0).is_none());
    }

    #[test]
    fn preserve_directory_focus_only_in_files_pane() {
        assert!(should_preserve_directory_row_focus(
            FocusPane::Files,
            Some(false)
        ));
        assert!(!should_preserve_directory_row_focus(
            FocusPane::Diff,
            Some(false)
        ));
        assert!(!should_preserve_directory_row_focus(
            FocusPane::Files,
            Some(true)
        ));
    }

    #[test]
    fn selected_visible_row_selectable_resolves_directory_vs_file() {
        let rows = vec![row(None, 0, false), row(Some("src/main.rs"), 1, true)];
        let visible = vec![0, 1];

        assert_eq!(
            current_visible_file_row_selectable(&rows, &visible, Some(0)),
            Some(false)
        );
        assert_eq!(
            current_visible_file_row_selectable(&rows, &visible, Some(1)),
            Some(true)
        );
        assert_eq!(
            current_visible_file_row_selectable(&rows, &visible, Some(9)),
            None
        );
    }
}
