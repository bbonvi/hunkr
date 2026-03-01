use super::*;

impl App {
    pub(super) fn scroll_diff_viewport(&mut self, delta: isize) {
        if self.rendered_diff.is_empty() {
            return;
        }

        let max_idx = self.rendered_diff.len() - 1;
        let next = scrolled_diff_position_preserving_offset(
            self.diff_position,
            delta,
            self.max_diff_scroll(),
            max_idx,
        );
        self.set_diff_scroll(next.scroll);
        self.diff_position.cursor = next.cursor.min(max_idx);
        self.sync_diff_visual_bounds();
        self.ensure_cursor_visible();
    }

    pub(super) fn move_file_cursor(&mut self, delta: isize) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            return;
        }

        let mut idx = self.file_ui.list_state.selected().unwrap_or(0) as isize;
        let len = visible.len() as isize;
        loop {
            idx = (idx + delta).clamp(0, len - 1);
            if self.file_rows[visible[idx as usize]].selectable || idx == 0 || idx == len - 1 {
                break;
            }
            if (delta > 0 && idx == len - 1) || (delta < 0 && idx == 0) {
                break;
            }
        }

        self.select_file_row(idx as usize);
    }

    pub(super) fn scroll_file_list_lines(&mut self, delta: isize) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            return;
        }

        let len = visible.len() as isize;
        let current = self.file_ui.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        if next == current as usize {
            return;
        }

        self.file_ui.list_state.select(Some(next));
        if self.file_rows[visible[next]].selectable {
            self.select_file_row(next);
        }
    }

    pub(super) fn page_files(&mut self, multiplier: f32) {
        let step = page_step(self.diff_ui.pane_rects.files.height, multiplier);
        self.move_file_cursor(step);
    }

    pub(super) fn select_first_file(&mut self) {
        let visible = self.visible_file_indices();
        if let Some(idx) = visible
            .iter()
            .position(|entry| self.file_rows[*entry].selectable)
        {
            self.select_file_row(idx);
        }
    }

    pub(super) fn select_last_file(&mut self) {
        let visible = self.visible_file_indices();
        if let Some(idx) = visible
            .iter()
            .rposition(|entry| self.file_rows[*entry].selectable)
        {
            self.select_file_row(idx);
        }
    }

    pub(super) fn select_file_row(&mut self, visible_idx: usize) {
        let visible = self.visible_file_indices();
        let Some(full_idx) = visible.get(visible_idx).copied() else {
            return;
        };
        if !self.file_rows[full_idx].selectable {
            return;
        }

        self.persist_selected_file_position();

        self.file_ui.list_state.select(Some(visible_idx));
        let path = self.file_rows[full_idx]
            .path
            .clone()
            .expect("selectable rows always contain path");
        self.diff_cache.selected_file = Some(path.clone());
        self.restore_diff_position(&path);
        self.sync_diff_cursor_to_content_bounds();
    }

    pub(super) fn move_commit_cursor(&mut self, delta: isize) {
        let visible = self.visible_commit_indices();
        if visible.is_empty() {
            return;
        }
        let len = visible.len() as isize;
        let current = self.commit_ui.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        self.commit_ui.list_state.select(Some(next));

        if self.commit_ui.visual_anchor.is_some() {
            self.apply_commit_visual_range();
        }
    }

    pub(super) fn scroll_commit_list_lines(&mut self, delta: isize) {
        self.move_commit_cursor(delta);
    }

    pub(super) fn page_commits(&mut self, multiplier: f32) {
        let step = page_step(self.diff_ui.pane_rects.commits.height, multiplier);
        self.move_commit_cursor(step);
    }

    pub(super) fn select_first_commit(&mut self) {
        if self.visible_commit_indices().is_empty() {
            return;
        }
        self.commit_ui.list_state.select(Some(0));
        if self.commit_ui.visual_anchor.is_some() {
            self.apply_commit_visual_range();
        }
    }

    pub(super) fn select_last_commit(&mut self) {
        let visible = self.visible_commit_indices();
        if visible.is_empty() {
            return;
        }
        self.commit_ui.list_state.select(Some(visible.len() - 1));
        if self.commit_ui.visual_anchor.is_some() {
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
            .commit_ui
            .list_state
            .selected()
            .and_then(|idx| visible.get(idx).copied());
        self.commit_ui.list_state.select(Some(visible_idx));

        match commit_mouse_selection_mode(modifiers) {
            CommitMouseSelectionMode::Replace => {
                select_only_index(&mut self.commits, full_idx);
                self.commit_ui.selection_anchor = Some(full_idx);
            }
            CommitMouseSelectionMode::Toggle => {
                if let Some(row) = self.commits.get_mut(full_idx) {
                    row.selected = !row.selected;
                }
                self.commit_ui.selection_anchor = Some(full_idx);
            }
            CommitMouseSelectionMode::Range => {
                let anchor = self
                    .commit_ui
                    .selection_anchor
                    .filter(|anchor| visible.contains(anchor))
                    .or(prior_cursor_full_idx.filter(|cursor| visible.contains(cursor)))
                    .unwrap_or(full_idx);
                apply_range_selection(&mut self.commits, anchor, full_idx);
                self.commit_ui.selection_anchor = Some(anchor);
            }
        }

        self.on_selection_changed();
        Some(full_idx)
    }

    pub(super) fn apply_commit_visual_range(&mut self) {
        let Some(anchor) = self.commit_ui.visual_anchor else {
            return;
        };
        let Some(cursor) = self.selected_commit_full_index() else {
            return;
        };

        let start = min(anchor, cursor);
        let end = max(anchor, cursor);
        apply_range_selection(&mut self.commits, start, end);
        self.on_selection_changed_debounced();
    }

    /// Apply a status update to the active commit selection, or to the cursor row when no
    /// explicit selection exists.
    pub(super) fn set_contextual_commit_status(&mut self, status: ReviewStatus) {
        let has_selection = self.commits.iter().any(|row| row.selected);
        if has_selection {
            let ids = self
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
        let Some(row) = self.commits.get(idx) else {
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
        let prior_selected = self.commit_ui.list_state.selected();
        let prior_top = self.commit_ui.list_state.offset();
        let fallback_visible_idx = self.commit_ui.list_state.selected();
        self.commit_ui.status_filter = self.commit_ui.status_filter.next();
        let deselected =
            deselect_rows_outside_status_filter(&mut self.commits, self.commit_ui.status_filter);
        if deselected > 0 {
            self.commit_ui.visual_anchor = None;
            if let Err(err) = self.rebuild_selection_dependent_views() {
                self.runtime.status = format!("failed to rebuild diff: {err:#}");
                return;
            }
        }
        self.sync_commit_cursor_for_filters(None, fallback_visible_idx);
        if let Some(next_top) = list_scroll_preserving_cursor_to_top_offset(
            prior_selected,
            prior_top,
            self.commit_ui.list_state.selected(),
        ) {
            *self.commit_ui.list_state.offset_mut() = next_top;
        }
        self.runtime.status = if deselected == 0 {
            format!(
                "Commit status filter: {}",
                self.commit_ui.status_filter.label()
            )
        } else {
            format!(
                "Commit status filter: {} (deselected {} hidden commit(s))",
                self.commit_ui.status_filter.label(),
                deselected
            )
        };
    }

    pub(super) fn set_status_for_ids(&mut self, ids: &BTreeSet<String>, status: ReviewStatus) {
        self.flush_pending_selection_rebuild();
        self.store.set_many_status(
            &mut self.review_state,
            ids.iter().cloned(),
            status,
            self.git.branch_name(),
        );

        apply_status_transition(&mut self.commits, ids, status);
        self.sync_commit_cursor_for_filters(None, self.commit_ui.list_state.selected());

        let save_result = self.store.save(&self.review_state);
        let mut status_message = if let Err(err) = save_result {
            format!("failed to persist status change: {err:#}")
        } else {
            format!("{} commit(s) -> {}", ids.len(), status.as_str())
        };
        let hidden_selected =
            selected_rows_hidden_by_status_filter(&self.commits, self.commit_ui.status_filter);
        if hidden_selected > 0 {
            status_message.push_str(&format!(", {hidden_selected} selected hidden by filter"));
        }

        if status != ReviewStatus::Unreviewed {
            self.commit_ui.visual_anchor = None;
        }
        if let Err(err) = self.sync_comment_report() {
            status_message.push_str(&format!(", review tasks sync failed: {err:#}"));
        }
        self.runtime.status = status_message;
    }

    pub(super) fn move_diff_cursor(&mut self, delta: isize) {
        if self.rendered_diff.is_empty() {
            return;
        }
        let len = self.rendered_diff.len() as isize;
        let next = (self.diff_position.cursor as isize + delta).clamp(0, len - 1) as usize;
        self.diff_position.cursor = next;
        self.ensure_cursor_visible();
    }

    pub(super) fn move_diff_block_cursor(&mut self, delta: isize) {
        if self.rendered_diff.is_empty() {
            self.reset_diff_block_cursor();
            return;
        }

        let line_len = self.current_diff_line_char_len();
        if line_len == 0 {
            self.reset_diff_block_cursor();
            return;
        }
        let max_col = line_len.saturating_sub(1) as isize;
        let next = (self.diff_ui.block_cursor_col as isize + delta).clamp(0, max_col) as usize;
        self.diff_ui.block_cursor_col = next;
        self.diff_ui.block_cursor_goal = next;
    }

    pub(super) fn move_diff_block_cursor_next_word_start(&mut self, big_word: bool) {
        let current_col = self.diff_ui.block_cursor_col;
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
        let Some(next_row) = self.next_diff_row_with_content(self.diff_position.cursor) else {
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
        if let Some(col) = vim_prev_word_start_column(line, self.diff_ui.block_cursor_col, big_word)
        {
            self.set_diff_block_cursor_col(col);
        }
    }

    pub(super) fn move_diff_block_cursor_next_word_end(&mut self, big_word: bool) {
        let current_col = self.diff_ui.block_cursor_col;
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
        let Some(next_row) = self.next_diff_row_with_content(self.diff_position.cursor) else {
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
        self.diff_ui.block_cursor_goal = col;
        self.sync_diff_block_cursor_to_cursor_line();
    }

    pub(super) fn sync_diff_block_cursor_to_cursor_line(&mut self) {
        if self.rendered_diff.is_empty() {
            self.reset_diff_block_cursor();
            return;
        }

        let line_len = self.current_diff_line_char_len();
        if line_len == 0 {
            self.diff_ui.block_cursor_col = 0;
            return;
        }

        let max_col = line_len.saturating_sub(1);
        self.diff_ui.block_cursor_col = self.diff_ui.block_cursor_goal.min(max_col);
    }

    pub(super) fn set_diff_cursor(&mut self, absolute_row: usize) {
        if self.rendered_diff.is_empty() {
            self.diff_position = DiffPosition::default();
            self.sync_diff_block_cursor_to_cursor_line();
            return;
        }
        self.diff_position.cursor = absolute_row.min(self.rendered_diff.len() - 1);
        self.ensure_cursor_visible();
    }

    pub(super) fn page_diff(&mut self, multiplier: f32) {
        let step = page_step(self.diff_ui.pane_rects.diff.height, multiplier);
        self.move_diff_cursor(step);
    }

    pub(super) fn align_diff_cursor_top(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        self.set_diff_scroll(self.diff_position.cursor);
        self.runtime.status = "zt".to_owned();
    }

    pub(super) fn align_diff_cursor_middle(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        let visible = self.visible_diff_rows();
        let scroll = self.diff_position.cursor.saturating_sub(visible / 2);
        self.set_diff_scroll(scroll);
        self.runtime.status = "zz".to_owned();
    }

    pub(super) fn align_diff_cursor_bottom(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        let visible = self.visible_diff_rows();
        let scroll = self
            .diff_position
            .cursor
            .saturating_sub(visible.saturating_sub(1));
        self.set_diff_scroll(scroll);
        self.runtime.status = "zb".to_owned();
    }

    pub(super) fn move_prev_hunk(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        for idx in (0..self.diff_position.cursor).rev() {
            if is_hunk_header_line(&self.rendered_diff[idx]) {
                self.set_diff_cursor(idx);
                self.runtime.status = format!("hunk {}", idx.saturating_add(1));
                return;
            }
        }
        self.runtime.status = "No previous hunk".to_owned();
    }

    pub(super) fn move_next_hunk(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        for idx in self.diff_position.cursor.saturating_add(1)..self.rendered_diff.len() {
            if is_hunk_header_line(&self.rendered_diff[idx]) {
                self.set_diff_cursor(idx);
                self.runtime.status = format!("hunk {}", idx.saturating_add(1));
                return;
            }
        }
        self.runtime.status = "No next hunk".to_owned();
    }

    pub(super) fn sticky_commit_banner_index_for_scroll(&self, scroll: usize) -> Option<usize> {
        if scroll == 0 || self.rendered_diff.is_empty() {
            return None;
        }
        let top = scroll.min(self.rendered_diff.len().saturating_sub(1));
        let file_range_idx = self.file_range_index_for_line(top)?;
        let file_range = self.diff_cache.file_ranges.get(file_range_idx)?;
        for idx in (file_range.start..=top).rev() {
            let is_commit_banner = self.rendered_diff[idx]
                .anchor
                .as_ref()
                .is_some_and(is_commit_anchor);
            if is_commit_banner {
                return (idx < top).then_some(idx);
            }
        }
        None
    }

    pub(super) fn sticky_file_banner_index_for_scroll(&self, scroll: usize) -> Option<usize> {
        if scroll == 0 || self.rendered_diff.is_empty() {
            return None;
        }
        let top = scroll.min(self.rendered_diff.len().saturating_sub(1));
        let file_range_idx = self.file_range_index_for_line(top)?;
        let file_range = self.diff_cache.file_ranges.get(file_range_idx)?;
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
        let viewport_rows = self.diff_ui.pane_rects.diff.height.saturating_sub(2).max(1) as usize;
        let sticky_rows = self
            .sticky_banner_indexes_for_scroll(scroll, viewport_rows)
            .len();
        viewport_rows.saturating_sub(sticky_rows).max(1)
    }

    pub(super) fn visible_diff_rows(&self) -> usize {
        self.visible_diff_rows_for_scroll(self.diff_position.scroll)
    }

    pub(super) fn max_diff_scroll(&self) -> usize {
        let len = self.rendered_diff.len();
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
        self.diff_position.scroll = scroll.min(self.max_diff_scroll());
    }

    pub(super) fn ensure_cursor_visible(&mut self) {
        let visible = self.visible_diff_rows();

        if self.diff_position.cursor < self.diff_position.scroll {
            self.diff_position.scroll = self.diff_position.cursor;
        } else if self.diff_position.cursor >= self.diff_position.scroll + visible {
            self.diff_position.scroll = self.diff_position.cursor + 1 - visible;
        }
        self.sync_diff_block_cursor_to_cursor_line();
        self.sync_selected_file_to_cursor();
    }

    pub(super) fn restore_diff_position(&mut self, path: &str) {
        let Some((start, end)) = self.file_range_for_path(path) else {
            self.diff_position = DiffPosition::default();
            return;
        };
        if end <= start {
            self.diff_position = DiffPosition::default();
            return;
        }

        let local = self
            .diff_cache
            .positions
            .get(path)
            .copied()
            .unwrap_or_default();
        let max_local = end - start - 1;
        self.diff_position = DiffPosition {
            scroll: start + local.scroll.min(max_local),
            cursor: start + local.cursor.min(max_local),
        };
    }

    pub(super) fn persist_selected_file_position(&mut self) {
        let Some(path) = self.diff_cache.selected_file.clone() else {
            return;
        };
        let Some((start, end)) = self.file_range_for_path(&path) else {
            return;
        };
        if end <= start {
            return;
        }

        let max_local = end - start - 1;
        self.diff_cache.positions.insert(
            path,
            DiffPosition {
                scroll: self
                    .diff_position
                    .scroll
                    .saturating_sub(start)
                    .min(max_local),
                cursor: self
                    .diff_position
                    .cursor
                    .saturating_sub(start)
                    .min(max_local),
            },
        );
    }

    pub(super) fn sync_selected_file_to_cursor(&mut self) {
        if self.rendered_diff.is_empty() {
            return;
        }
        let cursor = self.diff_position.cursor.min(self.rendered_diff.len() - 1);
        let Some(path) = self
            .file_path_for_line(cursor)
            .map(|value| value.to_owned())
        else {
            return;
        };

        if self.diff_cache.selected_file.as_deref() != Some(path.as_str()) {
            self.persist_selected_file_position();
            self.diff_cache.selected_file = Some(path.clone());
        }
        self.select_file_row_for_path(&path);
    }

    pub(super) fn sync_diff_visual_bounds(&mut self) {
        let Some(visual) = self.diff_ui.visual_selection else {
            return;
        };
        if self.rendered_diff.is_empty() {
            return;
        }
        let max_idx = self.rendered_diff.len() - 1;
        let clamped_anchor = visual.anchor.min(max_idx);
        if clamped_anchor != visual.anchor {
            self.diff_ui.visual_selection = Some(DiffVisualSelection {
                anchor: clamped_anchor,
                origin: visual.origin,
            });
        }
    }

    pub(super) fn clear_diff_visual_selection(&mut self) {
        self.diff_ui.visual_selection = None;
        self.diff_ui.mouse_anchor = None;
    }

    pub(super) fn set_focus(&mut self, next: FocusPane) {
        if self.preferences.focused == next {
            return;
        }

        if self.preferences.focused == FocusPane::Commits && next != FocusPane::Commits {
            self.flush_pending_selection_rebuild();
        }
        self.preferences.focused = next;
        self.commit_ui.visual_anchor = None;
        self.commit_ui.mouse_anchor = None;
        self.commit_ui.mouse_dragging = false;
        self.commit_ui.mouse_drag_mode = None;
        self.commit_ui.mouse_drag_baseline = None;
        self.clear_diff_visual_selection();
        self.diff_ui.pending_op = None;
    }

    pub(super) fn focus_next(&mut self) {
        let next = match self.preferences.focused {
            FocusPane::Commits => FocusPane::Files,
            FocusPane::Files => FocusPane::Diff,
            FocusPane::Diff => FocusPane::Commits,
        };
        self.set_focus(next);
    }

    pub(super) fn focus_prev(&mut self) {
        let next = match self.preferences.focused {
            FocusPane::Commits => FocusPane::Diff,
            FocusPane::Files => FocusPane::Commits,
            FocusPane::Diff => FocusPane::Files,
        };
        self.set_focus(next);
    }

    pub(super) fn diff_selected_range(&self) -> Option<(usize, usize)> {
        if self.rendered_diff.is_empty() {
            return None;
        }
        let max_idx = self.rendered_diff.len() - 1;
        let cursor = self.diff_position.cursor.min(max_idx);

        if let Some(visual) = self.diff_ui.visual_selection {
            let anchor = visual.anchor.min(max_idx);
            Some((min(anchor, cursor), max(anchor, cursor)))
        } else {
            Some((cursor, cursor))
        }
    }

    pub(super) fn diff_selection_spans_multiple_files(&self) -> bool {
        let Some((start, end)) = self.diff_selected_range() else {
            return false;
        };
        let mut paths = BTreeSet::new();
        for idx in start..=end {
            if let Some(path) = self.file_path_for_line(idx) {
                paths.insert(path);
                if paths.len() > 1 {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn comment_target_from_selection(&self) -> anyhow::Result<Option<CommentTarget>> {
        if self.diff_selection_spans_multiple_files() {
            return Ok(None);
        }

        let selected_commits_ordered = self.selected_commit_ids_oldest_first();
        let Some((start_idx, end_idx)) = self.diff_selected_range() else {
            return Ok(None);
        };
        let mut commit_anchors = Vec::new();
        let mut hunk_anchors = Vec::new();
        let mut commit_paths = BTreeSet::new();
        let mut hunk_paths = BTreeSet::new();
        let mut commit_lines = Vec::new();
        let mut hunk_lines = Vec::new();

        for idx in start_idx..=end_idx {
            let Some(line) = self.rendered_diff.get(idx) else {
                continue;
            };
            if let Some(anchor) = &line.anchor {
                if is_commit_anchor(anchor) {
                    commit_anchors.push(anchor.clone());
                    commit_paths.insert(anchor.file_path.clone());
                    if !line.raw_text.trim().is_empty() {
                        commit_lines.push(line.raw_text.clone());
                    }
                } else {
                    hunk_anchors.push(anchor.clone());
                    hunk_paths.insert(anchor.file_path.clone());
                    if !line.raw_text.trim().is_empty() {
                        hunk_lines.push(line.raw_text.clone());
                    }
                }
            }
        }

        if hunk_anchors.is_empty() && commit_anchors.is_empty() {
            return Ok(None);
        }

        if hunk_anchors.is_empty() {
            if commit_paths.len() > 1 {
                return Ok(None);
            }
            let Some(anchor) = commit_anchors.last().cloned() else {
                return Ok(None);
            };
            let commits = self.git.commits_affecting_selection(
                &selected_commits_ordered,
                &anchor.file_path,
                &[],
            )?;
            let commits = if commits.is_empty() {
                BTreeSet::from([anchor.commit_id.clone()])
            } else {
                commits
            };
            return Ok(Some(CommentTarget {
                kind: CommentTargetKind::Commit,
                start: anchor.clone(),
                end: anchor.clone(),
                commits,
                selected_lines: commit_lines
                    .last()
                    .map(|line| vec![line.clone()])
                    .unwrap_or_default(),
            }));
        }

        if hunk_paths.len() > 1 {
            return Ok(None);
        }

        let Some(start) = hunk_anchors.first().cloned() else {
            return Ok(None);
        };
        let Some(end) = hunk_anchors.last().cloned() else {
            return Ok(None);
        };
        let commits = self.git.commits_affecting_selection(
            &selected_commits_ordered,
            &start.file_path,
            &hunk_lines,
        )?;
        let commits = if commits.is_empty() {
            hunk_anchors
                .iter()
                .map(|anchor| anchor.commit_id.clone())
                .collect::<BTreeSet<_>>()
        } else {
            commits
        };

        Ok(Some(CommentTarget {
            kind: CommentTargetKind::Hunk,
            start,
            end,
            commits,
            selected_lines: hunk_lines,
        }))
    }

    pub(super) fn status_counts(&self) -> (usize, usize, usize, usize) {
        let mut unreviewed = 0;
        let mut reviewed = 0;
        let mut issue_found = 0;
        let mut resolved = 0;
        for row in &self.commits {
            if row.is_uncommitted {
                continue;
            }
            match row.status {
                ReviewStatus::Unreviewed => unreviewed += 1,
                ReviewStatus::Reviewed => reviewed += 1,
                ReviewStatus::IssueFound => issue_found += 1,
                ReviewStatus::Resolved => resolved += 1,
            }
        }
        (unreviewed, reviewed, issue_found, resolved)
    }

    pub(super) fn uncommitted_selected(&self) -> bool {
        self.commits
            .iter()
            .any(|row| row.is_uncommitted && row.selected)
    }

    /// Toggles visibility of deleted-file content when cursor is on the deleted-file toggle row.
    pub(super) fn toggle_deleted_file_content_under_cursor(&mut self) -> bool {
        let Some(line) = self.rendered_diff.get(self.diff_position.cursor) else {
            return false;
        };
        if line.raw_text != DELETED_FILE_TOGGLE_RAW_TEXT {
            return false;
        }
        let Some(path) = self
            .file_path_for_line(self.diff_position.cursor)
            .map(str::to_owned)
        else {
            return false;
        };
        if !matches!(
            self.aggregate.file_changes.get(&path),
            Some(change) if change.kind == FileChangeKind::Deleted
        ) {
            return false;
        }

        let now_visible = if self.deleted_file_content_visible.contains(&path) {
            self.deleted_file_content_visible.remove(&path);
            false
        } else {
            self.deleted_file_content_visible.insert(path.clone());
            true
        };

        self.capture_pending_diff_view_anchor();
        self.diff_cache
            .rendered_cache
            .retain(|(candidate_path, _), _| candidate_path != &path);
        self.diff_cache.rendered_key = None;
        self.diff_cache.file_ranges.clear();
        self.diff_cache.file_range_by_path.clear();
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
        if self.diff_ui.visual_selection.is_none() {
            self.runtime.status = "No diff visual range to copy".to_owned();
            return;
        }
        let Some((start, end)) = self.diff_selected_range() else {
            self.runtime.status = "No diff visual range to copy".to_owned();
            return;
        };

        let payload = self.rendered_diff[start..=end]
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

    /// Copies the active review-task markdown path to clipboard for quick sharing.
    pub(super) fn copy_review_tasks_path(&mut self) {
        let report_path = format_path_with_home_tilde(self.comments.report_path());
        let scope = format!("review tasks path: {report_path}");
        self.runtime.status = clipboard_copy_status(
            crate::clipboard::copy_to_clipboard_with_fallbacks(&report_path),
            &scope,
            &scope,
        );
    }

    pub(super) fn sync_comment_report(&self) -> anyhow::Result<()> {
        self.comments.sync_review_tasks_report(|commit_id| {
            self.store.commit_status(&self.review_state, commit_id)
        })?;
        Ok(())
    }
}

impl App {
    pub(super) fn diff_row_visible_in_viewport(&self, row: usize) -> bool {
        if self.rendered_diff.is_empty() {
            return false;
        }
        let viewport_rows = self.diff_ui.pane_rects.diff.height.saturating_sub(2).max(1) as usize;
        let sticky =
            self.sticky_banner_indexes_for_scroll(self.diff_position.scroll, viewport_rows);
        if sticky.contains(&row) {
            return true;
        }
        let body_rows = viewport_rows
            .saturating_sub(sticky.len().min(viewport_rows.saturating_sub(1)))
            .max(1);
        row >= self.diff_position.scroll
            && row < self.diff_position.scroll.saturating_add(body_rows)
    }

    fn current_diff_line_text(&self) -> Option<&str> {
        self.rendered_diff
            .get(self.diff_position.cursor)
            .map(|line| line.raw_text.as_str())
    }

    fn next_diff_row_with_content(&self, current_row: usize) -> Option<usize> {
        let start = current_row.saturating_add(1);
        (start..self.rendered_diff.len()).find(|idx| !self.rendered_diff[*idx].raw_text.is_empty())
    }

    fn current_diff_line_char_len(&self) -> usize {
        self.current_diff_line_text()
            .map(|line| line.chars().count())
            .unwrap_or(0)
    }

    fn reset_diff_block_cursor(&mut self) {
        self.diff_ui.block_cursor_col = 0;
        self.diff_ui.block_cursor_goal = 0;
    }
}

fn format_path_with_home_tilde(path: &std::path::Path) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.display().to_string();
    };
    let home = std::path::PathBuf::from(home);
    if let Ok(relative) = path.strip_prefix(&home) {
        if relative.as_os_str().is_empty() {
            return "~".to_owned();
        }
        return std::path::Path::new("~")
            .join(relative)
            .display()
            .to_string();
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::format_path_with_home_tilde;

    #[test]
    fn non_home_path_stays_absolute() {
        let path = std::path::Path::new("/opt/tools/review-tasks.md");
        assert_eq!(
            format_path_with_home_tilde(path),
            "/opt/tools/review-tasks.md"
        );
    }
}
