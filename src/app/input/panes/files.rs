use super::super::super::*;

impl App {
    pub(in crate::app) fn handle_files_key(&mut self, key: KeyEvent) {
        if let Some(target) = absolute_nav_target(key.code) {
            match target {
                AbsoluteNavTarget::Start => self.select_first_file(),
                AbsoluteNavTarget::End => self.select_last_file(),
            }
            return;
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_file_cursor(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_file_cursor(-1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_files(0.5)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.page_files(-0.5)
            }
            KeyCode::PageDown => self.page_files(1.0),
            KeyCode::PageUp => self.page_files(-1.0),
            KeyCode::Char('/') if key.modifiers == KeyModifiers::NONE => {
                self.ui.preferences.input_mode = InputMode::ListSearch(FocusPane::Files);
                self.ui.search.file_cursor = self.ui.search.file_query.len();
                self.runtime.status = format!("/{}", self.ui.search.file_query);
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.set_focus(FocusPane::Diff),
            _ => {}
        }
    }
}
