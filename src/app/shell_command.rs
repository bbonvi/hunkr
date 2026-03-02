//! Shell command modal state, history, reverse search, and process streaming.
use super::input::shell_controller;
use crate::app::*;
use std::sync::mpsc::TryRecvError;

const SHELL_OUTPUT_POLICY: ShellOutputPolicy = ShellOutputPolicy::new(256, 512, 1_000, 16 * 1024);
const SHELL_COPY_FLASH_DURATION: Duration = Duration::from_millis(100);

/// Shared shell stream/buffer policy so limits and trim behavior stay consistent and testable.
#[derive(Debug, Clone, Copy)]
pub(super) struct ShellOutputPolicy {
    stream_max_chunks_per_tick: usize,
    stream_channel_capacity: usize,
    max_lines: usize,
    max_partial_line_bytes: usize,
}

impl ShellOutputPolicy {
    /// Builds a policy with explicit limits for stream draining and buffered shell output.
    pub(super) const fn new(
        stream_max_chunks_per_tick: usize,
        stream_channel_capacity: usize,
        max_lines: usize,
        max_partial_line_bytes: usize,
    ) -> Self {
        Self {
            stream_max_chunks_per_tick,
            stream_channel_capacity,
            max_lines,
            max_partial_line_bytes,
        }
    }

    /// Returns how many chunks to drain from the shell stream per poll tick.
    pub(super) const fn stream_max_chunks_per_tick(self) -> usize {
        self.stream_max_chunks_per_tick
    }

    /// Returns the bounded channel capacity used between shell pipe readers and the UI loop.
    pub(super) const fn stream_channel_capacity(self) -> usize {
        self.stream_channel_capacity
    }

    /// Normalizes/sanitizes a shell chunk and enforces line/tail limits.
    ///
    /// Returns how many fully buffered lines were trimmed from the front.
    pub(super) fn append_sanitized_chunk(
        self,
        output_lines: &mut Vec<String>,
        output_tail: &mut String,
        chunk: &str,
    ) -> usize {
        let normalized = chunk.replace("\r\n", "\n").replace('\r', "\n");
        let sanitized = sanitize_terminal_text(&normalized);
        output_tail.push_str(&sanitized);

        let mut consumed = 0usize;
        while let Some(rel_idx) = output_tail[consumed..].find('\n') {
            let newline_idx = consumed + rel_idx;
            let line = output_tail[consumed..newline_idx].to_owned();
            output_lines.push(line);
            consumed = newline_idx.saturating_add(1);
        }
        if consumed > 0 {
            *output_tail = output_tail[consumed..].to_owned();
        }
        if output_tail.len() > self.max_partial_line_bytes {
            let start = clamp_char_boundary(
                output_tail,
                output_tail
                    .len()
                    .saturating_sub(self.max_partial_line_bytes),
            );
            *output_tail = output_tail[start..].to_owned();
        }

        if output_lines.len() <= self.max_lines {
            return 0;
        }
        let overflow = output_lines.len() - self.max_lines;
        output_lines.drain(..overflow);
        overflow
    }
}

impl App {
    pub(super) fn open_shell_command_modal(&mut self) {
        self.ui.preferences.input_mode = InputMode::ShellCommand;
        self.reset_shell_command_editor();
    }

    pub(super) fn handle_shell_command_input(&mut self, key: KeyEvent) {
        shell_controller::dispatch_shell_modal_key(self, key);
    }

    pub(super) fn poll_shell_command_stream(&mut self) {
        let mut changed = false;
        let mut should_finalize = false;
        let mut drained_chunks = Vec::<String>::new();

        if let Some(running) = self.ui.shell_command.running.as_mut() {
            if running.exit_status.is_none() {
                match running.child.try_wait() {
                    Ok(Some(status)) => {
                        running.exit_status = Some(status);
                        changed = true;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        self.ui
                            .shell_command
                            .output_lines
                            .push(format!("hunkr: failed to query process status: {err:#}"));
                        running.exit_status = running.child.wait().ok();
                        changed = true;
                    }
                }
            }

            let mut disconnected = false;
            for _ in 0..SHELL_OUTPUT_POLICY.stream_max_chunks_per_tick() {
                match running.stream_rx.try_recv() {
                    Ok(chunk) => drained_chunks.push(chunk),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            should_finalize = running.exit_status.is_some() && disconnected;
        }

        for chunk in drained_chunks {
            self.append_shell_output_chunk(&chunk);
            changed = true;
        }

        if should_finalize {
            self.finalize_shell_command();
            changed = true;
        }

        if changed {
            self.runtime.needs_redraw = true;
        }
    }

    pub(super) fn shell_output_flash_timeout(&self) -> Option<Duration> {
        self.ui
            .shell_command
            .output_flash_clear_due
            .map(|due| due.saturating_duration_since(self.now_instant()))
    }

    pub(super) fn poll_shell_output_flash(&mut self) {
        let Some(due) = self.ui.shell_command.output_flash_clear_due else {
            return;
        };
        if self.now_instant() < due {
            return;
        }
        self.ui.shell_command.output_flash_clear_due = None;
        self.clear_shell_output_visual_selection();
        self.runtime.needs_redraw = true;
    }

    pub(super) fn close_shell_command_modal(&mut self) {
        let was_running = self.ui.shell_command.running.is_some();
        self.stop_shell_process();
        if was_running {
            self.reconcile_repository_after_shell_command();
        }
        self.ui.preferences.input_mode = InputMode::Normal;
        self.reset_shell_command_editor();
    }

    pub(super) fn shell_output_rows(&self) -> Vec<String> {
        let mut rows = Vec::<String>::new();
        if let Some(command) = self.ui.shell_command.active_command.as_deref() {
            rows.push(format!("$ {command}"));
        }
        rows.extend(self.ui.shell_command.output_lines.iter().cloned());
        if !self.ui.shell_command.output_tail.is_empty() {
            rows.push(self.ui.shell_command.output_tail.clone());
        }
        if self.ui.shell_command.finished.is_some() {
            rows.push(String::new());
        }
        rows
    }

    pub(super) fn shell_output_total_lines(&self) -> usize {
        self.shell_output_rows().len()
    }

    pub(super) fn shell_output_visual_range(&self) -> Option<(usize, usize)> {
        let len = self.shell_output_total_lines();
        if len == 0 {
            return None;
        }
        let max_idx = len - 1;
        let cursor = self.ui.shell_command.output_cursor.min(max_idx);
        if let Some(visual) = self.ui.shell_command.output_visual_selection.as_ref() {
            let anchor = visual.anchor.min(max_idx);
            Some((min(anchor, cursor), max(anchor, cursor)))
        } else {
            None
        }
    }

    pub(super) fn shell_output_max_scroll(&self) -> usize {
        let viewport = self.ui.shell_command.output_viewport.max(1);
        self.shell_output_total_lines().saturating_sub(viewport)
    }

    pub(super) fn scroll_shell_output_lines(&mut self, delta: isize) {
        let max_scroll = self.shell_output_max_scroll();
        let delta_abs = delta.saturating_abs() as usize;
        let next = if delta >= 0 {
            self.ui
                .shell_command
                .output_scroll
                .saturating_add(delta_abs)
                .min(max_scroll)
        } else {
            self.ui
                .shell_command
                .output_scroll
                .saturating_sub(delta_abs)
        };
        self.ui.shell_command.output_scroll = next;
        self.ui.shell_command.output_follow = next >= max_scroll;
    }

    pub(super) fn set_shell_output_scroll(&mut self, scroll: usize) {
        let max_scroll = self.shell_output_max_scroll();
        let next = scroll.min(max_scroll);
        self.ui.shell_command.output_scroll = next;
        self.ui.shell_command.output_follow = next >= max_scroll;
    }

    pub(super) fn move_shell_output_cursor(&mut self, delta: isize) {
        let len = self.shell_output_total_lines();
        if len == 0 {
            self.ui.shell_command.output_cursor = 0;
            return;
        }

        let next =
            (self.ui.shell_command.output_cursor as isize + delta).clamp(0, len as isize - 1);
        self.ui.shell_command.output_cursor = next as usize;
        self.ensure_shell_output_cursor_visible();
    }

    pub(super) fn page_shell_output(&mut self, direction: isize) {
        let step = self.ui.shell_command.output_viewport.max(1) as isize;
        self.move_shell_output_cursor(step.saturating_mul(direction));
    }

    pub(super) fn set_shell_output_cursor(&mut self, idx: usize) {
        let len = self.shell_output_total_lines();
        if len == 0 {
            self.ui.shell_command.output_cursor = 0;
            return;
        }
        self.ui.shell_command.output_cursor = idx.min(len - 1);
        self.ensure_shell_output_cursor_visible();
    }

    pub(super) fn ensure_shell_output_cursor_visible(&mut self) {
        let len = self.shell_output_total_lines();
        if len == 0 {
            self.ui.shell_command.output_scroll = 0;
            self.ui.shell_command.output_follow = true;
            return;
        }
        let visible = self.ui.shell_command.output_viewport.max(1);
        if self.ui.shell_command.output_cursor < self.ui.shell_command.output_scroll {
            self.ui.shell_command.output_scroll = self.ui.shell_command.output_cursor;
        } else if self.ui.shell_command.output_cursor
            >= self.ui.shell_command.output_scroll.saturating_add(visible)
        {
            self.ui.shell_command.output_scroll = self
                .ui
                .shell_command
                .output_cursor
                .saturating_add(1)
                .saturating_sub(visible);
        }
        self.ui.shell_command.output_scroll = self.ui.shell_command.output_scroll.min(len - 1);
        self.ui.shell_command.output_follow =
            self.ui.shell_command.output_scroll >= self.shell_output_max_scroll();
    }

    fn clear_shell_output_visual_selection(&mut self) {
        self.ui.shell_command.output_visual_selection = None;
        self.ui.shell_command.output_mouse_anchor = None;
        self.ui.shell_command.output_flash_clear_due = None;
    }

    pub(super) fn sync_shell_output_visual_bounds(&mut self) {
        let Some(visual) = self.ui.shell_command.output_visual_selection.as_ref() else {
            return;
        };
        let len = self.shell_output_total_lines();
        if len == 0 {
            self.ui.shell_command.output_visual_selection = None;
            return;
        }
        let max_idx = len - 1;
        if visual.anchor > max_idx {
            self.ui.shell_command.output_visual_selection = Some(ShellOutputVisualSelection {
                anchor: max_idx,
                origin: visual.origin,
            });
        }
    }

    pub(super) fn handle_shell_command_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        let Some(output_rect) = self.ui.shell_command.output_rect else {
            return;
        };
        if !contains(output_rect, mouse.column, mouse.row) {
            if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
                self.ui.shell_command.output_mouse_anchor = None;
            }
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_shell_output_lines(-3),
            MouseEventKind::ScrollDown => self.scroll_shell_output_lines(3),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(idx) = shell_output_index_at(
                    output_rect,
                    mouse.column,
                    mouse.row,
                    self.ui.shell_command.output_scroll,
                    self.shell_output_total_lines(),
                ) {
                    self.set_shell_output_cursor(idx);
                    self.clear_shell_output_visual_selection();
                    self.ui.shell_command.output_mouse_anchor = Some(idx);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(idx) = shell_output_index_at(
                    output_rect,
                    mouse.column,
                    mouse.row,
                    self.ui.shell_command.output_scroll,
                    self.shell_output_total_lines(),
                ) {
                    self.set_shell_output_cursor(idx);
                    if let Some(anchor) = self.ui.shell_command.output_mouse_anchor {
                        self.ui.shell_command.output_visual_selection = (anchor
                            != self.ui.shell_command.output_cursor)
                            .then_some(ShellOutputVisualSelection {
                                anchor,
                                origin: ShellOutputVisualOrigin::Mouse,
                            });
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(idx) = shell_output_index_at(
                    output_rect,
                    mouse.column,
                    mouse.row,
                    self.ui.shell_command.output_scroll,
                    self.shell_output_total_lines(),
                ) {
                    self.set_shell_output_cursor(idx);
                }
                self.ui.shell_command.output_mouse_anchor = None;
            }
            _ => {}
        }
    }

    pub(in crate::app) fn handle_running_shell_command_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') if key.modifiers == KeyModifiers::NONE => {
                self.close_shell_command_modal();
            }
            KeyCode::Esc => {
                if self.ui.shell_command.output_visual_selection.is_some() {
                    self.clear_shell_output_visual_selection();
                    self.runtime.status = "Shell output visual range off".to_owned();
                } else {
                    self.interrupt_shell_command();
                }
            }
            KeyCode::Backspace => self.restart_shell_command_modal(),
            KeyCode::Up | KeyCode::Char('k') => self.move_shell_output_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_shell_output_cursor(1),
            KeyCode::Char('u') | KeyCode::Char('U')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let step = page_step(self.ui.shell_command.output_viewport as u16 + 2, -0.5);
                self.move_shell_output_cursor(step);
            }
            KeyCode::Char('d') | KeyCode::Char('D')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let step = page_step(self.ui.shell_command.output_viewport as u16 + 2, 0.5);
                self.move_shell_output_cursor(step);
            }
            KeyCode::PageUp => {
                self.page_shell_output(-1);
            }
            KeyCode::PageDown => {
                self.page_shell_output(1);
            }
            KeyCode::Home => self.set_shell_output_cursor(0),
            KeyCode::End => self.set_shell_output_cursor(usize::MAX),
            KeyCode::Char('g') => self.set_shell_output_cursor(0),
            KeyCode::Char('G') => self.set_shell_output_cursor(usize::MAX),
            KeyCode::Char('v') | KeyCode::Char('V') if key.modifiers == KeyModifiers::NONE => {
                if self.ui.shell_command.output_visual_selection.is_some() {
                    self.clear_shell_output_visual_selection();
                    self.runtime.status = "Shell output visual range off".to_owned();
                } else {
                    self.ui.shell_command.output_visual_selection =
                        Some(ShellOutputVisualSelection {
                            anchor: self.ui.shell_command.output_cursor,
                            origin: ShellOutputVisualOrigin::Keyboard,
                        });
                    self.ui.shell_command.output_flash_clear_due = None;
                    self.runtime.status = "Shell output visual range on".to_owned();
                }
            }
            KeyCode::Char('y') if key.modifiers == KeyModifiers::NONE => self.copy_shell_output(),
            _ => {}
        }
        self.sync_shell_output_visual_bounds();
    }

    pub(in crate::app) fn handle_finished_shell_command_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') if key.modifiers == KeyModifiers::NONE => {
                self.close_shell_command_modal();
            }
            KeyCode::Esc => {
                if self.ui.shell_command.output_visual_selection.is_some() {
                    self.clear_shell_output_visual_selection();
                    self.runtime.status = "Shell output visual range off".to_owned();
                } else {
                    self.close_shell_command_modal();
                }
            }
            KeyCode::Enter => self.close_shell_command_modal(),
            KeyCode::Backspace => self.restart_shell_command_modal(),
            KeyCode::Up | KeyCode::Char('k') => self.move_shell_output_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_shell_output_cursor(1),
            KeyCode::Char('u') | KeyCode::Char('U')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let step = page_step(self.ui.shell_command.output_viewport as u16 + 2, -0.5);
                self.move_shell_output_cursor(step);
            }
            KeyCode::Char('d') | KeyCode::Char('D')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                let step = page_step(self.ui.shell_command.output_viewport as u16 + 2, 0.5);
                self.move_shell_output_cursor(step);
            }
            KeyCode::PageUp => {
                self.page_shell_output(-1);
            }
            KeyCode::PageDown => {
                self.page_shell_output(1);
            }
            KeyCode::Home => self.set_shell_output_cursor(0),
            KeyCode::End => self.set_shell_output_cursor(usize::MAX),
            KeyCode::Char('g') => self.set_shell_output_cursor(0),
            KeyCode::Char('G') => self.set_shell_output_cursor(usize::MAX),
            KeyCode::Char('v') | KeyCode::Char('V') if key.modifiers == KeyModifiers::NONE => {
                if self.ui.shell_command.output_visual_selection.is_some() {
                    self.clear_shell_output_visual_selection();
                    self.runtime.status = "Shell output visual range off".to_owned();
                } else {
                    self.ui.shell_command.output_visual_selection =
                        Some(ShellOutputVisualSelection {
                            anchor: self.ui.shell_command.output_cursor,
                            origin: ShellOutputVisualOrigin::Keyboard,
                        });
                    self.ui.shell_command.output_flash_clear_due = None;
                    self.runtime.status = "Shell output visual range on".to_owned();
                }
            }
            KeyCode::Char('y') if key.modifiers == KeyModifiers::NONE => self.copy_shell_output(),
            _ => {}
        }
        self.sync_shell_output_visual_bounds();
    }

    pub(in crate::app) fn handle_shell_reverse_search_input(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
        {
            self.start_or_advance_shell_reverse_search();
            return;
        }

        match key.code {
            KeyCode::Esc => self.cancel_shell_reverse_search(),
            KeyCode::Enter => self.accept_shell_reverse_search(),
            KeyCode::Backspace => {
                if let Some(state) = self.ui.shell_command.reverse_search.as_mut() {
                    state.query.pop();
                    self.refresh_shell_reverse_search_matches();
                }
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(state) = self.ui.shell_command.reverse_search.as_mut() {
                    state.query.push(c);
                    self.refresh_shell_reverse_search_matches();
                }
            }
            _ => {}
        }
    }

    pub(in crate::app) fn start_or_advance_shell_reverse_search(&mut self) {
        if self.ui.shell_command.reverse_search.is_none() {
            self.ui.shell_command.reverse_search = Some(ShellReverseSearchState {
                query: String::new(),
                match_indexes: Vec::new(),
                match_cursor: 0,
                draft_buffer: self.ui.shell_command.buffer.clone(),
            });
            self.refresh_shell_reverse_search_matches();
            return;
        }

        if let Some(state) = self.ui.shell_command.reverse_search.as_mut()
            && !state.match_indexes.is_empty()
        {
            state.match_cursor = (state.match_cursor + 1) % state.match_indexes.len();
            self.apply_shell_reverse_search_match();
        }
    }

    fn refresh_shell_reverse_search_matches(&mut self) {
        let Some(state) = self.ui.shell_command.reverse_search.as_mut() else {
            return;
        };

        state.match_indexes =
            reverse_search_match_indexes(&self.ui.shell_command.history, &state.query);

        if state.match_indexes.is_empty() {
            state.match_cursor = 0;
            self.ui.shell_command.buffer = state.draft_buffer.clone();
            self.ui.shell_command.cursor = self.ui.shell_command.buffer.len();
            return;
        }

        state.match_cursor = state
            .match_cursor
            .min(state.match_indexes.len().saturating_sub(1));
        self.apply_shell_reverse_search_match();
    }

    fn apply_shell_reverse_search_match(&mut self) {
        let Some(state) = self.ui.shell_command.reverse_search.as_ref() else {
            return;
        };
        let Some(idx) = state.match_indexes.get(state.match_cursor).copied() else {
            return;
        };
        let Some(command) = self.ui.shell_command.history.get(idx) else {
            return;
        };
        self.ui.shell_command.buffer = command.raw.clone();
        self.ui.shell_command.cursor = self.ui.shell_command.buffer.len();
    }

    fn accept_shell_reverse_search(&mut self) {
        self.ui.shell_command.reverse_search = None;
        self.ui.shell_command.history_nav = None;
    }

    fn cancel_shell_reverse_search(&mut self) {
        if let Some(state) = self.ui.shell_command.reverse_search.take() {
            self.ui.shell_command.buffer = state.draft_buffer;
            self.ui.shell_command.cursor = self.ui.shell_command.buffer.len();
        }
    }

    pub(in crate::app) fn navigate_shell_history_previous(&mut self) {
        if self.ui.shell_command.history.is_empty() {
            return;
        }

        self.ui.shell_command.reverse_search = None;

        let next = match self.ui.shell_command.history_nav {
            Some(idx) => idx.saturating_sub(1),
            None => {
                self.ui.shell_command.history_draft = self.ui.shell_command.buffer.clone();
                self.ui.shell_command.history.len().saturating_sub(1)
            }
        };

        self.ui.shell_command.history_nav = Some(next);
        if let Some(cmd) = self.ui.shell_command.history.get(next) {
            self.ui.shell_command.buffer = cmd.raw.clone();
            self.ui.shell_command.cursor = self.ui.shell_command.buffer.len();
        }
    }

    pub(in crate::app) fn navigate_shell_history_next(&mut self) {
        let Some(current) = self.ui.shell_command.history_nav else {
            return;
        };

        let next = current.saturating_add(1);
        if next >= self.ui.shell_command.history.len() {
            self.ui.shell_command.history_nav = None;
            self.ui.shell_command.buffer = self.ui.shell_command.history_draft.clone();
            self.ui.shell_command.cursor = self.ui.shell_command.buffer.len();
            return;
        }

        self.ui.shell_command.history_nav = Some(next);
        if let Some(cmd) = self.ui.shell_command.history.get(next) {
            self.ui.shell_command.buffer = cmd.raw.clone();
            self.ui.shell_command.cursor = self.ui.shell_command.buffer.len();
        }
    }

    pub(in crate::app) fn apply_shell_command_editor_key(&mut self, key: KeyEvent) {
        let edit = apply_single_line_edit_key(
            &mut self.ui.shell_command.buffer,
            &mut self.ui.shell_command.cursor,
            key,
        );
        if !matches!(edit, SingleLineEditOutcome::NotHandled) {
            self.ui.shell_command.history_nav = None;
        }
    }

    pub(in crate::app) fn execute_shell_command(&mut self) {
        let command = self.ui.shell_command.buffer.trim().to_owned();
        if command.is_empty() {
            return;
        }

        self.ui.shell_command.reverse_search = None;
        self.ui.shell_command.history_nav = None;
        self.ui.shell_command.history_draft.clear();
        self.push_shell_history(command.clone());

        self.ui.shell_command.active_command = Some(command.clone());
        self.ui.shell_command.output_lines.clear();
        self.ui.shell_command.output_tail.clear();
        self.ui.shell_command.output_cursor = 0;
        self.clear_shell_output_visual_selection();
        self.ui.shell_command.output_scroll = 0;
        self.ui.shell_command.output_follow = true;
        self.ui.shell_command.finished = None;

        match spawn_shell_command(&command) {
            Ok(running) => {
                self.ui.shell_command.running = Some(running);
                self.ui.shell_command.cursor = self.ui.shell_command.buffer.len();
                self.set_shell_output_cursor(usize::MAX);
            }
            Err(err) => {
                self.ui
                    .shell_command
                    .output_lines
                    .push(format!("hunkr: failed to launch shell command: {err:#}"));
                self.ui.shell_command.output_follow = true;
                self.snap_shell_output_to_bottom();
            }
        }
    }

    fn append_shell_output_chunk(&mut self, chunk: &str) {
        let overflow = SHELL_OUTPUT_POLICY.append_sanitized_chunk(
            &mut self.ui.shell_command.output_lines,
            &mut self.ui.shell_command.output_tail,
            chunk,
        );
        self.apply_shell_output_line_overflow(overflow);

        if self.ui.shell_command.output_follow {
            self.snap_shell_output_to_bottom();
        }
        self.sync_shell_output_visual_bounds();
    }

    fn apply_shell_output_line_overflow(&mut self, overflow: usize) {
        if overflow == 0 {
            return;
        }
        self.ui.shell_command.output_cursor =
            self.ui.shell_command.output_cursor.saturating_sub(overflow);
        self.ui.shell_command.output_scroll =
            self.ui.shell_command.output_scroll.saturating_sub(overflow);
        if let Some(visual) = self.ui.shell_command.output_visual_selection.as_ref() {
            self.ui.shell_command.output_visual_selection = Some(ShellOutputVisualSelection {
                anchor: visual.anchor.saturating_sub(overflow),
                origin: visual.origin,
            });
        }
        if let Some(anchor) = self.ui.shell_command.output_mouse_anchor {
            self.ui.shell_command.output_mouse_anchor = Some(anchor.saturating_sub(overflow));
        }
    }

    fn finalize_shell_command(&mut self) {
        let Some(mut running) = self.ui.shell_command.running.take() else {
            return;
        };

        if let Some(handle) = running.stdout_reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = running.stderr_reader.take() {
            let _ = handle.join();
        }

        let Some(status) = running.exit_status.or_else(|| running.child.wait().ok()) else {
            self.ui
                .shell_command
                .output_lines
                .push("hunkr: failed to collect process exit status".to_owned());
            return;
        };
        self.ui.shell_command.finished = Some(ShellCommandResult {
            exit_status: status,
        });
        self.reconcile_repository_after_shell_command();

        if self.ui.shell_command.output_follow {
            self.snap_shell_output_to_bottom();
        }
        self.sync_shell_output_visual_bounds();
    }

    fn stop_shell_process(&mut self) {
        let Some(mut running) = self.ui.shell_command.running.take() else {
            return;
        };

        if let Some(pgid) = running.process_group_id {
            let _ = kill_process_group(pgid);
        }
        let _ = running.child.kill();
        let _ = running.child.wait();

        if let Some(handle) = running.stdout_reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = running.stderr_reader.take() {
            let _ = handle.join();
        }
    }

    fn interrupt_shell_command(&mut self) {
        let Some(running) = self.ui.shell_command.running.as_mut() else {
            return;
        };
        if let Some(pgid) = running.process_group_id {
            match kill_process_group(pgid) {
                Ok(()) => {
                    self.runtime.status = "Shell command interrupted".to_owned();
                    return;
                }
                Err(err) => {
                    self.runtime.status = format!(
                        "Failed to interrupt shell process group: {err}; trying direct kill"
                    );
                }
            }
        }

        match running.child.kill() {
            Ok(()) => {
                self.runtime.status = "Shell command interrupted".to_owned();
            }
            Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
                self.runtime.status = "Shell command already exited".to_owned();
            }
            Err(err) => {
                self.runtime.status = format!("Failed to interrupt shell command: {err}");
            }
        }
    }

    fn restart_shell_command_modal(&mut self) {
        let was_running = self.ui.shell_command.running.is_some();
        self.stop_shell_process();
        if was_running {
            self.reconcile_repository_after_shell_command();
        }
        self.reset_shell_command_editor();
        self.runtime.status = "Shell modal reset".to_owned();
    }

    fn reset_shell_command_editor(&mut self) {
        self.ui.shell_command.buffer.clear();
        self.ui.shell_command.cursor = 0;
        self.ui.shell_command.history_nav = None;
        self.ui.shell_command.history_draft.clear();
        self.ui.shell_command.reverse_search = None;
        self.ui.shell_command.active_command = None;
        self.ui.shell_command.output_lines.clear();
        self.ui.shell_command.output_tail.clear();
        self.ui.shell_command.output_cursor = 0;
        self.ui.shell_command.output_visual_selection = None;
        self.ui.shell_command.output_mouse_anchor = None;
        self.ui.shell_command.output_flash_clear_due = None;
        self.ui.shell_command.output_scroll = 0;
        self.ui.shell_command.output_follow = true;
        self.ui.shell_command.finished = None;
    }

    fn push_shell_history(&mut self, command: String) {
        self.ui
            .shell_command
            .history
            .push_back(ShellCommandHistoryEntry::new(command));
        while self.ui.shell_command.history.len() > SHELL_HISTORY_LIMIT {
            self.ui.shell_command.history.pop_front();
        }

        let snapshot = self
            .ui
            .shell_command
            .history
            .iter()
            .map(|entry| entry.raw.clone())
            .collect::<Vec<_>>();
        if let Err(err) = self.deps.store.save_shell_history(&snapshot) {
            self.runtime.status = format!("failed to save shell history: {err:#}");
        }
    }

    fn snap_shell_output_to_bottom(&mut self) {
        let max_scroll = self.shell_output_max_scroll();
        self.ui.shell_command.output_scroll = max_scroll;
        let len = self.shell_output_total_lines();
        self.ui.shell_command.output_cursor = len.saturating_sub(1);
        self.ui.shell_command.output_follow = true;
    }

    fn copy_shell_output(&mut self) {
        let rows = self.shell_output_rows();
        if rows.is_empty() {
            self.runtime.status = "No shell output to copy".to_owned();
            return;
        }

        let had_visual = self.ui.shell_command.output_visual_selection.is_some();
        let payload = if had_visual {
            shell_output_copy_payload_for_rows(&rows, self.shell_output_visual_range())
        } else {
            shell_output_copy_payload_for_rows(&rows, None)
        };
        let Some(payload) = payload else {
            self.runtime.status = "No shell output to copy".to_owned();
            return;
        };

        let post_action = selection_copy_post_action(had_visual, Some(SHELL_COPY_FLASH_DURATION));
        if let SelectionCopyPostAction::FlashThenClear(duration) = post_action {
            self.ui.shell_command.output_visual_selection = Some(ShellOutputVisualSelection {
                anchor: 0,
                origin: ShellOutputVisualOrigin::Keyboard,
            });
            self.ui.shell_command.output_cursor = rows.len().saturating_sub(1);
            self.ui.shell_command.output_flash_clear_due = Some(self.now_instant() + duration);
            self.ensure_shell_output_cursor_visible();
            self.runtime.needs_redraw = true;
        }

        let line_count = payload.lines().count().max(1);
        self.runtime.status = clipboard_copy_status(
            crate::clipboard::copy_to_clipboard_with_fallbacks(&payload),
            format!("{line_count} shell line(s)"),
            "shell output",
        );

        if matches!(post_action, SelectionCopyPostAction::ClearNow) {
            self.clear_shell_output_visual_selection();
        }
    }

    fn reconcile_repository_after_shell_command(&mut self) {
        let previous_branch = self.deps.git.branch_name().to_owned();
        let target = self.deps.git.root().to_path_buf();
        if let Err(err) = self.switch_repository_context(&target) {
            self.runtime.status =
                format!("shell sync failed to refresh repository context: {err:#}");
            return;
        }

        let next_branch = self.deps.git.branch_name().to_owned();
        if previous_branch != next_branch {
            self.runtime.status =
                format!("repository switched: {previous_branch} -> {next_branch}");
        }
    }
}

fn reverse_search_match_indexes(
    history: &VecDeque<ShellCommandHistoryEntry>,
    query: &str,
) -> Vec<usize> {
    let needle = query.to_ascii_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }

    let mut seen = std::collections::HashSet::<&str>::new();
    history
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(idx, command)| {
            if !command.raw_lower.contains(&needle) || !seen.insert(command.raw_lower.as_str()) {
                return None;
            }
            Some(idx)
        })
        .collect()
}

pub(super) fn shell_output_index_at(
    rect: ratatui::layout::Rect,
    x: u16,
    y: u16,
    scroll: usize,
    total_lines: usize,
) -> Option<usize> {
    if total_lines == 0 || !contains(rect, x, y) {
        return None;
    }
    let row = y.saturating_sub(rect.y) as usize;
    Some((scroll + row).min(total_lines - 1))
}

pub(super) fn shell_output_copy_payload_for_rows(
    rows: &[String],
    visual_range: Option<(usize, usize)>,
) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let selected = if let Some((start, end)) = visual_range {
        let end = end.min(rows.len().saturating_sub(1));
        let start = start.min(end);
        rows[start..=end].to_vec()
    } else {
        rows.to_vec()
    };
    Some(selected.join("\n"))
}

fn spawn_shell_command(command: &str) -> anyhow::Result<RunningShellCommand> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned());

    let (mut child, process_group_id) = spawn_shell_process(&shell, command)?;

    let stdout = child
        .stdout
        .take()
        .context("failed to capture child stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("failed to capture child stderr")?;

    let (tx, rx) = mpsc::sync_channel::<String>(SHELL_OUTPUT_POLICY.stream_channel_capacity());
    let stdout_reader = Some(spawn_shell_pipe_reader(stdout, tx.clone()));
    let stderr_reader = Some(spawn_shell_pipe_reader(stderr, tx));

    Ok(RunningShellCommand {
        child,
        process_group_id,
        stream_rx: rx,
        stdout_reader,
        stderr_reader,
        exit_status: None,
    })
}

fn spawn_shell_process(shell: &str, command: &str) -> anyhow::Result<(Child, Option<u32>)> {
    #[cfg(unix)]
    {
        let mut command_builder = Command::new("setsid");
        command_builder.arg(shell).arg("-lc").arg(command);
        apply_shell_non_interactive_env(&mut command_builder);
        command_builder
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Ok(child) = command_builder.spawn() {
            let pgid = child.id();
            return Ok((child, Some(pgid)));
        }
    }

    let mut command_builder = Command::new(shell);
    command_builder.arg("-lc").arg(command);
    apply_shell_non_interactive_env(&mut command_builder);
    let child = command_builder
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn `{shell} -lc <command>`"))?;
    Ok((child, None))
}

/// Configure child shell environment for deterministic non-interactive execution.
///
/// We always run with `stdin` disconnected, so pager/prompt tools must be disabled
/// to avoid blocking or confusing interactive output modes.
fn apply_shell_non_interactive_env(command: &mut Command) {
    command
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0");
}

fn spawn_shell_pipe_reader<R>(mut reader: R, tx: mpsc::SyncSender<String>) -> JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buf = [0_u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => {
                    if tx
                        .send(String::from_utf8_lossy(&buf[..read]).to_string())
                        .is_err()
                    {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx.send(format!("hunkr: stream read failed: {err}"));
                    break;
                }
            }
        }
    })
}

#[cfg(unix)]
fn kill_process_group(process_group_id: u32) -> std::io::Result<()> {
    let status = Command::new("kill")
        .arg("-KILL")
        .arg(format!("-{process_group_id}"))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "`kill -KILL -{process_group_id}` exited with status {status}"
        )))
    }
}

#[cfg(not(unix))]
fn kill_process_group(_process_group_id: u32) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process-group kill is only supported on unix",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsStr;

    #[test]
    fn shell_non_interactive_env_disables_pager_and_terminal_prompts() {
        let mut command = Command::new("/bin/sh");
        apply_shell_non_interactive_env(&mut command);
        let envs = command
            .get_envs()
            .map(|(key, value)| (key.to_os_string(), value.map(|v| v.to_os_string())))
            .collect::<HashMap<_, _>>();

        assert_eq!(
            envs.get(OsStr::new("GIT_PAGER")).and_then(|v| v.as_deref()),
            Some(OsStr::new("cat"))
        );
        assert_eq!(
            envs.get(OsStr::new("PAGER")).and_then(|v| v.as_deref()),
            Some(OsStr::new("cat"))
        );
        assert_eq!(
            envs.get(OsStr::new("GIT_TERMINAL_PROMPT"))
                .and_then(|v| v.as_deref()),
            Some(OsStr::new("0"))
        );
    }

    #[test]
    fn reverse_search_match_indexes_returns_empty_for_empty_query() {
        let history = std::collections::VecDeque::from(vec![
            ShellCommandHistoryEntry::new("git status".to_owned()),
            ShellCommandHistoryEntry::new("cargo test".to_owned()),
        ]);

        assert!(reverse_search_match_indexes(&history, "").is_empty());
    }

    #[test]
    fn reverse_search_match_indexes_is_newest_first_and_deduplicated() {
        let history = std::collections::VecDeque::from(vec![
            ShellCommandHistoryEntry::new("git status".to_owned()),
            ShellCommandHistoryEntry::new("cargo test".to_owned()),
            ShellCommandHistoryEntry::new("cargo fmt".to_owned()),
            ShellCommandHistoryEntry::new("cargo test".to_owned()),
        ]);

        assert_eq!(reverse_search_match_indexes(&history, "TEST"), vec![3]);
        assert_eq!(reverse_search_match_indexes(&history, "cargo"), vec![3, 2]);
    }
}
