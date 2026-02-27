//! Worktree switcher modal state and input handling.
use super::*;

impl App {
    pub(super) fn open_worktree_switcher(&mut self) {
        match self.refresh_worktree_switch_entries() {
            Ok(()) => {
                self.preferences.input_mode = InputMode::WorktreeSwitch;
                self.runtime.status = format!(
                    "Worktrees: {}/{} shown",
                    self.visible_worktree_indices().len(),
                    self.worktree_switch.entries.len()
                );
            }
            Err(err) => {
                self.runtime.status = format!("failed to load worktrees: {err:#}");
            }
        }
    }

    pub(super) fn handle_worktree_switch_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.preferences.input_mode = InputMode::Normal;
                self.runtime.status = "Worktree switcher closed".to_owned();
            }
            KeyCode::Enter => self.switch_to_selected_worktree(),
            KeyCode::Down | KeyCode::Char('j') => self.move_worktree_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_worktree_cursor(-1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_worktree_cursor(0.5)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_worktree_cursor(-0.5)
            }
            KeyCode::PageDown => self.page_worktree_cursor(1.0),
            KeyCode::PageUp => self.page_worktree_cursor(-1.0),
            KeyCode::Char('g') => self.select_first_worktree(),
            KeyCode::Char('G') => self.select_last_worktree(),
            KeyCode::Backspace => {
                self.worktree_switch.query.pop();
                self.sync_worktree_cursor(None, self.worktree_switch.list_state.selected());
                self.runtime.status = format!("/{}", self.worktree_switch.query);
            }
            KeyCode::Char('r') if key.modifiers == KeyModifiers::NONE => {
                if let Err(err) = self.refresh_worktree_switch_entries() {
                    self.runtime.status = format!("failed to refresh worktrees: {err:#}");
                } else {
                    self.runtime.status = format!(
                        "Worktrees refreshed: {}/{} shown",
                        self.visible_worktree_indices().len(),
                        self.worktree_switch.entries.len()
                    );
                }
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                self.worktree_switch.query.push(c);
                self.sync_worktree_cursor(None, self.worktree_switch.list_state.selected());
                self.runtime.status = format!("/{}", self.worktree_switch.query);
            }
            _ => {}
        }
    }

    pub(super) fn visible_worktree_indices(&self) -> Vec<usize> {
        let query = self.worktree_switch.query.trim();
        self.worktree_switch
            .entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| worktree_matches_query(entry, query).then_some(idx))
            .collect()
    }

    pub(super) fn selected_worktree_full_index(&self) -> Option<usize> {
        let visible = self.visible_worktree_indices();
        let selected_visible_idx = self.worktree_switch.list_state.selected()?;
        visible.get(selected_visible_idx).copied()
    }

    fn switch_to_selected_worktree(&mut self) {
        let Some(idx) = self.selected_worktree_full_index() else {
            self.runtime.status = "No worktree selected".to_owned();
            return;
        };
        let Some(entry) = self.worktree_switch.entries.get(idx) else {
            self.runtime.status = "No worktree selected".to_owned();
            return;
        };

        let target = entry.path.clone();
        if target == self.git.root() {
            self.preferences.input_mode = InputMode::Normal;
            self.runtime.status = format!("Already on worktree {}", short_path_label(&target));
            return;
        }

        let previous_root = self.git.root().to_path_buf();
        let previous_branch = self.git.branch_name().to_owned();
        if let Err(err) = self.switch_repository_context(&target) {
            self.runtime.status = format!("worktree switch failed: {err:#}");
            return;
        }

        self.preferences.input_mode = InputMode::Normal;
        let next_root = self.git.root().to_path_buf();
        let next_branch = self.git.branch_name().to_owned();
        self.runtime.status = format!(
            "worktree switched: {} -> {} ({previous_branch} -> {next_branch})",
            short_path_label(&previous_root),
            short_path_label(&next_root)
        );
    }

    fn refresh_worktree_switch_entries(&mut self) -> anyhow::Result<()> {
        let preferred_path = self
            .selected_worktree_full_index()
            .and_then(|idx| self.worktree_switch.entries.get(idx))
            .map(|entry| entry.path.clone());
        let fallback_visible_idx = self.worktree_switch.list_state.selected();
        self.worktree_switch.entries = self.git.list_worktrees()?;
        self.sync_worktree_cursor(preferred_path.as_deref(), fallback_visible_idx);
        Ok(())
    }

    fn move_worktree_cursor(&mut self, delta: isize) {
        let visible = self.visible_worktree_indices();
        if visible.is_empty() {
            self.worktree_switch.list_state.select(None);
            return;
        }
        let len = visible.len() as isize;
        let current = self.worktree_switch.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        self.worktree_switch.list_state.select(Some(next));
    }

    fn page_worktree_cursor(&mut self, multiplier: f32) {
        let step = page_step(self.worktree_switch.viewport_rows as u16 + 2, multiplier);
        self.move_worktree_cursor(step);
    }

    fn select_first_worktree(&mut self) {
        if self.visible_worktree_indices().is_empty() {
            self.worktree_switch.list_state.select(None);
            return;
        }
        self.worktree_switch.list_state.select(Some(0));
    }

    fn select_last_worktree(&mut self) {
        let visible = self.visible_worktree_indices();
        if visible.is_empty() {
            self.worktree_switch.list_state.select(None);
            return;
        }
        self.worktree_switch
            .list_state
            .select(Some(visible.len().saturating_sub(1)));
    }

    fn sync_worktree_cursor(&mut self, preferred_path: Option<&Path>, fallback: Option<usize>) {
        let visible = self.visible_worktree_indices();
        if visible.is_empty() {
            self.worktree_switch.list_state.select(None);
            return;
        }

        let selected = preferred_path
            .and_then(|path| {
                visible.iter().position(|idx| {
                    self.worktree_switch
                        .entries
                        .get(*idx)
                        .is_some_and(|entry| entry.path == path)
                })
            })
            .or_else(|| fallback.map(|idx| idx.min(visible.len().saturating_sub(1))))
            .unwrap_or(0);
        self.worktree_switch.list_state.select(Some(selected));
    }
}

fn worktree_matches_query(entry: &WorktreeInfo, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let query = query.to_ascii_lowercase();
    let path = entry.path.to_string_lossy().to_ascii_lowercase();
    let branch = entry
        .branch
        .as_deref()
        .unwrap_or("detached")
        .to_ascii_lowercase();
    let head = entry.head.to_ascii_lowercase();
    path.contains(&query) || branch.contains(&query) || head.contains(&query)
}

pub(super) fn short_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}
