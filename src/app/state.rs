use super::*;

impl App {
    pub(super) fn reload_commits(&mut self, preserve_manual_selection: bool) -> anyhow::Result<()> {
        let history = self.git.load_first_parent_history(HISTORY_LIMIT)?;
        let default_selected = self.git.default_unpushed_commit_ids()?;
        let prior_cursor_idx = self.commit_list_state.selected();
        let prior_cursor_commit_id = prior_cursor_idx
            .and_then(|idx| self.commits.get(idx))
            .map(|row| row.info.id.clone());
        let prior_visual_anchor_commit_id = self
            .commit_visual_anchor
            .and_then(|idx| self.commits.get(idx))
            .map(|row| row.info.id.clone());

        let mut old_selected = BTreeSet::new();
        if preserve_manual_selection {
            for row in &self.commits {
                if row.selected {
                    old_selected.insert(row.info.id.clone());
                }
            }
        }

        let had_existing_rows = !self.commits.is_empty();
        let mut known = BTreeSet::new();
        for row in &self.commits {
            known.insert(row.info.id.clone());
        }

        self.commits = history
            .into_iter()
            .map(|info| {
                let status = self.store.commit_status(&self.review_state, &info.id);
                let selected = if preserve_manual_selection && old_selected.contains(&info.id) {
                    true
                } else if !had_existing_rows {
                    default_selected.contains(&info.id) && status == ReviewStatus::Unreviewed
                } else {
                    false
                };
                CommitRow {
                    info,
                    selected,
                    status,
                    is_uncommitted: false,
                }
            })
            .collect();

        let uncommitted_selected =
            preserve_manual_selection && old_selected.contains(UNCOMMITTED_COMMIT_ID);
        self.commits.insert(
            0,
            CommitRow {
                info: CommitInfo {
                    short_id: UNCOMMITTED_COMMIT_SHORT.to_owned(),
                    id: UNCOMMITTED_COMMIT_ID.to_owned(),
                    summary: UNCOMMITTED_COMMIT_SUMMARY.to_owned(),
                    author: "local".to_owned(),
                    timestamp: Utc::now().timestamp(),
                    unpushed: false,
                },
                selected: uncommitted_selected,
                status: ReviewStatus::Unreviewed,
                is_uncommitted: true,
            },
        );

        let restored_cursor = restore_list_index_by_commit_id(
            &self.commits,
            prior_cursor_commit_id.as_deref(),
            prior_cursor_idx,
        );
        self.commit_list_state.select(restored_cursor);
        self.commit_visual_anchor = prior_visual_anchor_commit_id
            .as_deref()
            .and_then(|commit_id| index_of_commit(&self.commits, commit_id));

        let new_commits = self
            .commits
            .iter()
            .filter(|row| {
                !row.is_uncommitted
                    && !known.contains(&row.info.id)
                    && row.status == ReviewStatus::Unreviewed
            })
            .count();
        if new_commits > 0 {
            self.status = format!("{} new unreviewed commit(s) detected", new_commits);
        }

        self.rebuild_selection_dependent_views()?;
        self.sync_comment_report()?;
        Ok(())
    }

    pub(super) fn rebuild_selection_dependent_views(&mut self) -> anyhow::Result<()> {
        let selected_ordered = self.selected_commit_ids_oldest_first();
        let mut aggregate = if selected_ordered.is_empty() {
            AggregatedDiff::default()
        } else {
            self.git.aggregate_for_commits(&selected_ordered)?
        };
        if self.uncommitted_selected() {
            merge_aggregate_diff(&mut aggregate, self.git.aggregate_uncommitted()?);
        }
        let changed_paths = changed_paths_between_aggregates(&self.aggregate, &aggregate);
        let aggregate_changed = !changed_paths.is_empty();

        if aggregate_changed {
            self.capture_pending_diff_view_anchor();
        }

        self.aggregate = aggregate;
        self.prune_diff_positions_for_removed_files();

        if aggregate_changed {
            self.rendered_diff_cache
                .retain(|(path, _), _| !changed_paths.contains(path));
            self.rendered_diff_key = None;
            self.file_diff_ranges.clear();
            self.file_diff_range_by_path.clear();
            self.diff_pending_op = None;
        }

        self.rebuild_file_tree();
        self.ensure_selected_file_exists();
        self.ensure_rendered_diff();
        Ok(())
    }

    pub(super) fn prune_diff_positions_for_removed_files(&mut self) {
        let existing_paths = self
            .aggregate
            .files
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        prune_diff_positions_for_missing_paths(&mut self.diff_positions, &existing_paths);

        if let Some(path) = self.selected_file.as_ref()
            && !existing_paths.contains(path)
        {
            self.diff_position = DiffPosition::default();
        }
    }

    pub(super) fn capture_pending_diff_view_anchor(&mut self) {
        self.pending_diff_view_anchor =
            capture_pending_diff_view_anchor(&self.rendered_diff, self.diff_position);
    }

    pub(super) fn apply_pending_diff_view_anchor(&mut self) {
        let Some(pending) = self.pending_diff_view_anchor.take() else {
            return;
        };
        if self.rendered_diff.is_empty() {
            self.diff_position = DiffPosition::default();
            return;
        }

        let cursor_idx = find_index_for_line_locator(&self.rendered_diff, &pending.cursor_line);
        let top_idx = find_index_for_line_locator(&self.rendered_diff, &pending.top_line);

        match (cursor_idx, top_idx) {
            (Some(cursor), Some(top)) => {
                self.diff_position.cursor = cursor;
                self.diff_position.scroll = top;
            }
            (Some(cursor), None) => {
                self.diff_position.cursor = cursor;
                self.diff_position.scroll = cursor.saturating_sub(pending.cursor_to_top_offset);
            }
            (None, Some(top)) => {
                self.diff_position.scroll = top;
                self.diff_position.cursor = top.saturating_add(pending.cursor_to_top_offset);
            }
            (None, None) => {}
        }
    }

    pub(super) fn ensure_rendered_diff(&mut self) {
        if self.file_rows.is_empty() {
            self.rendered_diff = Arc::new(Vec::new());
            self.rendered_diff_key = None;
            self.file_diff_ranges.clear();
            self.file_diff_range_by_path.clear();
            self.diff_position = DiffPosition::default();
            return;
        }

        let key = self.theme_mode;
        if self.rendered_diff_key.as_ref() == Some(&key) {
            return;
        }

        let theme = UiTheme::from_mode(self.theme_mode);
        let ordered_paths = self.file_tree_paths_in_order();
        let mut rendered = Vec::new();
        let mut ranges = Vec::new();
        let mut range_by_path = HashMap::new();
        let total_files = ordered_paths.len();

        for (idx, path) in ordered_paths.iter().enumerate() {
            let range_start = rendered.len();
            rendered.push(rendered_file_header_line(
                path,
                idx + 1,
                total_files,
                &theme,
                self.nerd_fonts,
                &self.nerd_font_theme,
            ));

            let file_key = (path.clone(), self.theme_mode);
            let file_rendered = if let Some(cached) = self.rendered_diff_cache.get(&file_key) {
                cached.clone()
            } else {
                let built = self
                    .aggregate
                    .files
                    .get(path)
                    .map(|patch| Arc::new(self.build_diff_lines(patch)))
                    .unwrap_or_else(|| Arc::new(Vec::new()));
                self.rendered_diff_cache
                    .insert(file_key.clone(), built.clone());
                built
            };

            rendered.extend(file_rendered.iter().cloned());

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

        self.rendered_diff = Arc::new(rendered);
        self.file_diff_ranges = ranges;
        self.file_diff_range_by_path = range_by_path;
        self.rendered_diff_key = Some(key);
        self.apply_pending_diff_view_anchor();
        self.sync_diff_cursor_to_content_bounds();
    }

    pub(super) fn sync_diff_cursor_to_content_bounds(&mut self) {
        if self.rendered_diff.is_empty() {
            self.diff_position = DiffPosition::default();
            return;
        }

        if self.diff_position.cursor >= self.rendered_diff.len() {
            self.diff_position.cursor = self.rendered_diff.len() - 1;
        }
        self.sync_diff_visual_bounds();

        self.ensure_cursor_visible();
    }

    pub(super) fn invalidate_diff_cache(&mut self) {
        self.rendered_diff_cache.clear();
        self.rendered_diff_key = None;
        self.file_diff_ranges.clear();
        self.file_diff_range_by_path.clear();
        self.ensure_rendered_diff();
    }

    pub(super) fn current_comment_id(&self) -> Option<u64> {
        self.rendered_diff
            .get(self.diff_position.cursor)
            .and_then(|line| line.comment_id)
    }

    pub(super) fn build_diff_lines(&self, patch: &FilePatch) -> Vec<RenderedDiffLine> {
        let mut rendered = Vec::new();
        let theme = UiTheme::from_mode(self.theme_mode);
        let now_ts = Utc::now().timestamp();
        let file_comments: Vec<&ReviewComment> = self
            .comments
            .comments()
            .iter()
            .filter(|comment| comment.target.end.file_path == patch.path)
            .collect();
        let mut last_commit_banner: Option<String> = None;
        let mut inserted_commit_comments = BTreeSet::new();

        for hunk in &patch.hunks {
            let commit_anchor = CommentAnchor {
                commit_id: hunk.commit_id.clone(),
                commit_summary: hunk.commit_summary.clone(),
                file_path: patch.path.clone(),
                hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
                old_lineno: None,
                new_lineno: None,
            };
            if should_render_commit_banner(last_commit_banner.as_deref(), &hunk.commit_id) {
                let age = format_relative_time(hunk.commit_timestamp, now_ts);
                let (commit_line, line) = if hunk.commit_short.is_empty() {
                    (
                        format!("---- {} ({})", hunk.commit_summary, age),
                        Line::from(vec![
                            Span::styled("---- ", Style::default().fg(theme.dimmed)),
                            Span::styled(
                                hunk.commit_summary.clone(),
                                Style::default().fg(theme.text),
                            ),
                            Span::raw(" "),
                            Span::styled(format!("({})", age), Style::default().fg(theme.dimmed)),
                        ]),
                    )
                } else {
                    (
                        format!(
                            "---- commit {} {} ({})",
                            hunk.commit_short, hunk.commit_summary, age
                        ),
                        Line::from(vec![
                            Span::styled("---- ", Style::default().fg(theme.dimmed)),
                            Span::styled("commit ", Style::default().fg(theme.muted)),
                            Span::styled(
                                hunk.commit_short.clone(),
                                Style::default()
                                    .fg(theme.focus_border)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(" "),
                            Span::styled(
                                hunk.commit_summary.clone(),
                                Style::default().fg(theme.text),
                            ),
                            Span::raw(" "),
                            Span::styled(format!("({})", age), Style::default().fg(theme.dimmed)),
                        ]),
                    )
                };
                rendered.push(RenderedDiffLine {
                    line,
                    raw_text: commit_line,
                    anchor: Some(commit_anchor.clone()),
                    comment_id: None,
                });

                let mut commit_comments: Vec<&ReviewComment> = file_comments
                    .iter()
                    .copied()
                    .filter(|comment| {
                        comment_targets_commit_end(comment, &patch.path, &hunk.commit_id)
                    })
                    .collect();
                commit_comments.sort_by_key(|comment| comment.id);
                for comment in commit_comments {
                    if inserted_commit_comments.insert(comment.id) {
                        push_comment_lines(&mut rendered, comment, &theme, now_ts);
                    }
                }
            }
            last_commit_banner = Some(hunk.commit_id.clone());

            let mut hunk_comments: Vec<&ReviewComment> = file_comments
                .iter()
                .copied()
                .filter(|comment| {
                    comment_targets_hunk_end(comment, &patch.path, &hunk.commit_id, &hunk.header)
                })
                .collect();
            hunk_comments.sort_by_key(|comment| comment.id);
            let mut injected_comment_ids = BTreeSet::new();

            let hunk_anchor = CommentAnchor {
                commit_id: hunk.commit_id.clone(),
                commit_summary: hunk.commit_summary.clone(),
                file_path: patch.path.clone(),
                hunk_header: hunk.header.clone(),
                old_lineno: Some(hunk.old_start),
                new_lineno: Some(hunk.new_start),
            };
            let hunk_label = format!("@@ {}", hunk.header);
            rendered.push(RenderedDiffLine {
                line: Line::from(vec![
                    Span::styled("@@ ", Style::default().fg(theme.muted)),
                    Span::styled(hunk.header.clone(), Style::default().fg(theme.diff_header)),
                ]),
                raw_text: hunk_label,
                anchor: Some(CommentAnchor {
                    commit_id: hunk.commit_id.clone(),
                    commit_summary: hunk.commit_summary.clone(),
                    file_path: patch.path.clone(),
                    hunk_header: hunk.header.clone(),
                    old_lineno: Some(hunk.old_start),
                    new_lineno: Some(hunk.new_start),
                }),
                comment_id: None,
            });
            push_comment_lines_for_anchor(
                &mut rendered,
                &hunk_comments,
                &mut injected_comment_ids,
                &hunk_anchor,
                &theme,
                now_ts,
            );

            for line in &hunk.lines {
                let anchor = CommentAnchor {
                    commit_id: hunk.commit_id.clone(),
                    commit_summary: hunk.commit_summary.clone(),
                    file_path: patch.path.clone(),
                    hunk_header: hunk.header.clone(),
                    old_lineno: line.old_lineno,
                    new_lineno: line.new_lineno,
                };
                rendered.push(RenderedDiffLine {
                    line: self.render_code_line(&patch.path, line, &theme),
                    raw_text: raw_diff_text(line),
                    anchor: Some(anchor.clone()),
                    comment_id: None,
                });
                push_comment_lines_for_anchor(
                    &mut rendered,
                    &hunk_comments,
                    &mut injected_comment_ids,
                    &anchor,
                    &theme,
                    now_ts,
                );
            }

            for comment in hunk_comments {
                if injected_comment_ids.insert(comment.id) {
                    push_comment_lines(&mut rendered, comment, &theme, now_ts);
                }
            }

            rendered.push(rendered_separator_line(&theme));
        }

        rendered
    }

    pub(super) fn render_code_line(
        &self,
        path: &str,
        line: &HunkLine,
        theme: &UiTheme,
    ) -> Line<'static> {
        let (prefix, accent, bg) = match line.kind {
            DiffLineKind::Add => ('+', theme.diff_add, Some(theme.diff_add_bg)),
            DiffLineKind::Remove => ('-', theme.diff_remove, Some(theme.diff_remove_bg)),
            DiffLineKind::Context => (' ', theme.dimmed, None),
            DiffLineKind::Meta => ('~', theme.diff_meta, None),
        };

        let old_col = line
            .old_lineno
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_owned());
        let new_col = line
            .new_lineno
            .map(|n| format!("{:>4}", n))
            .unwrap_or_else(|| "    ".to_owned());

        let mut spans = vec![
            Span::styled(
                format!("{} {} ", old_col, new_col),
                Style::default().fg(theme.dimmed),
            ),
            Span::styled(
                prefix.to_string(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ];

        let highlighted = self
            .highlighter
            .highlight(self.theme_mode, path, &line.text);
        for mut span in highlighted {
            if let Some(bg_color) = bg {
                span.style = span.style.bg(bg_color);
            }
            spans.push(span);
        }

        Line::from(spans)
    }

    pub(super) fn rebuild_file_tree(&mut self) {
        let mut tree = FileTree::default();
        let mut draft_paths = BTreeSet::new();
        for (path, patch) in &self.aggregate.files {
            if patch
                .hunks
                .iter()
                .any(|hunk| hunk.commit_id == UNCOMMITTED_COMMIT_ID)
            {
                draft_paths.insert(path.clone());
            }
            let modified_ts = patch
                .hunks
                .iter()
                .map(|h| h.commit_timestamp)
                .max()
                .unwrap_or(0);
            tree.insert(path, modified_ts);
        }

        self.file_rows = tree.flattened_rows(self.nerd_fonts, &self.nerd_font_theme);
        for row in &mut self.file_rows {
            if row.selectable
                && row
                    .path
                    .as_ref()
                    .is_some_and(|path| draft_paths.contains(path))
            {
                row.modified_ts = None;
            }
        }
        if self.file_rows.is_empty() {
            self.file_list_state.select(None);
            self.selected_file = None;
            return;
        }

        if self.file_list_state.selected().is_none()
            && let Some(idx) = self.file_rows.iter().position(|row| row.selectable)
        {
            self.file_list_state.select(Some(idx));
            self.selected_file = self.file_rows[idx].path.clone();
        }
    }

    pub(super) fn ensure_selected_file_exists(&mut self) {
        if self.file_rows.is_empty() {
            self.selected_file = None;
            self.file_list_state.select(None);
            return;
        }

        if let Some(path) = self.selected_file.clone()
            && let Some(idx) = self
                .file_rows
                .iter()
                .position(|row| row.selectable && row.path.as_ref() == Some(&path))
        {
            self.file_list_state.select(Some(idx));
            return;
        }

        if let Some(idx) = self.file_rows.iter().position(|row| row.selectable) {
            self.file_list_state.select(Some(idx));
            self.selected_file = self.file_rows[idx].path.clone();
        }
    }

    pub(super) fn on_selection_changed(&mut self) {
        if let Err(err) = self.rebuild_selection_dependent_views() {
            self.status = format!("failed to rebuild diff: {err:#}");
        } else {
            let selected = self.commits.iter().filter(|row| row.selected).count();
            self.status = format!("{} commit(s) selected", selected);
        }
    }

    pub(super) fn selected_commit_ids_oldest_first(&self) -> Vec<String> {
        selected_ids_oldest_first(&self.commits)
    }

    pub(super) fn file_tree_paths_in_order(&self) -> Vec<String> {
        self.file_rows
            .iter()
            .filter(|row| row.selectable)
            .filter_map(|row| row.path.clone())
            .collect()
    }

    pub(super) fn file_range_for_path(&self, path: &str) -> Option<(usize, usize)> {
        self.file_diff_range_by_path.get(path).copied()
    }

    pub(super) fn file_range_index_for_line(&self, line: usize) -> Option<usize> {
        if self.file_diff_ranges.is_empty() {
            return None;
        }

        let mut left = 0usize;
        let mut right = self.file_diff_ranges.len();
        while left < right {
            let mid = left + ((right - left) / 2);
            let range = &self.file_diff_ranges[mid];
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
            .and_then(|idx| self.file_diff_ranges.get(idx))
            .map(|range| range.path.as_str())
    }

    pub(super) fn select_file_row_for_path(&mut self, path: &str) {
        if let Some(idx) = self
            .file_rows
            .iter()
            .position(|row| row.selectable && row.path.as_deref() == Some(path))
        {
            self.file_list_state.select(Some(idx));
        }
    }

    pub(super) fn selected_file_progress(&self) -> Option<(usize, usize)> {
        let path = self.selected_file.as_deref()?;
        let total = self.file_diff_ranges.len();
        let index = self
            .file_diff_ranges
            .iter()
            .position(|range| range.path == path)?;
        Some((index + 1, total))
    }
}

pub(super) fn rendered_file_header_line(
    path: &str,
    file_index: usize,
    total_files: usize,
    theme: &UiTheme,
    nerd_fonts: bool,
    nerd_font_theme: &NerdFontTheme,
) -> RenderedDiffLine {
    let raw_text = format!("==== file {file_index}/{total_files}: {path} ====");
    let display_path = format_path_with_icon(path, nerd_fonts, nerd_font_theme);
    RenderedDiffLine {
        line: Line::from(vec![
            Span::styled("==== ", Style::default().fg(theme.dimmed)),
            Span::styled(
                format!("file {file_index}/{total_files}"),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(": ", Style::default().fg(theme.dimmed)),
            Span::styled(display_path, Style::default().fg(theme.text)),
            Span::styled(" ====", Style::default().fg(theme.dimmed)),
        ]),
        raw_text,
        anchor: None,
        comment_id: None,
    }
}

pub(super) fn rendered_separator_line(theme: &UiTheme) -> RenderedDiffLine {
    RenderedDiffLine {
        line: Line::from(Span::styled("            ", Style::default().fg(theme.dimmed))),
        raw_text: String::new(),
        anchor: None,
        comment_id: None,
    }
}
