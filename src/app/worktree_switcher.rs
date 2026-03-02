//! Worktree switcher modal state and input handling.
use crate::app::*;

impl App {
    pub(super) fn open_worktree_switcher(&mut self) {
        self.ui.worktree_switch.search_active = false;
        self.ui.worktree_switch.query.clear();
        match self.refresh_worktree_switch_entries() {
            Ok(()) => {
                self.ui.preferences.input_mode = InputMode::WorktreeSwitch;
                self.runtime.status = format!(
                    "Worktrees: {}/{} shown",
                    self.visible_worktree_indices().len(),
                    self.ui.worktree_switch.entries.len()
                );
            }
            Err(err) => {
                self.runtime.status = format!("failed to load worktrees: {err:#}");
            }
        }
    }

    pub(super) fn handle_worktree_switch_input(&mut self, key: KeyEvent) {
        if key.modifiers == KeyModifiers::NONE && matches!(key.code, KeyCode::Char('q')) {
            self.close_worktree_switcher();
            return;
        }

        if self.ui.worktree_switch.search_active {
            self.handle_worktree_search_input(key);
            return;
        }

        if let Some(target) = absolute_nav_target(key.code) {
            match target {
                AbsoluteNavTarget::Start => self.select_first_worktree(),
                AbsoluteNavTarget::End => self.select_last_worktree(),
            }
            return;
        }

        match key.code {
            KeyCode::Esc => self.close_worktree_switcher(),
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
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.ui.worktree_switch.search_active = true;
                self.runtime.status = format!("/{}", self.ui.worktree_switch.query);
            }
            KeyCode::Char('r') if key.modifiers == KeyModifiers::NONE => {
                if let Err(err) = self.refresh_worktree_switch_entries() {
                    self.runtime.status = format!("failed to refresh worktrees: {err:#}");
                } else {
                    self.runtime.status = format!(
                        "Worktrees refreshed: {}/{} shown",
                        self.visible_worktree_indices().len(),
                        self.ui.worktree_switch.entries.len()
                    );
                }
            }
            _ => {}
        }
    }

    pub(super) fn visible_worktree_indices(&self) -> Vec<usize> {
        let query = self.ui.worktree_switch.query.trim();
        self.ui
            .worktree_switch
            .entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| worktree_matches_query(entry, query).then_some(idx))
            .collect()
    }

    pub(super) fn selected_worktree_full_index(&self) -> Option<usize> {
        let visible = self.visible_worktree_indices();
        let selected_visible_idx = self.ui.worktree_switch.list_state.selected()?;
        visible.get(selected_visible_idx).copied()
    }

    fn switch_to_selected_worktree(&mut self) {
        let Some(idx) = self.selected_worktree_full_index() else {
            self.runtime.status = "No worktree selected".to_owned();
            return;
        };
        let Some(entry) = self.ui.worktree_switch.entries.get(idx) else {
            self.runtime.status = "No worktree selected".to_owned();
            return;
        };

        let target = entry.path.clone();
        if target == self.deps.git.root() {
            self.ui.preferences.input_mode = InputMode::Normal;
            self.runtime.status = format!("Already on worktree {}", short_path_label(&target));
            return;
        }

        let previous_root = self.deps.git.root().to_path_buf();
        let previous_branch = self.deps.git.branch_name().to_owned();
        if let Err(err) = self.switch_repository_context(&target) {
            self.runtime.status = format!("worktree switch failed: {err:#}");
            return;
        }

        self.ui.preferences.input_mode = InputMode::Normal;
        let next_root = self.deps.git.root().to_path_buf();
        let next_branch = self.deps.git.branch_name().to_owned();
        self.runtime.status = format!(
            "worktree switched: {} -> {} ({previous_branch} -> {next_branch})",
            short_path_label(&previous_root),
            short_path_label(&next_root)
        );
    }

    fn refresh_worktree_switch_entries(&mut self) -> anyhow::Result<()> {
        let preferred_path = self
            .selected_worktree_full_index()
            .and_then(|idx| self.ui.worktree_switch.entries.get(idx))
            .map(|entry| entry.path.clone());
        let fallback_visible_idx = self.ui.worktree_switch.list_state.selected();
        self.ui.worktree_switch.entries = self.deps.git.list_worktrees()?;
        self.sync_worktree_cursor(preferred_path.as_deref(), fallback_visible_idx);
        Ok(())
    }

    fn move_worktree_cursor(&mut self, delta: isize) {
        let visible = self.visible_worktree_indices();
        if visible.is_empty() {
            self.ui.worktree_switch.list_state.select(None);
            return;
        }
        let len = visible.len() as isize;
        let current = self.ui.worktree_switch.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        self.ui.worktree_switch.list_state.select(Some(next));
    }

    fn page_worktree_cursor(&mut self, multiplier: f32) {
        let step = page_step(self.ui.worktree_switch.viewport_rows as u16 + 2, multiplier);
        self.move_worktree_cursor(step);
    }

    fn select_first_worktree(&mut self) {
        if self.visible_worktree_indices().is_empty() {
            self.ui.worktree_switch.list_state.select(None);
            return;
        }
        self.ui.worktree_switch.list_state.select(Some(0));
    }

    fn select_last_worktree(&mut self) {
        let visible = self.visible_worktree_indices();
        if visible.is_empty() {
            self.ui.worktree_switch.list_state.select(None);
            return;
        }
        self.ui
            .worktree_switch
            .list_state
            .select(Some(visible.len().saturating_sub(1)));
    }

    fn sync_worktree_cursor(&mut self, preferred_path: Option<&Path>, fallback: Option<usize>) {
        let visible = self.visible_worktree_indices();
        if visible.is_empty() {
            self.ui.worktree_switch.list_state.select(None);
            return;
        }

        let selected = preferred_path
            .and_then(|path| {
                visible.iter().position(|idx| {
                    self.ui
                        .worktree_switch
                        .entries
                        .get(*idx)
                        .is_some_and(|entry| entry.path == path)
                })
            })
            .or_else(|| fallback.map(|idx| idx.min(visible.len().saturating_sub(1))))
            .unwrap_or(0);
        self.ui.worktree_switch.list_state.select(Some(selected));
    }

    fn handle_worktree_search_input(&mut self, key: KeyEvent) {
        let fallback_visible_idx = self.ui.worktree_switch.list_state.selected();
        if key.modifiers == KeyModifiers::NONE && matches!(key.code, KeyCode::Char('q')) {
            self.close_worktree_switcher();
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.ui.worktree_switch.search_active = false;
                self.ui.worktree_switch.query.clear();
                self.sync_worktree_cursor(None, fallback_visible_idx);
                self.runtime.status = "Worktree search cleared".to_owned();
            }
            KeyCode::Enter => {
                self.ui.worktree_switch.search_active = false;
                let query = self.ui.worktree_switch.query.trim();
                self.runtime.status = if query.is_empty() {
                    "Worktree search off".to_owned()
                } else {
                    format!("Worktree filter: /{query}")
                };
            }
            KeyCode::Backspace => {
                self.ui.worktree_switch.query.pop();
                self.sync_worktree_cursor(None, fallback_visible_idx);
                self.runtime.status = format!("/{}", self.ui.worktree_switch.query);
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                self.ui.worktree_switch.query.push(c);
                self.sync_worktree_cursor(None, fallback_visible_idx);
                self.runtime.status = format!("/{}", self.ui.worktree_switch.query);
            }
            _ => {}
        }
    }

    fn close_worktree_switcher(&mut self) {
        self.ui.worktree_switch.search_active = false;
        self.ui.preferences.input_mode = InputMode::Normal;
        self.runtime.status = "Worktree switcher closed".to_owned();
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
