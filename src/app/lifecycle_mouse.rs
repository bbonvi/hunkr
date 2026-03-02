//! Mouse interaction handlers for list panes, diff, and comment editor modal.
use super::input::modal_controller;
use crate::app::*;

impl App {
    pub(super) fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        if self.onboarding_active() {
            return;
        }

        if modal_controller::dispatch_modal_mouse(self, mouse) {
            return;
        }
        if matches!(
            self.ui.preferences.input_mode,
            InputMode::WorktreeSwitch | InputMode::DiffSearch | InputMode::ListSearch(_)
        ) {
            return;
        }
        let x = mouse.column;
        let y = mouse.row;

        let in_files = contains(self.ui.diff_ui.pane_rects.files, x, y);
        let in_commits = contains(self.ui.diff_ui.pane_rects.commits, x, y);
        let in_diff = contains(self.ui.diff_ui.pane_rects.diff, x, y);
        let resolve_diff_row = |app: &Self, mouse_y: u16| -> Option<usize> {
            let viewport_rows = app
                .ui
                .diff_ui
                .pane_rects
                .diff
                .height
                .saturating_sub(2)
                .max(1) as usize;
            let sticky_banner_indexes = app
                .sticky_banner_indexes_for_scroll(app.domain.diff_position.scroll, viewport_rows);
            diff_index_at(
                mouse_y,
                app.ui.diff_ui.pane_rects.diff,
                app.domain.diff_position.scroll,
                &sticky_banner_indexes,
            )
        };
        let resolve_commit_visible_idx = |app: &Self, mouse_y: u16| -> Option<usize> {
            list_index_at(
                mouse_y,
                app.ui.diff_ui.pane_rects.commits,
                app.ui.commit_ui.list_state.offset(),
            )
        };

        match mouse.kind {
            MouseEventKind::ScrollDown => {
                if in_diff {
                    self.clear_keyboard_diff_visual_selection();
                    self.scroll_diff_viewport(self.ui.preferences.diff_wheel_scroll_lines);
                } else if in_files && self.should_scroll_list_wheel(FocusPane::Files, 1) {
                    self.scroll_file_list_lines(1);
                } else if in_commits && self.should_scroll_list_wheel(FocusPane::Commits, 1) {
                    self.ui.commit_ui.visual_anchor = None;
                    self.scroll_commit_list_lines(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if in_diff {
                    self.clear_keyboard_diff_visual_selection();
                    self.scroll_diff_viewport(-self.ui.preferences.diff_wheel_scroll_lines);
                } else if in_files && self.should_scroll_list_wheel(FocusPane::Files, -1) {
                    self.scroll_file_list_lines(-1);
                } else if in_commits && self.should_scroll_list_wheel(FocusPane::Commits, -1) {
                    self.ui.commit_ui.visual_anchor = None;
                    self.scroll_commit_list_lines(-1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.ui.commit_ui.mouse_anchor = None;
                self.ui.commit_ui.mouse_dragging = false;
                self.ui.commit_ui.mouse_drag_mode = None;
                self.ui.commit_ui.mouse_drag_baseline = None;
                if in_files {
                    self.set_focus(FocusPane::Files);
                    self.ui.diff_ui.mouse_anchor = None;
                    if let Some(idx) = list_index_at(
                        y,
                        self.ui.diff_ui.pane_rects.files,
                        self.ui.file_ui.list_state.offset(),
                    ) {
                        self.select_file_row(idx);
                    }
                } else if in_commits {
                    self.set_focus(FocusPane::Commits);
                    self.ui.diff_ui.mouse_anchor = None;
                    self.ui.commit_ui.visual_anchor = None;
                    if let Some(idx) = resolve_commit_visible_idx(self, y) {
                        let drag_mode = commit_mouse_selection_mode(mouse.modifiers);
                        let baseline = self.domain.commits.iter().map(|row| row.selected).collect();
                        let clicked_full_idx =
                            self.select_commit_row_with_mouse(idx, mouse.modifiers);
                        if matches!(
                            drag_mode,
                            CommitMouseSelectionMode::Replace | CommitMouseSelectionMode::Toggle
                        ) {
                            self.ui.commit_ui.mouse_anchor = clicked_full_idx;
                            self.ui.commit_ui.mouse_drag_mode = Some(drag_mode);
                            if drag_mode == CommitMouseSelectionMode::Toggle {
                                self.ui.commit_ui.mouse_drag_baseline = Some(baseline);
                            }
                        } else {
                            self.ui.commit_ui.mouse_anchor = None;
                        }
                    }
                } else if in_diff {
                    self.set_focus(FocusPane::Diff);
                    self.ui.diff_ui.visual_selection = None;
                    if let Some(row) = resolve_diff_row(self, y) {
                        self.sync_diff_cursor_to_mouse_position(row, x);
                        self.ui.diff_ui.mouse_anchor = Some(self.domain.diff_position.cursor);
                    } else {
                        self.ui.diff_ui.mouse_anchor = None;
                    }
                } else {
                    self.ui.diff_ui.mouse_anchor = None;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.ui.commit_ui.mouse_anchor.is_some() => {
                let edge_delta = list_drag_scroll_delta(
                    y,
                    self.ui.diff_ui.pane_rects.commits,
                    LIST_DRAG_EDGE_MARGIN,
                );
                if edge_delta != 0 {
                    self.scroll_commit_list_lines(edge_delta);
                }

                let target_visible_idx =
                    resolve_commit_visible_idx(self, y).or(self.ui.commit_ui.list_state.selected());
                if let Some(visible_idx) = target_visible_idx {
                    let visible_indices = self.visible_commit_indices();
                    if let Some(full_idx) = visible_indices.get(visible_idx).copied() {
                        self.ui.commit_ui.list_state.select(Some(visible_idx));
                        let anchor = self.ui.commit_ui.mouse_anchor.expect("checked above");
                        match self.ui.commit_ui.mouse_drag_mode {
                            Some(CommitMouseSelectionMode::Toggle) => {
                                if let Some(baseline) =
                                    self.ui.commit_ui.mouse_drag_baseline.as_deref()
                                {
                                    apply_toggle_range_from_baseline(
                                        &mut self.domain.commits,
                                        baseline,
                                        anchor,
                                        full_idx,
                                    );
                                } else {
                                    apply_range_selection(
                                        &mut self.domain.commits,
                                        anchor,
                                        full_idx,
                                    );
                                }
                            }
                            _ => apply_range_selection(&mut self.domain.commits, anchor, full_idx),
                        }
                        if anchor != full_idx {
                            self.ui.commit_ui.mouse_dragging = true;
                        }
                        if self.ui.commit_ui.mouse_dragging {
                            self.on_selection_changed_debounced();
                        }
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if in_diff => {
                if let Some(row) = resolve_diff_row(self, y) {
                    self.sync_diff_cursor_to_mouse_position(row, x);
                    self.ui.diff_ui.visual_selection = diff_visual_from_drag_anchor(
                        self.ui.diff_ui.mouse_anchor,
                        self.domain.diff_position.cursor,
                    );
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let commit_dragging = self.ui.commit_ui.mouse_dragging;
                self.ui.commit_ui.mouse_anchor = None;
                self.ui.commit_ui.mouse_dragging = false;
                self.ui.commit_ui.mouse_drag_mode = None;
                self.ui.commit_ui.mouse_drag_baseline = None;

                if in_diff {
                    if let Some(row) = resolve_diff_row(self, y) {
                        self.sync_diff_cursor_to_mouse_position(row, x);
                    }
                    self.ui.diff_ui.visual_selection = diff_visual_from_drag_anchor(
                        self.ui.diff_ui.mouse_anchor,
                        self.domain.diff_position.cursor,
                    );
                }
                self.ui.diff_ui.mouse_anchor = None;

                if commit_dragging {
                    self.on_selection_changed();
                }
            }
            _ => {}
        }
    }

    fn sync_diff_cursor_to_mouse_position(&mut self, row: usize, mouse_x: u16) {
        self.set_diff_cursor(row);
        let col = diff_column_at_for_rendered_line(
            mouse_x,
            self.ui.diff_ui.pane_rects.diff,
            self.domain
                .rendered_diff
                .get(self.domain.diff_position.cursor),
        );
        self.set_diff_block_cursor_col(col);
    }

    fn should_scroll_list_wheel(&mut self, pane: FocusPane, delta: isize) -> bool {
        let now = self.now_instant();
        if list_wheel_event_is_duplicate(
            self.ui.diff_ui.last_list_wheel_event,
            pane,
            delta,
            now,
            self.ui.preferences.list_wheel_coalesce,
        ) {
            return false;
        }
        self.ui.diff_ui.last_list_wheel_event = Some((pane, delta, now));
        true
    }

    fn clear_keyboard_diff_visual_selection(&mut self) {
        if should_clear_diff_visual_on_wheel(self.ui.diff_ui.visual_selection) {
            self.clear_diff_visual_selection();
        }
    }

    pub(in crate::app) fn handle_comment_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        let Some(editor_rect) = self.ui.comment_editor.rect else {
            return;
        };
        if self.ui.comment_editor.line_ranges.is_empty() {
            return;
        }
        let inside_editor = contains(editor_rect, mouse.column, mouse.row);
        let resolve_cursor = |app: &Self, x: u16, y: u16| -> usize {
            let row = y.saturating_sub(editor_rect.y) as usize;
            let line_idx = row.min(app.ui.comment_editor.line_ranges.len() - 1);
            let (line_start, line_end) = app.ui.comment_editor.line_ranges[line_idx];
            let col = x
                .saturating_sub(editor_rect.x)
                .saturating_sub(app.ui.comment_editor.text_offset)
                .min(editor_rect.width.saturating_sub(1)) as usize;
            line_cursor_with_column(&app.ui.comment_editor.buffer, line_start, line_end, col)
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.ui.comment_editor.cursor = idx;
                self.ui.comment_editor.selection = None;
                self.ui.comment_editor.mouse_anchor = Some(idx);
            }
            MouseEventKind::Drag(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.ui.comment_editor.cursor = idx;
                if let Some(anchor) = self.ui.comment_editor.mouse_anchor {
                    self.ui.comment_editor.selection = (anchor != idx).then_some((anchor, idx));
                }
            }
            MouseEventKind::Up(MouseButton::Left) if inside_editor => {
                let idx = resolve_cursor(self, mouse.column, mouse.row);
                self.ui.comment_editor.cursor = idx;
                if let Some(anchor) = self.ui.comment_editor.mouse_anchor.take() {
                    self.ui.comment_editor.selection = (anchor != idx).then_some((anchor, idx));
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.ui.comment_editor.mouse_anchor = None;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.ui.comment_editor.mouse_anchor = None;
            }
            _ => {}
        }
    }
}
