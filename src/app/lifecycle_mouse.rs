//! Mouse interaction handlers for list panes and diff interactions.
use super::input::modal_controller;
use crate::app::*;

impl App {
    pub(super) fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        if self.dispatch_helper_click(mouse) {
            return;
        }

        if self.runtime.show_help {
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
        let resolve_diff_row = |app: &Self, mouse_y: u16| -> Option<DiffVisibleRow> {
            let rect = app.ui.diff_ui.pane_rects.diff;
            if rect.height < 3 || mouse_y <= rect.y || mouse_y >= rect.y + rect.height - 1 {
                return None;
            }
            let row = mouse_y.saturating_sub(rect.y + 1) as usize;
            if let Some(entry) = app.ui.diff_ui.visible_rows.get(row).copied() {
                return Some(entry);
            }

            let viewport_rows = rect.height.saturating_sub(2).max(1) as usize;
            let sticky_banner_indexes = app
                .sticky_banner_indexes_for_scroll(app.domain.diff_position.scroll, viewport_rows);
            diff_index_at(
                mouse_y,
                rect,
                app.domain.diff_position.scroll,
                &sticky_banner_indexes,
            )
            .map(|line_index| DiffVisibleRow {
                line_index,
                wrapped_row_offset: 0,
            })
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
                    self.set_focus(FocusPane::Diff);
                    self.clear_keyboard_diff_visual_selection();
                    self.scroll_diff_viewport(self.ui.preferences.diff_wheel_scroll_lines);
                } else if in_files {
                    self.set_focus(FocusPane::Files);
                    if self.should_scroll_list_wheel(FocusPane::Files, 1) {
                        self.scroll_file_list_lines(1);
                    }
                } else if in_commits {
                    self.set_focus(FocusPane::Commits);
                    if self.should_scroll_list_wheel(FocusPane::Commits, 1) {
                        self.ui.commit_ui.visual_anchor = None;
                        self.scroll_commit_list_lines(1);
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if in_diff {
                    self.set_focus(FocusPane::Diff);
                    self.clear_keyboard_diff_visual_selection();
                    self.scroll_diff_viewport(-self.ui.preferences.diff_wheel_scroll_lines);
                } else if in_files {
                    self.set_focus(FocusPane::Files);
                    if self.should_scroll_list_wheel(FocusPane::Files, -1) {
                        self.scroll_file_list_lines(-1);
                    }
                } else if in_commits {
                    self.set_focus(FocusPane::Commits);
                    if self.should_scroll_list_wheel(FocusPane::Commits, -1) {
                        self.ui.commit_ui.visual_anchor = None;
                        self.scroll_commit_list_lines(-1);
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.ui.commit_ui.mouse_anchor = None;
                self.ui.commit_ui.mouse_dragging = false;
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
                        let clicked_full_idx =
                            self.select_commit_row_with_mouse(idx, mouse.modifiers);
                        self.ui.commit_ui.mouse_anchor = match drag_mode {
                            CommitMouseSelectionMode::Replace => clicked_full_idx,
                            CommitMouseSelectionMode::Range => self.ui.commit_ui.selection_anchor,
                        };
                    }
                } else if in_diff {
                    self.set_focus(FocusPane::Diff);
                    self.ui.diff_ui.visual_selection = None;
                    if let Some(row) = resolve_diff_row(self, y) {
                        self.sync_diff_cursor_to_mouse_position(
                            row.line_index,
                            row.wrapped_row_offset,
                            x,
                        );
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
                        apply_range_selection(&mut self.domain.commits, anchor, full_idx);
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
                    self.sync_diff_cursor_to_mouse_position(
                        row.line_index,
                        row.wrapped_row_offset,
                        x,
                    );
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

                if in_diff {
                    if let Some(row) = resolve_diff_row(self, y) {
                        self.sync_diff_cursor_to_mouse_position(
                            row.line_index,
                            row.wrapped_row_offset,
                            x,
                        );
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

    fn sync_diff_cursor_to_mouse_position(
        &mut self,
        line_index: usize,
        wrapped_row_offset: usize,
        mouse_x: u16,
    ) {
        self.set_diff_cursor(line_index);
        let col = diff_column_at_for_rendered_line(
            mouse_x,
            self.ui.diff_ui.pane_rects.diff,
            wrapped_row_offset,
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

    fn dispatch_helper_click(&mut self, mouse: crossterm::event::MouseEvent) -> bool {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return false;
        }
        let Some(hitbox) = self
            .ui
            .helper_click_hitboxes
            .iter()
            .rev()
            .find(|hitbox| contains(hitbox.rect, mouse.column, mouse.row))
            .copied()
        else {
            return false;
        };
        self.trigger_helper_click_action(hitbox.action);
        true
    }

    fn trigger_helper_click_action(&mut self, action: HelperClickAction) {
        match action {
            HelperClickAction::Key { code, modifiers } => {
                let key = KeyEvent::new(code, modifiers);
                self.handle_key(key);
            }
        }
    }
}
