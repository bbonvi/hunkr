use super::services::repository_workflow;
use crate::app::*;

impl App {
    pub(super) fn switch_repository_context(&mut self, target: &Path) -> anyhow::Result<()> {
        repository_workflow::switch_repository_context(self, target)
    }

    pub(super) fn reload_commits(&mut self, preserve_manual_selection: bool) -> anyhow::Result<()> {
        repository_workflow::reload_commits(self, preserve_manual_selection)
    }

    /// Restores persisted UI session state after initial commit loading.
    pub(super) fn restore_persisted_ui_session(&mut self) -> anyhow::Result<()> {
        let session = self.domain.review_state.ui_session.clone();

        if let Some(filter) = session.commit_status_filter {
            self.ui.commit_ui.status_filter = commit_status_filter_from_session(filter);
        }

        restore_commit_selection(&mut self.domain.commits, &session.selected_commit_ids);
        self.ui.commit_ui.visual_anchor = None;
        self.ui.commit_ui.selection_anchor = None;
        self.ui.commit_ui.mouse_anchor = None;
        self.ui.commit_ui.mouse_dragging = false;

        self.runtime.selection_rebuild_due = None;
        self.reset_diff_view_for_commit_selection_change();
        self.ui.diff_cache.selected_file = session.selected_file;
        self.ui.diff_cache.positions = session
            .diff_positions
            .into_iter()
            .map(|(path, position)| (path, diff_position_from_session(position)))
            .collect();
        self.ui.diff_cache.pending_view_anchor = None;
        self.rebuild_selection_dependent_views()?;

        self.sync_commit_cursor_for_filters(
            session.commit_cursor_id.as_deref(),
            self.ui.commit_ui.list_state.selected(),
        );

        if let Some(focused) = session.focused_pane.map(focus_pane_from_session) {
            let has_files = !self.visible_file_indices().is_empty();
            self.ui.preferences.focused = restore_focus_with_availability(focused, has_files);
        }

        Ok(())
    }

    /// Captures the current UI context into persisted review state.
    pub(super) fn snapshot_ui_session_state(&mut self) {
        self.persist_selected_file_position();
        let available_paths = self
            .domain
            .aggregate
            .files
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let diff_positions = self
            .ui
            .diff_cache
            .positions
            .iter()
            .filter(|(path, _)| available_paths.contains(*path))
            .map(|(path, position)| (path.clone(), session_diff_position_from_runtime(*position)))
            .collect::<BTreeMap<_, _>>();

        self.domain.review_state.ui_session = crate::model::UiSessionState {
            selected_commit_ids: self
                .domain
                .commits
                .iter()
                .filter(|row| row.selected)
                .map(|row| row.info.id.clone())
                .collect(),
            commit_cursor_id: self.selected_commit_id(),
            commit_status_filter: Some(commit_status_filter_to_session(
                self.ui.commit_ui.status_filter,
            )),
            focused_pane: Some(focus_pane_to_session(self.ui.preferences.focused)),
            selected_file: self
                .ui
                .diff_cache
                .selected_file
                .clone()
                .filter(|path| available_paths.contains(path)),
            diff_positions,
        };
    }

    /// Persists current UI session state before process exit.
    pub fn persist_session_state_before_exit(&mut self) -> anyhow::Result<()> {
        if !self.deps.store.root_dir().exists() {
            return Ok(());
        }
        self.flush_pending_selection_rebuild();
        self.snapshot_ui_session_state();
        self.deps
            .store
            .save_state_merged(&mut self.domain.review_state)
    }

    pub(super) fn rebuild_selection_dependent_views(&mut self) -> anyhow::Result<()> {
        repository_workflow::rebuild_selection_dependent_views(self)
    }

    pub(super) fn apply_startup_starter_selection(&mut self) -> anyhow::Result<bool> {
        repository_workflow::apply_startup_starter_selection(self)
    }

    pub(super) fn prune_diff_positions_for_removed_files(&mut self) {
        let existing_paths = self
            .domain
            .aggregate
            .files
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        prune_diff_positions_for_missing_paths(&mut self.ui.diff_cache.positions, &existing_paths);

        if let Some(path) = self.ui.diff_cache.selected_file.as_ref()
            && !existing_paths.contains(path)
        {
            self.domain.diff_position = DiffPosition::default();
        }
    }

    pub(super) fn capture_pending_diff_view_anchor(&mut self) {
        self.ui.diff_cache.pending_view_anchor =
            capture_pending_diff_view_anchor(&self.domain.rendered_diff, self.domain.diff_position);
    }

    pub(super) fn apply_pending_diff_view_anchor(&mut self) {
        let Some(pending) = self.ui.diff_cache.pending_view_anchor.take() else {
            return;
        };
        if self.domain.rendered_diff.is_empty() {
            self.domain.diff_position = DiffPosition::default();
            return;
        }

        let cursor_idx =
            find_index_for_line_locator(&self.domain.rendered_diff, &pending.cursor_line);
        let top_idx = find_index_for_line_locator(&self.domain.rendered_diff, &pending.top_line);

        match (cursor_idx, top_idx) {
            (Some(cursor), Some(top)) => {
                self.domain.diff_position.cursor = cursor;
                self.domain.diff_position.scroll = top;
            }
            (Some(cursor), None) => {
                self.domain.diff_position.cursor = cursor;
                self.domain.diff_position.scroll =
                    cursor.saturating_sub(pending.cursor_to_top_offset);
            }
            (None, Some(top)) => {
                self.domain.diff_position.scroll = top;
                self.domain.diff_position.cursor = top.saturating_add(pending.cursor_to_top_offset);
            }
            (None, None) => {}
        }
    }

    pub(super) fn ensure_rendered_diff(&mut self) {
        if self.domain.file_rows.is_empty() {
            self.domain.rendered_diff = Arc::new(Vec::new());
            self.ui.diff_cache.rendered_key = None;
            self.ui.diff_cache.file_ranges.clear();
            self.ui.diff_cache.file_range_by_path.clear();
            self.domain.diff_position = DiffPosition::default();
            self.sync_diff_block_cursor_to_cursor_line();
            return;
        }

        let ordered_paths = self.file_tree_paths_in_order();
        let key = RenderedDiffKey {
            theme_mode: self.ui.preferences.theme.resolved_mode(),
            visible_paths: ordered_paths.clone(),
        };
        if self.ui.diff_cache.rendered_key.as_ref() == Some(&key) {
            return;
        }

        // Preserve local viewport within the selected file before ranges are rebuilt.
        self.persist_selected_file_position();

        if ordered_paths.is_empty() {
            self.domain.rendered_diff = Arc::new(Vec::new());
            self.ui.diff_cache.file_ranges.clear();
            self.ui.diff_cache.file_range_by_path.clear();
            self.ui.diff_cache.rendered_key = Some(key);
            self.domain.diff_position = DiffPosition::default();
            self.sync_diff_block_cursor_to_cursor_line();
            return;
        }

        let theme = self.active_theme().clone();
        let mut estimated_rows = ordered_paths.len().saturating_mul(2);
        for path in &ordered_paths {
            if let Some(patch) = self.domain.aggregate.files.get(path) {
                let hunk_lines = patch
                    .hunks
                    .iter()
                    .map(|hunk| hunk.lines.len())
                    .sum::<usize>();
                estimated_rows = estimated_rows
                    .saturating_add(hunk_lines)
                    .saturating_add(patch.hunks.len().saturating_mul(2));
            } else {
                estimated_rows = estimated_rows.saturating_add(1);
            }
        }

        let mut rendered = Vec::with_capacity(estimated_rows);
        let mut ranges = Vec::new();
        let mut range_by_path = HashMap::new();
        let total_files = ordered_paths.len();

        for (idx, path) in ordered_paths.iter().enumerate() {
            let range_start = rendered.len();
            rendered.push(rendered_file_header_line(
                path,
                idx + 1,
                total_files,
                self.domain.aggregate.file_changes.get(path),
                &theme,
                self.ui.preferences.nerd_fonts,
                &self.ui.preferences.nerd_font_theme,
            ));

            let mut file_rendered = self
                .domain
                .aggregate
                .files
                .get(path)
                .map(|patch| self.build_diff_lines(patch))
                .unwrap_or_default();
            rendered.append(&mut file_rendered);

            if idx + 1 < total_files {
                rendered.push(rendered_separator_line(&theme));
            }

            let range_end = rendered.len();
            ranges.push(FileDiffRange {
                path: path.clone(),
                start: range_start,
                end: range_end,
            });
            range_by_path.insert(path.clone(), (range_start, range_end));
        }

        self.domain.rendered_diff = Arc::new(rendered);
        self.ui.diff_cache.file_ranges = ranges;
        self.ui.diff_cache.file_range_by_path = range_by_path;
        self.ui.diff_cache.rendered_key = Some(key);
        if let Some(path) = self.ui.diff_cache.selected_file.clone()
            && self.ui.diff_cache.file_range_by_path.contains_key(&path)
        {
            self.restore_diff_position(&path);
        } else {
            self.domain.diff_position = DiffPosition::default();
        }
        self.apply_pending_diff_view_anchor();
        self.sync_diff_cursor_to_content_bounds();
    }

    pub(super) fn sync_diff_cursor_to_content_bounds(&mut self) {
        if self.domain.rendered_diff.is_empty() {
            self.domain.diff_position = DiffPosition::default();
            self.sync_diff_block_cursor_to_cursor_line();
            return;
        }

        if self.domain.diff_position.cursor >= self.domain.rendered_diff.len() {
            self.domain.diff_position.cursor = self.domain.rendered_diff.len() - 1;
        }
        if self.domain.diff_position.scroll >= self.domain.rendered_diff.len() {
            self.domain.diff_position.scroll = self.domain.rendered_diff.len() - 1;
        }
        self.sync_diff_visual_bounds();

        if diff_viewport_layout_ready(&self.ui.diff_ui.pane_rects) {
            self.ensure_cursor_visible();
        } else {
            self.sync_diff_block_cursor_to_cursor_line();
        }
    }

    pub(super) fn invalidate_diff_cache(&mut self) {
        self.ui.diff_cache.rendered_key = None;
        self.ui.diff_cache.file_ranges.clear();
        self.ui.diff_cache.file_range_by_path.clear();
        self.ensure_rendered_diff();
    }

    pub(super) fn build_diff_lines(&self, patch: &FilePatch) -> Vec<RenderedDiffLine> {
        let mut rendered = Vec::new();
        let theme = self.active_theme().clone();
        let now_ts = self.now_timestamp();
        let deleted_file =
            should_hide_deleted_file_content(self.domain.aggregate.file_changes.get(&patch.path));
        let deleted_content_expanded = deleted_file
            && self
                .domain
                .deleted_file_content_visible
                .contains(&patch.path);
        if deleted_file && !deleted_content_expanded {
            let mut last_commit_banner: Option<Arc<str>> = None;
            let mut rendered_toggle = false;
            for hunk in &patch.hunks {
                if should_render_commit_banner(
                    last_commit_banner.as_deref(),
                    hunk.commit_id.as_ref(),
                ) {
                    rendered.push(rendered_commit_banner_line(
                        &patch.path,
                        hunk,
                        &theme,
                        now_ts,
                    ));
                    if !rendered_toggle {
                        rendered.push(deleted_file_toggle_line(
                            false,
                            self.ui.preferences.nerd_fonts,
                            &theme,
                        ));
                        rendered_toggle = true;
                    }
                }
                last_commit_banner = Some(hunk.commit_id.clone());
            }
            if !rendered_toggle {
                rendered.push(deleted_file_toggle_line(
                    false,
                    self.ui.preferences.nerd_fonts,
                    &theme,
                ));
            }
            rendered.push(rendered_separator_line(&theme));
            return rendered;
        }
        let mut last_commit_banner: Option<Arc<str>> = None;
        let mut rendered_deleted_toggle = false;
        let shared_file_path: Arc<str> = Arc::from(patch.path.as_str());

        for hunk in &patch.hunks {
            if should_render_commit_banner(last_commit_banner.as_deref(), hunk.commit_id.as_ref()) {
                rendered.push(rendered_commit_banner_line(
                    &patch.path,
                    hunk,
                    &theme,
                    now_ts,
                ));
                if deleted_content_expanded && !rendered_deleted_toggle {
                    rendered.push(deleted_file_toggle_line(
                        true,
                        self.ui.preferences.nerd_fonts,
                        &theme,
                    ));
                    rendered_deleted_toggle = true;
                }
            }
            last_commit_banner = Some(hunk.commit_id.clone());
            let shared_anchor_meta = Arc::new(DiffLineAnchorMeta {
                commit_id: hunk.commit_id.clone(),
                commit_summary: hunk.commit_summary.clone(),
                file_path: shared_file_path.clone(),
                hunk_header: hunk.header.clone(),
            });

            let hunk_header = sanitize_terminal_text(&hunk.header);
            let hunk_label = format!("@@ {hunk_header}");
            rendered.push(RenderedDiffLine {
                #[cfg(test)]
                line: Line::from(""),
                raw_text: Arc::from(hunk_label),
                anchor: Some(DiffLineAnchor {
                    meta: shared_anchor_meta.clone(),
                    old_lineno: Some(hunk.old_start),
                    new_lineno: Some(hunk.new_start),
                }),
            });

            for line in &hunk.lines {
                let anchor = DiffLineAnchor {
                    meta: shared_anchor_meta.clone(),
                    old_lineno: line.old_lineno,
                    new_lineno: line.new_lineno,
                };
                rendered.push(RenderedDiffLine {
                    #[cfg(test)]
                    line: Line::from(""),
                    raw_text: line.text.clone(),
                    anchor: Some(anchor),
                });
            }

            rendered.push(rendered_separator_line(&theme));
        }
        if deleted_content_expanded && !rendered_deleted_toggle {
            rendered.push(deleted_file_toggle_line(
                true,
                self.ui.preferences.nerd_fonts,
                &theme,
            ));
        }

        rendered
    }

    pub(super) fn rebuild_file_tree(&mut self) {
        let mut tree = FileTree::default();
        let mut draft_paths = BTreeSet::new();
        for (path, patch) in &self.domain.aggregate.files {
            if patch
                .hunks
                .iter()
                .any(|hunk| hunk.commit_id.as_ref() == UNCOMMITTED_COMMIT_ID)
            {
                draft_paths.insert(path.clone());
            }
            let modified_ts = patch
                .hunks
                .iter()
                .map(|h| h.commit_timestamp)
                .max()
                .unwrap_or(0);
            tree.insert_with_change(
                path,
                modified_ts,
                self.domain.aggregate.file_changes.get(path).cloned(),
            );
        }

        self.domain.file_rows = tree.flattened_rows(
            self.ui.preferences.nerd_fonts,
            &self.ui.preferences.nerd_font_theme,
        );
        for row in &mut self.domain.file_rows {
            if row.selectable
                && row
                    .path
                    .as_ref()
                    .is_some_and(|path| draft_paths.contains(path))
            {
                row.modified_ts = None;
            }
        }
        if self.domain.file_rows.is_empty() {
            self.ui.file_ui.list_state.select(None);
            self.ui.diff_cache.selected_file = None;
        }
    }

    pub(super) fn ensure_selected_file_exists(&mut self) {
        if self.domain.file_rows.is_empty() {
            self.ui.diff_cache.selected_file = None;
            self.ui.file_ui.list_state.select(None);
            return;
        }

        if let Some(path) = self.ui.diff_cache.selected_file.clone()
            && let Some(idx) = self
                .domain
                .file_rows
                .iter()
                .position(|row| row.selectable && row.path.as_ref() == Some(&path))
        {
            self.ui.diff_cache.selected_file = self.domain.file_rows[idx].path.clone();
            return;
        }

        if let Some(idx) = self.domain.file_rows.iter().position(|row| row.selectable) {
            self.ui.diff_cache.selected_file = self.domain.file_rows[idx].path.clone();
        }
    }

    pub(super) fn visible_commit_indices(&self) -> Vec<usize> {
        self.domain
            .commits
            .iter()
            .enumerate()
            .filter(|(_, row)| self.ui.commit_ui.status_filter.matches_row(row))
            .filter(|(_, row)| commit_row_matches_filter_query(row, &self.ui.search.commit_query))
            .map(|(idx, _)| idx)
            .collect()
    }

    pub(super) fn visible_file_indices(&self) -> Vec<usize> {
        matching_file_indices_with_parent_dirs(&self.domain.file_rows, &self.ui.search.file_query)
    }

    pub(super) fn selected_commit_full_index(&self) -> Option<usize> {
        let visible = self.visible_commit_indices();
        self.ui
            .commit_ui
            .list_state
            .selected()
            .and_then(|idx| visible.get(idx).copied())
    }

    pub(super) fn selected_commit_id(&self) -> Option<String> {
        self.selected_commit_full_index()
            .and_then(|idx| self.domain.commits.get(idx))
            .map(|row| row.info.id.clone())
    }

    pub(super) fn sync_commit_cursor_for_filters(
        &mut self,
        preferred_commit_id: Option<&str>,
        fallback_visible_idx: Option<usize>,
    ) {
        let visible = self.visible_commit_indices();
        if visible.is_empty() {
            self.ui.commit_ui.list_state.select(None);
            self.ui.commit_ui.visual_anchor = None;
            return;
        }

        if let Some(commit_id) = preferred_commit_id
            && let Some(full_idx) = index_of_commit(&self.domain.commits, commit_id)
            && let Some(visible_idx) = visible.iter().position(|entry| *entry == full_idx)
        {
            self.ui.commit_ui.list_state.select(Some(visible_idx));
            return;
        }

        let selected_idx = fallback_visible_idx.unwrap_or(0).min(visible.len() - 1);
        self.ui.commit_ui.list_state.select(Some(selected_idx));

        if self
            .ui
            .commit_ui
            .visual_anchor
            .is_some_and(|anchor| !visible.contains(&anchor))
        {
            self.ui.commit_ui.visual_anchor = None;
        }
        if self
            .ui
            .commit_ui
            .selection_anchor
            .is_some_and(|anchor| !visible.contains(&anchor))
        {
            self.ui.commit_ui.selection_anchor = None;
        }
    }

    pub(super) fn sync_file_cursor_for_filters(&mut self) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            self.ui.file_ui.list_state.select(None);
            return;
        }
        let visible_len = visible.len();
        if let Some(visible_idx) = self
            .ui
            .file_ui
            .list_state
            .selected()
            .filter(|idx| *idx < visible_len)
        {
            // Preserve the user's current row focus (including directories) and
            // only recompute the target diff file for that focused row.
            self.select_file_row(visible_idx);
            return;
        }

        if let Some(path) = self.ui.diff_cache.selected_file.clone()
            && let Some(full_idx) = self
                .domain
                .file_rows
                .iter()
                .position(|row| row.selectable && row.path.as_ref() == Some(&path))
            && let Some(visible_idx) = visible.iter().position(|entry| *entry == full_idx)
        {
            self.ui.file_ui.list_state.select(Some(visible_idx));
            return;
        }

        let Some((visible_idx, full_idx)) = visible
            .iter()
            .enumerate()
            .find(|(_, idx)| self.domain.file_rows[**idx].selectable)
            .map(|(visible_idx, idx)| (visible_idx, *idx))
        else {
            self.ui.file_ui.list_state.select(None);
            return;
        };

        self.ui.file_ui.list_state.select(Some(visible_idx));
        let next_path = self.domain.file_rows[full_idx].path.clone();
        if next_path != self.ui.diff_cache.selected_file {
            self.persist_selected_file_position();
            self.ui.diff_cache.selected_file = next_path.clone();
            if let Some(path) = next_path {
                self.restore_diff_position(&path);
                self.sync_diff_cursor_to_content_bounds();
            }
        }
    }

    pub(super) fn on_selection_changed(&mut self) {
        self.runtime.selection_rebuild_due = None;
        self.reset_diff_view_for_commit_selection_change();
        if let Err(err) = self.rebuild_selection_dependent_views() {
            self.runtime.status = format!("failed to rebuild diff: {err:#}");
        } else {
            let selected = self
                .domain
                .commits
                .iter()
                .filter(|row| row.selected)
                .count();
            self.runtime.status = format!("{} commit(s) selected", selected);
        }
    }

    pub(super) fn on_selection_changed_debounced(&mut self) {
        self.runtime.selection_rebuild_due =
            Some(self.now_instant() + self.tuning.selection_rebuild_debounce);
        self.reset_diff_view_for_commit_selection_change();
        let selected = self
            .domain
            .commits
            .iter()
            .filter(|row| row.selected)
            .count();
        self.runtime.status = format!("{} commit(s) selected", selected);
    }

    pub(super) fn flush_pending_selection_rebuild(&mut self) {
        if self.runtime.selection_rebuild_due.take().is_none() {
            return;
        }
        self.reset_diff_view_for_commit_selection_change();
        if let Err(err) = self.rebuild_selection_dependent_views() {
            self.runtime.status = format!("failed to rebuild diff: {err:#}");
            return;
        }
        let selected = self
            .domain
            .commits
            .iter()
            .filter(|row| row.selected)
            .count();
        self.runtime.status = format!("{} commit(s) selected", selected);
    }

    pub(super) fn reset_diff_view_for_commit_selection_change(&mut self) {
        self.ui.diff_cache.pending_view_anchor = None;
        self.ui.diff_cache.positions.clear();
        self.domain.diff_position = DiffPosition::default();
        self.ui.diff_ui.visual_selection = None;
        self.ui.diff_ui.block_cursor_col = 0;
        self.ui.diff_ui.block_cursor_goal = 0;
        self.ui.diff_ui.mouse_anchor = None;
    }

    pub(super) fn selected_commit_ids_oldest_first(&self) -> Vec<String> {
        selected_ids_oldest_first(&self.domain.commits)
    }

    pub(super) fn file_tree_paths_in_order(&self) -> Vec<String> {
        self.visible_file_indices()
            .into_iter()
            .filter_map(|idx| self.domain.file_rows.get(idx))
            .filter(|row| row.selectable)
            .filter_map(|row| row.path.clone())
            .collect()
    }

    pub(super) fn file_range_for_path(&self, path: &str) -> Option<(usize, usize)> {
        self.ui.diff_cache.file_range_by_path.get(path).copied()
    }

    pub(super) fn file_range_index_for_line(&self, line: usize) -> Option<usize> {
        if self.ui.diff_cache.file_ranges.is_empty() {
            return None;
        }

        let mut left = 0usize;
        let mut right = self.ui.diff_cache.file_ranges.len();
        while left < right {
            let mid = left + ((right - left) / 2);
            let range = &self.ui.diff_cache.file_ranges[mid];
            if line < range.start {
                right = mid;
            } else if line >= range.end {
                left = mid + 1;
            } else {
                return Some(mid);
            }
        }
        None
    }

    pub(super) fn file_path_for_line(&self, line: usize) -> Option<&str> {
        self.file_range_index_for_line(line)
            .and_then(|idx| self.ui.diff_cache.file_ranges.get(idx))
            .map(|range| range.path.as_str())
    }

    pub(super) fn select_file_row_for_path(&mut self, path: &str) {
        if let Some(idx) = self
            .domain
            .file_rows
            .iter()
            .position(|row| row.selectable && row.path.as_deref() == Some(path))
        {
            let visible = self.visible_file_indices();
            let visible_idx = visible.iter().position(|entry| *entry == idx);
            self.ui.file_ui.list_state.select(visible_idx);
        }
    }

    pub(super) fn selected_file_progress(&self) -> Option<(usize, usize)> {
        let path = self.ui.diff_cache.selected_file.as_deref()?;
        let total = self.ui.diff_cache.file_ranges.len();
        let index = self
            .ui
            .diff_cache
            .file_ranges
            .iter()
            .position(|range| range.path == path)?;
        Some((index + 1, total))
    }
}

fn diff_viewport_layout_ready(rects: &PaneRects) -> bool {
    rects.diff.height > 2
}

/// Renders one commit banner row for the current file+commit hunk boundary.
fn rendered_commit_banner_line(
    patch_path: &str,
    hunk: &crate::model::Hunk,
    _theme: &UiTheme,
    now_ts: i64,
) -> RenderedDiffLine {
    let commit_anchor = DiffLineAnchor {
        meta: Arc::new(DiffLineAnchorMeta {
            commit_id: hunk.commit_id.clone(),
            commit_summary: hunk.commit_summary.clone(),
            file_path: Arc::from(patch_path),
            hunk_header: Arc::from(COMMIT_ANCHOR_HEADER),
        }),
        old_lineno: None,
        new_lineno: None,
    };
    let age = format_relative_time(hunk.commit_timestamp, now_ts);
    let commit_summary = sanitize_terminal_text(&hunk.commit_summary);
    let commit_line = if hunk.commit_short.is_empty() {
        format!("---- {commit_summary} ({age})")
    } else {
        format!(
            "---- commit {} {} ({})",
            hunk.commit_short, commit_summary, age
        )
    };
    RenderedDiffLine {
        #[cfg(test)]
        line: Line::from(""),
        raw_text: Arc::from(commit_line),
        anchor: Some(commit_anchor),
    }
}

fn deleted_file_toggle_line(
    _expanded: bool,
    _nerd_fonts: bool,
    _theme: &UiTheme,
) -> RenderedDiffLine {
    RenderedDiffLine {
        #[cfg(test)]
        line: Line::from(""),
        raw_text: Arc::from(DELETED_FILE_TOGGLE_RAW_TEXT),
        anchor: None,
    }
}

fn focus_pane_to_session(focused: FocusPane) -> crate::model::UiSessionFocusPane {
    match focused {
        FocusPane::Commits => crate::model::UiSessionFocusPane::Commits,
        FocusPane::Files => crate::model::UiSessionFocusPane::Files,
        FocusPane::Diff => crate::model::UiSessionFocusPane::Diff,
    }
}

fn focus_pane_from_session(focused: crate::model::UiSessionFocusPane) -> FocusPane {
    match focused {
        crate::model::UiSessionFocusPane::Commits => FocusPane::Commits,
        crate::model::UiSessionFocusPane::Files => FocusPane::Files,
        crate::model::UiSessionFocusPane::Diff => FocusPane::Diff,
    }
}

fn commit_status_filter_to_session(
    filter: CommitStatusFilter,
) -> crate::model::UiSessionCommitStatusFilter {
    match filter {
        CommitStatusFilter::All => crate::model::UiSessionCommitStatusFilter::All,
        CommitStatusFilter::UnreviewedOrIssueFound => {
            crate::model::UiSessionCommitStatusFilter::UnreviewedOrIssueFound
        }
        CommitStatusFilter::Reviewed => crate::model::UiSessionCommitStatusFilter::Reviewed,
    }
}

fn commit_status_filter_from_session(
    filter: crate::model::UiSessionCommitStatusFilter,
) -> CommitStatusFilter {
    match filter {
        crate::model::UiSessionCommitStatusFilter::All => CommitStatusFilter::All,
        crate::model::UiSessionCommitStatusFilter::UnreviewedOrIssueFound => {
            CommitStatusFilter::UnreviewedOrIssueFound
        }
        crate::model::UiSessionCommitStatusFilter::Reviewed => CommitStatusFilter::Reviewed,
    }
}

fn session_diff_position_from_runtime(
    position: DiffPosition,
) -> crate::model::UiSessionDiffPosition {
    crate::model::UiSessionDiffPosition {
        scroll: position.scroll,
        cursor: position.cursor,
    }
}

fn diff_position_from_session(position: crate::model::UiSessionDiffPosition) -> DiffPosition {
    DiffPosition {
        scroll: position.scroll,
        cursor: position.cursor,
    }
}

fn restore_commit_selection(rows: &mut [CommitRow], selected_commit_ids: &BTreeSet<String>) {
    for row in rows.iter_mut() {
        row.selected = selected_commit_ids.contains(&row.info.id);
    }
    let Some(start) = rows.iter().position(|row| row.selected) else {
        return;
    };
    let end = rows.iter().rposition(|row| row.selected).unwrap_or(start);
    apply_range_selection(rows, start, end);
}

fn restore_focus_with_availability(focused: FocusPane, has_files: bool) -> FocusPane {
    if focused == FocusPane::Files && !has_files {
        FocusPane::Commits
    } else {
        focused
    }
}

pub(super) fn format_uncommitted_summary(file_count: usize) -> String {
    let noun = if file_count == 1 { "file" } else { "files" };
    format!("{UNCOMMITTED_COMMIT_SUMMARY} ({file_count} {noun})")
}

/// Returns whether deleted-file payload should be replaced with a concise removal marker.
pub(super) fn should_hide_deleted_file_content(file_change: Option<&FileChangeSummary>) -> bool {
    file_change.is_some_and(|change| change.kind == FileChangeKind::Deleted)
}

pub(super) fn rendered_file_header_line(
    path: &str,
    file_index: usize,
    total_files: usize,
    file_change: Option<&FileChangeSummary>,
    _theme: &UiTheme,
    nerd_fonts: bool,
    _nerd_font_theme: &NerdFontTheme,
) -> RenderedDiffLine {
    let sanitized_path = sanitize_terminal_text(path);
    let raw_change_suffix = file_change
        .map(|change| format!(" · {}", format_file_change_badge(change, nerd_fonts)))
        .unwrap_or_default();
    let rename_from = file_change
        .and_then(|change| change.old_path.as_ref())
        .map(|from| format!(" (from {})", sanitize_terminal_text(from)))
        .unwrap_or_default();
    let raw_text = format!(
        "==== file {file_index}/{total_files}: {sanitized_path}{rename_from}{raw_change_suffix} ===="
    );
    RenderedDiffLine {
        #[cfg(test)]
        line: Line::from(""),
        raw_text: Arc::from(raw_text),
        anchor: None,
    }
}

pub(super) fn rendered_separator_line(_theme: &UiTheme) -> RenderedDiffLine {
    RenderedDiffLine {
        #[cfg(test)]
        line: Line::from(""),
        raw_text: Arc::from(""),
        anchor: None,
    }
}

#[cfg(test)]
mod restore_tests {
    use super::{CommitRow, FocusPane, restore_commit_selection, restore_focus_with_availability};
    use crate::model::{
        CommitInfo, ReviewStatus, UNCOMMITTED_COMMIT_ID, UNCOMMITTED_COMMIT_SHORT,
        UNCOMMITTED_COMMIT_SUMMARY,
    };
    use std::collections::BTreeSet;

    fn row(id: &str) -> CommitRow {
        CommitRow {
            info: CommitInfo {
                id: id.to_owned(),
                short_id: id.to_owned(),
                summary: String::new(),
                author: String::new(),
                timestamp: 0,
                unpushed: false,
                decorations: Vec::new(),
            },
            selected: false,
            status: ReviewStatus::Unreviewed,
            is_uncommitted: false,
        }
    }

    #[test]
    fn restore_commit_selection_keeps_only_available_ids_and_coalesces_to_range() {
        let mut rows = vec![row("a1"), row("b2"), row("c3")];
        rows.insert(
            0,
            CommitRow {
                info: CommitInfo {
                    id: UNCOMMITTED_COMMIT_ID.to_owned(),
                    short_id: UNCOMMITTED_COMMIT_SHORT.to_owned(),
                    summary: UNCOMMITTED_COMMIT_SUMMARY.to_owned(),
                    author: "local".to_owned(),
                    timestamp: 0,
                    unpushed: false,
                    decorations: Vec::new(),
                },
                selected: false,
                status: ReviewStatus::Unreviewed,
                is_uncommitted: true,
            },
        );
        let selected = BTreeSet::from([
            "b2".to_owned(),
            "missing".to_owned(),
            UNCOMMITTED_COMMIT_ID.to_owned(),
        ]);

        restore_commit_selection(&mut rows, &selected);

        assert!(rows[0].selected);
        assert!(rows[1].selected);
        assert!(rows[2].selected);
        assert!(!rows[3].selected);
    }

    #[test]
    fn restore_focus_falls_back_from_files_when_no_files_are_available() {
        assert_eq!(
            restore_focus_with_availability(FocusPane::Files, false),
            FocusPane::Commits
        );
        assert_eq!(
            restore_focus_with_availability(FocusPane::Files, true),
            FocusPane::Files
        );
        assert_eq!(
            restore_focus_with_availability(FocusPane::Diff, false),
            FocusPane::Diff
        );
    }

    #[test]
    fn diff_viewport_layout_ready_requires_inner_rows() {
        let mut rects = crate::app::PaneRects::default();
        rects.diff.height = 2;
        assert!(!super::diff_viewport_layout_ready(&rects));

        rects.diff.height = 3;
        assert!(super::diff_viewport_layout_ready(&rects));
    }
}
