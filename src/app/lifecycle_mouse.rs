//! Mouse interaction handlers for list panes, diff, and comment editor modal.
use super::*;

impl App {
    pub(super) fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        if self.onboarding_active() {
            return;
        }

        if matches!(
            self.preferences.input_mode,
            InputMode::CommentCreate | InputMode::CommentEdit(_)
        ) {
            self.handle_comment_mouse(mouse);
            return;
        }
        if matches!(self.preferences.input_mode, InputMode::ShellCommand) {
            self.handle_shell_command_mouse(mouse);
            return;
        }
        if matches!(self.preferences.input_mode, InputMode::WorktreeSwitch) {
            return;
        }
        let x = mouse.column;
        let y = mouse.row;

        let in_files = contains(self.diff_ui.pane_rects.files, x, y);
        let in_commits = contains(self.diff_ui.pane_rects.commits, x, y);
        let in_diff = contains(self.diff_ui.pane_rects.diff, x, y);
        let resolve_diff_row = |app: &Self, mouse_y: u16| -> Option<usize> {
            let viewport_rows =
                app.diff_ui.pane_rects.diff.height.saturating_sub(2).max(1) as usize;
            let sticky_banner_indexes =
                app.sticky_banner_indexes_for_scroll(app.diff_position.scroll, viewport_rows);
            diff_index_at(
                mouse_y,
                app.diff_ui.pane_rects.diff,
                app.diff_position.scroll,
                &sticky_banner_indexes,
            )
        };
        let resolve_commit_visible_idx = |app: &Self, mouse_y: u16| -> Option<usize> {
            list_index_at(
                mouse_y,
                app.diff_ui.pane_rects.commits,
                app.commit_ui.list_state.offset(),
            )
        };

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if in_diff {
                    self.clear_keyboard_diff_visual_selection();
                    self.scroll_diff_viewport(self.preferences.diff_wheel_scroll_lines);
                } else if in_files && self.should_scroll_list_wheel(FocusPane::Files, 1) {
                    self.scroll_file_list_lines(1);
                } else if in_commits && self.should_scroll_list_wheel(FocusPane::Commits, 1) {
                    self.commit_ui.visual_anchor = None;
                    self.scroll_commit_list_lines(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if in_diff {
                    self.clear_keyboard_diff_visual_selection();
                    self.scroll_diff_viewport(-self.preferences.diff_wheel_scroll_lines);
                } else if in_files && self.should_scroll_list_wheel(FocusPane::Files, -1) {
                    self.scroll_file_list_lines(-1);
                } else if in_commits && self.should_scroll_list_wheel(FocusPane::Commits, -1) {
                    self.commit_ui.visual_anchor = None;
                    self.scroll_commit_list_lines(-1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.commit_ui.mouse_anchor = None;
                self.commit_ui.mouse_dragging = false;
                self.commit_ui.mouse_drag_mode = None;
                self.commit_ui.mouse_drag_baseline = None;
                if in_files {
                    self.set_focus(FocusPane::Files);
                    self.diff_ui.mouse_anchor = None;
                    if let Some(idx) = list_index_at(
                        y,
                        self.diff_ui.pane_rects.files,
                        self.file_ui.list_state.offset(),
                    ) {
                        self.select_file_row(idx);
                    }
                } else if in_commits {
                    self.set_focus(FocusPane::Commits);
                    self.diff_ui.mouse_anchor = None;
                    self.commit_ui.visual_anchor = None;
                    if let Some(idx) = resolve_commit_visible_idx(self, y) {
                        let drag_mode = commit_mouse_selection_mode(mouse.modifiers);
                        let baseline = self.commits.iter().map(|row| row.selected).collect();
                        let clicked_full_idx =
                            self.select_commit_row_with_mouse(idx, mouse.modifiers);
                        if matches!(
                            drag_mode,
                            CommitMouseSelectionMode::Replace | CommitMouseSelectionMode::Toggle
                        ) {
                            self.commit_ui.mouse_anchor = clicked_full_idx;
                            self.commit_ui.mouse_drag_mode = Some(drag_mode);
                            if drag_mode == CommitMouseSelectionMode::Toggle {
                                self.commit_ui.mouse_drag_baseline = Some(baseline);
                            }
                        } else {
                            self.commit_ui.mouse_anchor = None;
                        }
                    }
                } else if in_diff {
                    self.set_focus(FocusPane::Diff);
                    self.diff_ui.visual_selection = None;
                    if let Some(row) = resolve_diff_row(self, y) {
                        self.set_diff_cursor(row);
                        self.diff_ui.mouse_anchor = Some(self.diff_position.cursor);
                    } else {
                        self.diff_ui.mouse_anchor = None;
                    }
                } else {
                    self.diff_ui.mouse_anchor = None;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.commit_ui.mouse_anchor.is_some() => {
                let edge_delta = list_drag_scroll_delta(
                    y,
                    self.diff_ui.pane_rects.commits,
                    LIST_DRAG_EDGE_MARGIN,
                );
                if edge_delta != 0 {
                    self.scroll_commit_list_lines(edge_delta);
                }

                let target_visible_idx =
                    resolve_commit_visible_idx(self, y).or(self.commit_ui.list_state.selected());
                if let Some(visible_idx) = target_visible_idx {
                    let visible_indices = self.visible_commit_indices();
                    if let Some(full_idx) = visible_indices.get(visible_idx).copied() {
                        self.commit_ui.list_state.select(Some(visible_idx));
                        let anchor = self.commit_ui.mouse_anchor.expect("checked above");
                        match self.commit_ui.mouse_drag_mode {
                            Some(CommitMouseSelectionMode::Toggle) => {
                                if let Some(baseline) =
                                    self.commit_ui.mouse_drag_baseline.as_deref()
                                {
                                    apply_toggle_range_from_baseline(
                                        &mut self.commits,
                                        baseline,
                                        anchor,
                                        full_idx,
                                    );
                                } else {
                                    apply_range_selection(&mut self.commits, anchor, full_idx);
                                }
                            }
                            _ => apply_range_selection(&mut self.commits, anchor, full_idx),
                        }
                        if anchor != full_idx {
                            self.commit_ui.mouse_dragging = true;
                        }
                        if self.commit_ui.mouse_dragging {
                            self.on_selection_changed_debounced();
                        }
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if in_diff => {
                if let Some(row) = resolve_diff_row(self, y) {
                    self.set_diff_cursor(row);
                    self.diff_ui.visual_selection = diff_visual_from_drag_anchor(
                        self.diff_ui.mouse_anchor,
                        self.diff_position.cursor,
                    );
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let commit_dragging = self.commit_ui.mouse_dragging;
                self.commit_ui.mouse_anchor = None;
                self.commit_ui.mouse_dragging = false;
                self.commit_ui.mouse_drag_mode = None;
                self.commit_ui.mouse_drag_baseline = None;

                if in_diff {
                    if let Some(row) = resolve_diff_row(self, y) {
                        self.set_diff_cursor(row);
                    }
                    self.diff_ui.visual_selection = diff_visual_from_drag_anchor(
                        self.diff_ui.mouse_anchor,
                        self.diff_position.cursor,
                    );
                }
                self.diff_ui.mouse_anchor = None;

                if commit_dragging {
                    self.on_selection_changed();
                }
            }
            _ => {}
        }
    }

    fn should_scroll_list_wheel(&mut self, pane: FocusPane, delta: isize) -> bool {
        let now = Instant::now();
        if list_wheel_event_is_duplicate(
            self.diff_ui.last_list_wheel_event,
            pane,
            delta,
            now,
            self.preferences.list_wheel_coalesce,
        ) {
            return false;
        }
        self.diff_ui.last_list_wheel_event = Some((pane, delta, now));
        true
    }

    fn clear_keyboard_diff_visual_selection(&mut self) {
        if should_clear_diff_visual_on_wheel(self.diff_ui.visual_selection) {
            self.clear_diff_visual_selection();
        }
    }

    fn handle_comment_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        let Some(editor_rect) = self.comment_editor.rect else {
            return;
        };
        if self.comment_editor.line_ranges.is_empty() {
            return;
        }
        let inside_editor = contains(editor_rect, mouse.column, mouse.row);
        let resolve_cursor = |app: &Self, x: u16, y: u16| -> usize {
            let row = y.saturating_sub(editor_rect.y) as usize;
            let line_idx =
                (app.comment_editor.view_start + row).min(app.comment_editor.line_ranges.len() - 1);
            let (line_start, line_end) = app.comment_editor.line_ranges[line_idx];
            let col = x
                .saturating_sub(editor_rect.x)
                .saturating_sub(app.comment_editor.text_offset)
                .min(editor_rect.width.saturating_sub(1)) as usize;
            line_cursor_with_column(&app.comment_editor.buffer, line_start, line_end, col)
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.comment_editor.cursor = idx;
                self.comment_editor.selection = None;
                self.comment_editor.mouse_anchor = Some(idx);
            }
            MouseEventKind::Drag(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.comment_editor.cursor = idx;
                if let Some(anchor) = self.comment_editor.mouse_anchor {
                    self.comment_editor.selection = (anchor != idx).then_some((anchor, idx));
                }
            }
            MouseEventKind::Up(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.comment_editor.cursor = idx;
                if let Some(anchor) = self.comment_editor.mouse_anchor.take() {
                    self.comment_editor.selection = (anchor != idx).then_some((anchor, idx));
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.comment_editor.mouse_anchor = None;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.comment_editor.mouse_anchor = None;
            }
            _ => {}
        }
    }
}
