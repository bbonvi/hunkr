//! Shell command modal state, history, reverse search, and process streaming.
use super::*;
use std::sync::mpsc::TryRecvError;

impl App {
    pub(super) fn open_shell_command_modal(&mut self) {
        self.preferences.input_mode = InputMode::ShellCommand;
        self.reset_shell_command_editor();
    }

    pub(super) fn handle_shell_command_input(&mut self, key: KeyEvent) {
        if self.shell_command.running.is_some() {
            self.handle_running_shell_command_input(key);
            return;
        }

        if self.shell_command.finished.is_some() {
            self.handle_finished_shell_command_input(key);
            return;
        }

        if self.shell_command.reverse_search.is_some() {
            self.handle_shell_reverse_search_input(key);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
        {
            self.start_or_advance_shell_reverse_search();
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P'))
        {
            self.navigate_shell_history_previous();
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N'))
        {
            self.navigate_shell_history_next();
            return;
        }

        match key.code {
            KeyCode::Esc => self.close_shell_command_modal(),
            KeyCode::Enter => self.execute_shell_command(),
            KeyCode::Up => self.navigate_shell_history_previous(),
            KeyCode::Down => self.navigate_shell_history_next(),
            KeyCode::PageUp => {
                self.scroll_shell_output_lines(-(self.shell_command.output_viewport as isize))
            }
            KeyCode::PageDown => {
                self.scroll_shell_output_lines(self.shell_command.output_viewport as isize)
            }
            KeyCode::Home => {
                self.shell_command.cursor = 0;
                self.shell_command.history_nav = None;
            }
            KeyCode::End => {
                self.shell_command.cursor = self.shell_command.buffer.len();
                self.shell_command.history_nav = None;
            }
            KeyCode::Left
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.shell_command.cursor =
                    prev_word_boundary(&self.shell_command.buffer, self.shell_command.cursor);
                self.shell_command.history_nav = None;
            }
            KeyCode::Left => {
                self.shell_command.cursor = prev_char_boundary(
                    &self.shell_command.buffer,
                    clamp_char_boundary(&self.shell_command.buffer, self.shell_command.cursor),
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Right
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.shell_command.cursor =
                    next_word_boundary(&self.shell_command.buffer, self.shell_command.cursor);
                self.shell_command.history_nav = None;
            }
            KeyCode::Right => {
                self.shell_command.cursor = next_char_boundary(
                    &self.shell_command.buffer,
                    clamp_char_boundary(&self.shell_command.buffer, self.shell_command.cursor),
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Backspace
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                delete_prev_word(
                    &mut self.shell_command.buffer,
                    &mut self.shell_command.cursor,
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Backspace => {
                delete_prev_char(
                    &mut self.shell_command.buffer,
                    &mut self.shell_command.cursor,
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Delete
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                delete_next_word(
                    &mut self.shell_command.buffer,
                    &mut self.shell_command.cursor,
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Delete => {
                delete_next_char(
                    &mut self.shell_command.buffer,
                    &mut self.shell_command.cursor,
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Char('a') | KeyCode::Char('A')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.shell_command.cursor = 0;
                self.shell_command.history_nav = None;
            }
            KeyCode::Char('e') | KeyCode::Char('E')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.shell_command.cursor = self.shell_command.buffer.len();
                self.shell_command.history_nav = None;
            }
            KeyCode::Char('u') | KeyCode::Char('U')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                delete_to_line_start(
                    &mut self.shell_command.buffer,
                    &mut self.shell_command.cursor,
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Char('k') | KeyCode::Char('K')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                delete_to_line_end(
                    &mut self.shell_command.buffer,
                    &mut self.shell_command.cursor,
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Char('w') | KeyCode::Char('W')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                delete_prev_word(
                    &mut self.shell_command.buffer,
                    &mut self.shell_command.cursor,
                );
                self.shell_command.history_nav = None;
            }
            KeyCode::Char('b') | KeyCode::Char('B')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.shell_command.cursor =
                    prev_word_boundary(&self.shell_command.buffer, self.shell_command.cursor);
                self.shell_command.history_nav = None;
            }
            KeyCode::Char('f') | KeyCode::Char('F')
                if key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.shell_command.cursor =
                    next_word_boundary(&self.shell_command.buffer, self.shell_command.cursor);
                self.shell_command.history_nav = None;
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at_cursor(
                    &mut self.shell_command.buffer,
                    &mut self.shell_command.cursor,
                    c,
                );
                self.shell_command.history_nav = None;
            }
            _ => {}
        }
    }

    pub(super) fn poll_shell_command_stream(&mut self) {
        let mut changed = false;
        let mut should_finalize = false;
        let mut drained_chunks = Vec::<String>::new();

        if let Some(running) = self.shell_command.running.as_mut() {
            if running.exit_status.is_none() {
                match running.child.try_wait() {
                    Ok(Some(status)) => {
                        running.exit_status = Some(status);
                        changed = true;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        self.shell_command
                            .output_lines
                            .push(format!("hunkr: failed to query process status: {err:#}"));
                        running.exit_status = running.child.wait().ok();
                        changed = true;
                    }
                }
            }

            let mut disconnected = false;
            loop {
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

    pub(super) fn close_shell_command_modal(&mut self) {
        let was_running = self.shell_command.running.is_some();
        self.stop_shell_process();
        if was_running {
            self.reconcile_repository_after_shell_command();
        }
        self.preferences.input_mode = InputMode::Normal;
        self.reset_shell_command_editor();
    }

    pub(super) fn shell_output_total_lines(&self) -> usize {
        let mut total = 0usize;
        if self.shell_command.active_command.is_some() {
            total = total.saturating_add(1);
        }
        total = total.saturating_add(self.shell_command.output_lines.len());
        if !self.shell_command.output_tail.is_empty() {
            total = total.saturating_add(1);
        }
        if self.shell_command.finished.is_some() {
            total = total.saturating_add(2);
        }
        total
    }

    pub(super) fn shell_output_max_scroll(&self) -> usize {
        let viewport = self.shell_command.output_viewport.max(1);
        self.shell_output_total_lines().saturating_sub(viewport)
    }

    pub(super) fn scroll_shell_output_lines(&mut self, delta: isize) {
        let max_scroll = self.shell_output_max_scroll();
        let delta_abs = delta.saturating_abs() as usize;
        let next = if delta >= 0 {
            self.shell_command
                .output_scroll
                .saturating_add(delta_abs)
                .min(max_scroll)
        } else {
            self.shell_command.output_scroll.saturating_sub(delta_abs)
        };
        self.shell_command.output_scroll = next;
        self.shell_command.output_follow = next >= max_scroll;
    }

    pub(super) fn set_shell_output_scroll(&mut self, scroll: usize) {
        let max_scroll = self.shell_output_max_scroll();
        let next = scroll.min(max_scroll);
        self.shell_command.output_scroll = next;
        self.shell_command.output_follow = next >= max_scroll;
    }

    pub(super) fn handle_shell_command_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        let Some(output_rect) = self.shell_command.output_rect else {
            return;
        };
        if !contains(output_rect, mouse.column, mouse.row) {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_shell_output_lines(-3),
            MouseEventKind::ScrollDown => self.scroll_shell_output_lines(3),
            _ => {}
        }
    }

    fn handle_running_shell_command_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_shell_command_modal(),
            KeyCode::Up => self.scroll_shell_output_lines(-1),
            KeyCode::Down => self.scroll_shell_output_lines(1),
            KeyCode::PageUp => {
                self.scroll_shell_output_lines(-(self.shell_command.output_viewport as isize))
            }
            KeyCode::PageDown => {
                self.scroll_shell_output_lines(self.shell_command.output_viewport as isize)
            }
            KeyCode::Home => self.set_shell_output_scroll(0),
            KeyCode::End => {
                let max_scroll = self.shell_output_max_scroll();
                self.set_shell_output_scroll(max_scroll);
            }
            _ => {}
        }
    }

    fn handle_finished_shell_command_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.close_shell_command_modal(),
            KeyCode::Up => self.scroll_shell_output_lines(-1),
            KeyCode::Down => self.scroll_shell_output_lines(1),
            KeyCode::PageUp => {
                self.scroll_shell_output_lines(-(self.shell_command.output_viewport as isize))
            }
            KeyCode::PageDown => {
                self.scroll_shell_output_lines(self.shell_command.output_viewport as isize)
            }
            KeyCode::Home => self.set_shell_output_scroll(0),
            KeyCode::End => {
                let max_scroll = self.shell_output_max_scroll();
                self.set_shell_output_scroll(max_scroll);
            }
            _ => {}
        }
    }

    fn handle_shell_reverse_search_input(&mut self, key: KeyEvent) {
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
                if let Some(state) = self.shell_command.reverse_search.as_mut() {
                    state.query.pop();
                    self.refresh_shell_reverse_search_matches();
                }
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Some(state) = self.shell_command.reverse_search.as_mut() {
                    state.query.push(c);
                    self.refresh_shell_reverse_search_matches();
                }
            }
            _ => {}
        }
    }

    fn start_or_advance_shell_reverse_search(&mut self) {
        if self.shell_command.reverse_search.is_none() {
            self.shell_command.reverse_search = Some(ShellReverseSearchState {
                query: String::new(),
                match_indexes: Vec::new(),
                match_cursor: 0,
                draft_buffer: self.shell_command.buffer.clone(),
            });
            self.refresh_shell_reverse_search_matches();
            return;
        }

        if let Some(state) = self.shell_command.reverse_search.as_mut()
            && !state.match_indexes.is_empty()
        {
            state.match_cursor = (state.match_cursor + 1) % state.match_indexes.len();
            self.apply_shell_reverse_search_match();
        }
    }

    fn refresh_shell_reverse_search_matches(&mut self) {
        let Some(state) = self.shell_command.reverse_search.as_mut() else {
            return;
        };

        let needle = state.query.to_ascii_lowercase();
        state.match_indexes = self
            .shell_command
            .history
            .iter()
            .enumerate()
            .filter(|(_, cmd)| needle.is_empty() || cmd.to_ascii_lowercase().contains(&needle))
            .map(|(idx, _)| idx)
            .rev()
            .collect();

        if state.match_indexes.is_empty() {
            state.match_cursor = 0;
            self.shell_command.buffer = state.draft_buffer.clone();
            self.shell_command.cursor = self.shell_command.buffer.len();
            return;
        }

        state.match_cursor = state
            .match_cursor
            .min(state.match_indexes.len().saturating_sub(1));
        self.apply_shell_reverse_search_match();
    }

    fn apply_shell_reverse_search_match(&mut self) {
        let Some(state) = self.shell_command.reverse_search.as_ref() else {
            return;
        };
        let Some(idx) = state.match_indexes.get(state.match_cursor).copied() else {
            return;
        };
        let Some(command) = self.shell_command.history.get(idx) else {
            return;
        };
        self.shell_command.buffer = command.clone();
        self.shell_command.cursor = self.shell_command.buffer.len();
    }

    fn accept_shell_reverse_search(&mut self) {
        self.shell_command.reverse_search = None;
        self.shell_command.history_nav = None;
    }

    fn cancel_shell_reverse_search(&mut self) {
        if let Some(state) = self.shell_command.reverse_search.take() {
            self.shell_command.buffer = state.draft_buffer;
            self.shell_command.cursor = self.shell_command.buffer.len();
        }
    }

    fn navigate_shell_history_previous(&mut self) {
        if self.shell_command.history.is_empty() {
            return;
        }

        self.shell_command.reverse_search = None;

        let next = match self.shell_command.history_nav {
            Some(idx) => idx.saturating_sub(1),
            None => {
                self.shell_command.history_draft = self.shell_command.buffer.clone();
                self.shell_command.history.len().saturating_sub(1)
            }
        };

        self.shell_command.history_nav = Some(next);
        if let Some(cmd) = self.shell_command.history.get(next) {
            self.shell_command.buffer = cmd.clone();
            self.shell_command.cursor = self.shell_command.buffer.len();
        }
    }

    fn navigate_shell_history_next(&mut self) {
        let Some(current) = self.shell_command.history_nav else {
            return;
        };

        let next = current.saturating_add(1);
        if next >= self.shell_command.history.len() {
            self.shell_command.history_nav = None;
            self.shell_command.buffer = self.shell_command.history_draft.clone();
            self.shell_command.cursor = self.shell_command.buffer.len();
            return;
        }

        self.shell_command.history_nav = Some(next);
        if let Some(cmd) = self.shell_command.history.get(next) {
            self.shell_command.buffer = cmd.clone();
            self.shell_command.cursor = self.shell_command.buffer.len();
        }
    }

    fn execute_shell_command(&mut self) {
        let command = self.shell_command.buffer.trim().to_owned();
        if command.is_empty() {
            return;
        }

        self.shell_command.reverse_search = None;
        self.shell_command.history_nav = None;
        self.shell_command.history_draft.clear();
        self.push_shell_history(command.clone());

        self.shell_command.active_command = Some(command.clone());
        self.shell_command.output_lines.clear();
        self.shell_command.output_tail.clear();
        self.shell_command.output_scroll = 0;
        self.shell_command.output_follow = true;
        self.shell_command.finished = None;

        match spawn_shell_command(&command) {
            Ok(running) => {
                self.shell_command.running = Some(running);
                self.shell_command.cursor = self.shell_command.buffer.len();
            }
            Err(err) => {
                self.shell_command
                    .output_lines
                    .push(format!("hunkr: failed to launch shell command: {err:#}"));
                self.shell_command.output_follow = true;
                self.snap_shell_output_to_bottom();
            }
        }
    }

    fn append_shell_output_chunk(&mut self, chunk: &str) {
        let normalized = chunk.replace("\r\n", "\n").replace('\r', "\n");
        self.shell_command.output_tail.push_str(&normalized);

        while let Some(newline_idx) = self.shell_command.output_tail.find('\n') {
            let line = self.shell_command.output_tail[..newline_idx].to_owned();
            self.shell_command.output_lines.push(line);
            let rest = self.shell_command.output_tail[newline_idx + 1..].to_owned();
            self.shell_command.output_tail = rest;
        }

        if self.shell_command.output_follow {
            self.snap_shell_output_to_bottom();
        }
    }

    fn finalize_shell_command(&mut self) {
        let Some(mut running) = self.shell_command.running.take() else {
            return;
        };

        if let Some(handle) = running.stdout_reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = running.stderr_reader.take() {
            let _ = handle.join();
        }

        let Some(status) = running.exit_status.or_else(|| running.child.wait().ok()) else {
            self.shell_command
                .output_lines
                .push("hunkr: failed to collect process exit status".to_owned());
            return;
        };
        self.shell_command.finished = Some(ShellCommandResult {
            exit_status: status,
        });
        self.reconcile_repository_after_shell_command();

        if self.shell_command.output_follow {
            self.snap_shell_output_to_bottom();
        }
    }

    fn stop_shell_process(&mut self) {
        let Some(mut running) = self.shell_command.running.take() else {
            return;
        };

        let _ = running.child.kill();
        let _ = running.child.wait();

        if let Some(handle) = running.stdout_reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = running.stderr_reader.take() {
            let _ = handle.join();
        }
    }

    fn reset_shell_command_editor(&mut self) {
        self.shell_command.buffer.clear();
        self.shell_command.cursor = 0;
        self.shell_command.history_nav = None;
        self.shell_command.history_draft.clear();
        self.shell_command.reverse_search = None;
        self.shell_command.active_command = None;
        self.shell_command.output_lines.clear();
        self.shell_command.output_tail.clear();
        self.shell_command.output_scroll = 0;
        self.shell_command.output_follow = true;
        self.shell_command.finished = None;
    }

    fn push_shell_history(&mut self, command: String) {
        self.shell_command.history.push_back(command);
        while self.shell_command.history.len() > SHELL_HISTORY_LIMIT {
            self.shell_command.history.pop_front();
        }

        let snapshot = self
            .shell_command
            .history
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        if let Err(err) = self.store.save_shell_history(&snapshot) {
            self.runtime.status = format!("failed to save shell history: {err:#}");
        }
    }

    fn snap_shell_output_to_bottom(&mut self) {
        let max_scroll = self.shell_output_max_scroll();
        self.shell_command.output_scroll = max_scroll;
        self.shell_command.output_follow = true;
    }

    fn reconcile_repository_after_shell_command(&mut self) {
        let previous_branch = self.git.branch_name().to_owned();
        let reopened = match GitService::open_at(self.git.root()) {
            Ok(git) => git,
            Err(err) => {
                self.runtime.status = format!("shell sync failed to reopen repository: {err:#}");
                return;
            }
        };

        let next_branch = reopened.branch_name().to_owned();
        self.git = reopened;
        self.comments = match CommentStore::new(self.store.root_dir(), &next_branch) {
            Ok(store) => store,
            Err(err) => {
                self.runtime.status = format!(
                    "shell sync failed to reload comments for branch {next_branch}: {err:#}"
                );
                return;
            }
        };

        if let Err(err) = self.reload_commits(true) {
            self.runtime.status = format!("shell sync failed to refresh UI: {err:#}");
            return;
        }

        let now = Instant::now();
        self.runtime.last_refresh = now;
        self.runtime.last_relative_time_redraw = now;
        self.runtime.needs_redraw = true;

        if previous_branch != next_branch {
            self.runtime.status =
                format!("repository switched: {previous_branch} -> {next_branch}");
        }
    }
}

fn spawn_shell_command(command: &str) -> anyhow::Result<RunningShellCommand> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned());

    let mut child = Command::new(&shell)
        .arg("-lc")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn `{shell} -lc <command>`"))?;

    let stdout = child
        .stdout
        .take()
        .context("failed to capture child stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("failed to capture child stderr")?;

    let (tx, rx) = mpsc::channel::<String>();
    let stdout_reader = Some(spawn_shell_pipe_reader(stdout, tx.clone()));
    let stderr_reader = Some(spawn_shell_pipe_reader(stderr, tx));

    Ok(RunningShellCommand {
        child,
        stream_rx: rx,
        stdout_reader,
        stderr_reader,
        exit_status: None,
    })
}

fn spawn_shell_pipe_reader<R>(mut reader: R, tx: mpsc::Sender<String>) -> JoinHandle<()>
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
