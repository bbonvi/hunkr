//! Render pipeline and modal/footer presentation for the lifecycle flow.
use super::*;
use ratatui::widgets::{List, ListItem};

impl App {
    pub(super) fn render_header(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let selected = self.commits.iter().filter(|row| row.selected).count();
        let (unreviewed, reviewed, issue_found, resolved) = self.status_counts();
        let focus = match self.preferences.focused {
            FocusPane::Files => "FILES",
            FocusPane::Commits => "COMMITS",
            FocusPane::Diff => "DIFF",
        };
        let headline = Line::from(vec![
            Span::styled(
                app_title_label(self.preferences.nerd_fonts),
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("branch:{} ", self.git.branch_name()),
                Style::default().fg(theme.text),
            ),
            Span::styled(
                format!("wt:{} ", short_path_label(self.git.root())),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!("focus:{} ", focus),
                Style::default().fg(theme.accent),
            ),
            Span::styled(
                format!("selected:{} ", selected),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!(
                    "U:{} R:{} I:{} Z:{} ",
                    unreviewed, reviewed, issue_found, resolved
                ),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!("theme:{} ", self.preferences.theme_mode.label()),
                Style::default().fg(theme.dimmed),
            ),
            Span::styled(
                format!(
                    "nf:{} ",
                    if self.preferences.nerd_fonts {
                        "on"
                    } else {
                        "off"
                    }
                ),
                Style::default().fg(theme.dimmed),
            ),
        ]);

        let header = Paragraph::new(headline).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(header, rect);
    }
    pub(super) fn render_files(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let files_search_mode = matches!(
            self.preferences.input_mode,
            InputMode::ListSearch(FocusPane::Files)
        );
        let file_query = self.search.file_query.trim();
        let files_search_display = if !file_query.is_empty() {
            format!("/{file_query}")
        } else if files_search_mode {
            "/".to_owned()
        } else {
            "off".to_owned()
        };
        let visible_indices = self.visible_file_indices();
        let visible_rows: Vec<TreeRow> = visible_indices
            .iter()
            .filter_map(|idx| self.file_rows.get(*idx).cloned())
            .collect();
        ListPaneRenderer::new(theme, self.preferences.focused, self.preferences.nerd_fonts)
            .render_files(
                frame,
                rect,
                FilePaneModel {
                    file_rows: &visible_rows,
                    changed_files: self.aggregate.files.len(),
                    shown_files: visible_rows.iter().filter(|row| row.selectable).count(),
                    search_display: &files_search_display,
                    search_enabled: files_search_mode || !file_query.is_empty(),
                    file_list_state: &mut self.file_ui.list_state,
                },
            );
    }

    pub(super) fn render_commits(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let commits_search_mode = matches!(
            self.preferences.input_mode,
            InputMode::ListSearch(FocusPane::Commits)
        );
        let commit_query = self.search.commit_query.trim();
        let commits_search_display = if !commit_query.is_empty() {
            format!("/{commit_query}")
        } else if commits_search_mode {
            "/".to_owned()
        } else {
            "off".to_owned()
        };
        let visible_indices = self.visible_commit_indices();
        let visible_rows: Vec<CommitRow> = visible_indices
            .iter()
            .filter_map(|idx| self.commits.get(*idx).cloned())
            .collect();
        let selected_total = self.commits.iter().filter(|row| row.selected).count();
        ListPaneRenderer::new(theme, self.preferences.focused, self.preferences.nerd_fonts)
            .render_commits(
                frame,
                rect,
                CommitPaneModel {
                    commits: &visible_rows,
                    status_counts: self.status_counts(),
                    selected_total,
                    shown_commits: visible_rows.len(),
                    total_commits: self.commits.len(),
                    status_filter: self.commit_ui.status_filter.label(),
                    search_display: &commits_search_display,
                    search_enabled: commits_search_mode || !commit_query.is_empty(),
                    commit_list_state: &mut self.commit_ui.list_state,
                },
            );
    }

    pub(super) fn render_diff(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let selected_lines = self
            .diff_selected_range()
            .map(|(start, end)| end.saturating_sub(start) + 1)
            .unwrap_or(0);
        let viewport_rows = rect.height.saturating_sub(2).max(1) as usize;
        let sticky_banner_indexes =
            self.sticky_banner_indexes_for_scroll(self.diff_position.scroll, viewport_rows);
        let sticky_rows = sticky_banner_indexes
            .len()
            .min(viewport_rows.saturating_sub(1));
        let body_rows = viewport_rows.saturating_sub(sticky_rows);
        let mut visible_indexes = BTreeSet::new();
        for idx in sticky_banner_indexes.iter().take(sticky_rows) {
            visible_indexes.insert(*idx);
        }
        for row in 0..body_rows {
            let idx = self.diff_position.scroll.saturating_add(row);
            if idx >= self.rendered_diff.len() {
                break;
            }
            visible_indexes.insert(idx);
        }
        let mut line_overrides = HashMap::new();
        for idx in visible_indexes {
            if let Some(line) = self.highlight_visible_diff_line(idx, theme) {
                line_overrides.insert(idx, line);
            }
        }
        let empty_state_message = diff_empty_state_message(
            !self.rendered_diff.is_empty(),
            self.aggregate.files.len(),
            self.diff_cache.file_ranges.len(),
            &self.search.file_query,
        );
        let selected_file = self
            .diff_cache
            .selected_file
            .as_deref()
            .filter(|path| self.diff_cache.file_range_by_path.contains_key(*path));
        let title = DiffPaneTitle {
            selected_file,
            selected_file_progress: self.selected_file_progress(),
            nerd_fonts: self.preferences.nerd_fonts,
            nerd_font_theme: &self.preferences.nerd_font_theme,
            selected_lines,
        };
        let body = DiffPaneBody {
            rendered_diff: &self.rendered_diff,
            diff_position: self.diff_position,
            visual_range: self.diff_selected_range(),
            sticky_banner_indexes: &sticky_banner_indexes,
            empty_state_message: empty_state_message.as_deref(),
            line_overrides: &line_overrides,
        };
        DiffPaneRenderer::new(theme, self.preferences.focused).render(frame, rect, title, body);
    }

    fn highlight_visible_diff_line(
        &mut self,
        idx: usize,
        theme: &UiTheme,
    ) -> Option<Line<'static>> {
        let rendered = self.rendered_diff.get(idx)?;
        let anchor = rendered.anchor.as_ref()?;
        if is_commit_anchor(anchor) {
            return None;
        }

        let mut chars = rendered.raw_text.chars();
        let prefix = chars.next()?;
        if !matches!(prefix, '+' | '-' | ' ') {
            return None;
        }
        if rendered.line.spans.len() < 4 {
            return None;
        }

        let code_text = chars.as_str();
        let mut spans = vec![
            rendered.line.spans[0].clone(),
            rendered.line.spans[1].clone(),
            rendered.line.spans[2].clone(),
        ];
        let mut highlighted = self.diff_cache.highlighter.highlight_single_line(
            self.preferences.theme_mode,
            &anchor.file_path,
            code_text,
        );
        if highlighted.is_empty() {
            highlighted.push(Span::raw(code_text.to_owned()));
        }

        let bg = match prefix {
            '+' => Some(theme.diff_add_bg),
            '-' => Some(theme.diff_remove_bg),
            _ => None,
        };
        if let Some(bg_color) = bg {
            for span in &mut highlighted {
                span.style = span.style.bg(bg_color);
            }
        }
        spans.extend(highlighted);
        Some(Line::from(spans))
    }

    pub(super) fn render_footer(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
    ) {
        let commit_visual_active = self.commit_ui.visual_anchor.is_some();
        let diff_visual_active = self.diff_ui.visual_selection.is_some();
        let mode = footer_mode_label(
            self.preferences.input_mode,
            commit_visual_active,
            diff_visual_active,
        );

        let pane_line = match self.preferences.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" save ", Style::default().fg(theme.muted)),
                key_chip("Alt+Enter", theme),
                Span::styled(" newline ", Style::default().fg(theme.muted)),
                key_chip("mouse", theme),
                Span::styled(" cursor/select ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" cancel comment", Style::default().fg(theme.muted)),
            ]),
            InputMode::ShellCommand => {
                if self.shell_command.running.is_some() {
                    Line::from(vec![
                        key_chip("j/k", theme),
                        Span::styled(" move ", Style::default().fg(theme.muted)),
                        key_chip("Ctrl-d/u", theme),
                        Span::styled(" jump ", Style::default().fg(theme.muted)),
                        key_chip("v", theme),
                        Span::styled(" range ", Style::default().fg(theme.muted)),
                        key_chip("y", theme),
                        Span::styled(" copy output ", Style::default().fg(theme.muted)),
                        key_chip("Esc", theme),
                        Span::styled(" interrupt ", Style::default().fg(theme.muted)),
                        key_chip("Backspace", theme),
                        Span::styled(" reset", Style::default().fg(theme.muted)),
                    ])
                } else if self.shell_command.finished.is_some() {
                    Line::from(vec![
                        key_chip("j/k", theme),
                        Span::styled(" move ", Style::default().fg(theme.muted)),
                        key_chip("Ctrl-d/u", theme),
                        Span::styled(" jump ", Style::default().fg(theme.muted)),
                        key_chip("v", theme),
                        Span::styled(" range ", Style::default().fg(theme.muted)),
                        key_chip("y", theme),
                        Span::styled(" copy output ", Style::default().fg(theme.muted)),
                        key_chip("Esc", theme),
                        Span::styled(" close ", Style::default().fg(theme.muted)),
                        key_chip("Backspace", theme),
                        Span::styled(" reset", Style::default().fg(theme.muted)),
                    ])
                } else {
                    Line::from(vec![
                        key_chip("Enter", theme),
                        Span::styled(" run ", Style::default().fg(theme.muted)),
                        key_chip("Ctrl-r", theme),
                        Span::styled(" history search ", Style::default().fg(theme.muted)),
                        key_chip("Up/Down", theme),
                        Span::styled(" history ", Style::default().fg(theme.muted)),
                        key_chip("Esc", theme),
                        Span::styled(" close shell", Style::default().fg(theme.muted)),
                    ])
                }
            }
            InputMode::WorktreeSwitch => Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" switch ", Style::default().fg(theme.muted)),
                key_chip("j/k", theme),
                Span::styled(" move ", Style::default().fg(theme.muted)),
                key_chip("Ctrl-d/u", theme),
                Span::styled(" jump ", Style::default().fg(theme.muted)),
                key_chip("type", theme),
                Span::styled(" filter ", Style::default().fg(theme.muted)),
                key_chip("Backspace", theme),
                Span::styled(" edit filter ", Style::default().fg(theme.muted)),
                key_chip("r", theme),
                Span::styled(" refresh ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" close", Style::default().fg(theme.muted)),
            ]),
            InputMode::DiffSearch => Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" search ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" cancel search", Style::default().fg(theme.muted)),
            ]),
            InputMode::ListSearch(_) => Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" defocus ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" clear ", Style::default().fg(theme.muted)),
                key_chip("Backspace", theme),
                Span::styled(" edit", Style::default().fg(theme.muted)),
            ]),
            InputMode::Normal => match self.preferences.focused {
                FocusPane::Files => Line::from(vec![
                    key_chip("j/k", theme),
                    Span::styled(" move ", Style::default().fg(theme.muted)),
                    key_chip("Ctrl-d/u", theme),
                    Span::styled(" jump ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" filter ", Style::default().fg(theme.muted)),
                    key_chip("Enter", theme),
                    Span::styled(" focus diff", Style::default().fg(theme.muted)),
                ]),
                FocusPane::Commits => Line::from(vec![
                    key_chip("space", theme),
                    Span::styled(" select ", Style::default().fg(theme.muted)),
                    key_chip("u/r/i/s", theme),
                    Span::styled(" current ", Style::default().fg(theme.muted)),
                    key_chip("U/R/I/S", theme),
                    Span::styled(" selected ", Style::default().fg(theme.muted)),
                    key_chip("e", theme),
                    Span::styled(" status filter ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" search", Style::default().fg(theme.muted)),
                ]),
                FocusPane::Diff => Line::from(vec![
                    key_chip("v", theme),
                    Span::styled(" range ", Style::default().fg(theme.muted)),
                    key_chip("m", theme),
                    Span::styled(" comment ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" search ", Style::default().fg(theme.muted)),
                    key_chip("[/]", theme),
                    Span::styled(" hunks ", Style::default().fg(theme.muted)),
                    key_chip("zz/zt/zb", theme),
                    Span::styled(" scroll ", Style::default().fg(theme.muted)),
                    key_chip("e/D", theme),
                    Span::styled(" edit/delete ", Style::default().fg(theme.muted)),
                    key_chip("y", theme),
                    Span::styled(" copy visual ", Style::default().fg(theme.muted)),
                    key_chip("Y", theme),
                    Span::styled(" copy task path ", Style::default().fg(theme.muted)),
                    key_chip("Ctrl-d/u", theme),
                    Span::styled(" jump", Style::default().fg(theme.muted)),
                ]),
            },
        };

        let global_line = Line::from(vec![
            key_chip("1/2/3", theme),
            Span::styled(" panes ", Style::default().fg(theme.dimmed)),
            key_chip("Tab", theme),
            Span::styled(" cycle all ", Style::default().fg(theme.dimmed)),
            key_chip("h/l", theme),
            Span::styled(" prev/next pane ", Style::default().fg(theme.dimmed)),
            key_chip("!", theme),
            Span::styled(" shell ", Style::default().fg(theme.dimmed)),
            key_chip("w", theme),
            Span::styled(" worktrees ", Style::default().fg(theme.dimmed)),
            key_chip("t", theme),
            Span::styled(" theme ", Style::default().fg(theme.dimmed)),
            key_chip("?", theme),
            Span::styled(" help ", Style::default().fg(theme.dimmed)),
            key_chip("q", theme),
            Span::styled(" quit", Style::default().fg(theme.dimmed)),
        ]);

        let mut status = vec![footer_chip(
            mode,
            footer_mode_style(mode, theme),
            Modifier::BOLD,
        )];
        let show_primary_status = !self.runtime.status.is_empty()
            && !matches!(
                self.preferences.input_mode,
                InputMode::DiffSearch | InputMode::ListSearch(_) | InputMode::WorktreeSwitch
            );
        if show_primary_status {
            status.push(Span::raw(" "));
            status.push(Span::styled(
                self.runtime.status.clone(),
                footer_status_style(&self.runtime.status, theme),
            ));
        }

        if let Some(scope) = footer_visual_scope_label(commit_visual_active, diff_visual_active) {
            status.push(footer_separator(theme));
            status.push(footer_chip(
                &format!("{scope} RANGE"),
                Style::default().fg(theme.focus_border).bg(blend_colors(
                    theme.panel_title_bg,
                    theme.visual_bg,
                    156,
                )),
                Modifier::BOLD,
            ));
        }

        match self.preferences.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => {
                let line_count = self.comment_editor.buffer.matches('\n').count() + 1;
                let (line, col) = comment_cursor_line_col(
                    &self.comment_editor.buffer,
                    self.comment_editor.cursor,
                );
                status.push(footer_separator(theme));
                status.push(footer_detail_chip(
                    format!("{} chars", self.comment_editor.buffer.chars().count()),
                    theme,
                ));
                status.push(Span::raw(" "));
                status.push(footer_detail_chip(
                    format!("Ln {line}, Col {col}, {line_count} lines"),
                    theme,
                ));
            }
            InputMode::DiffSearch => {
                let query = if self.search.diff_buffer.is_empty() {
                    "/".to_owned()
                } else {
                    format!("/{}", self.search.diff_buffer)
                };
                status.push(footer_separator(theme));
                status.push(footer_chip(
                    &query,
                    Style::default().fg(theme.accent).bg(blend_colors(
                        theme.panel_title_bg,
                        theme.border,
                        176,
                    )),
                    Modifier::BOLD,
                ));
            }
            InputMode::ListSearch(pane) => {
                let query = match pane {
                    FocusPane::Commits => &self.search.commit_query,
                    FocusPane::Files => &self.search.file_query,
                    FocusPane::Diff => "",
                };
                let query = if query.is_empty() {
                    "/".to_owned()
                } else {
                    format!("/{}", query)
                };
                status.push(footer_separator(theme));
                status.push(footer_chip(
                    &query,
                    Style::default().fg(theme.accent).bg(blend_colors(
                        theme.panel_title_bg,
                        theme.border,
                        176,
                    )),
                    Modifier::BOLD,
                ));
            }
            InputMode::ShellCommand => {
                status.push(footer_separator(theme));
                let command = self
                    .shell_command
                    .active_command
                    .as_deref()
                    .unwrap_or(self.shell_command.buffer.as_str());
                let label = if self.shell_command.running.is_some() {
                    "running"
                } else if self.shell_command.finished.is_some() {
                    "done"
                } else if self.shell_command.reverse_search.is_some() {
                    "search"
                } else {
                    "input"
                };
                status.push(footer_chip(
                    &format!("{label}: {}", truncate(command, 42)),
                    Style::default().fg(theme.accent).bg(blend_colors(
                        theme.panel_title_bg,
                        theme.border,
                        176,
                    )),
                    Modifier::BOLD,
                ));
            }
            InputMode::WorktreeSwitch => {
                let query = if self.worktree_switch.query.is_empty() {
                    "/".to_owned()
                } else {
                    format!("/{}", self.worktree_switch.query)
                };
                status.push(footer_separator(theme));
                status.push(footer_chip(
                    &format!(
                        "{query} {}/{}",
                        self.visible_worktree_indices().len(),
                        self.worktree_switch.entries.len()
                    ),
                    Style::default().fg(theme.accent).bg(blend_colors(
                        theme.panel_title_bg,
                        theme.border,
                        176,
                    )),
                    Modifier::BOLD,
                ));
            }
            InputMode::Normal => {}
        }

        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(1),
                ratatui::layout::Constraint::Length(2),
            ])
            .split(rect);

        let status_widget =
            Paragraph::new(Line::from(status)).style(Style::default().fg(theme.text));
        let hint_widget = Paragraph::new(vec![pane_line, global_line])
            .style(Style::default().fg(theme.dimmed))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme.border)),
            );

        frame.render_widget(status_widget, chunks[0]);
        frame.render_widget(hint_widget, chunks[1]);
    }

    pub(super) fn render_onboarding(&self, frame: &mut Frame<'_>, theme: &UiTheme) {
        let area = centered_rect(54, 36, frame.area());
        frame.render_widget(Clear, area);

        let block = Block::default()
            .title(Span::styled(
                " WELCOME TO HUNKR ",
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(ratatui::layout::Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .style(Style::default().bg(theme.modal_bg))
            .border_style(Style::default().fg(theme.focus_border));
        frame.render_widget(block.clone(), area);
        let inner = block.inner(area);

        let question_line = match self.runtime.onboarding_step {
            Some(OnboardingStep::ConsentProjectDataDir) => Line::from(vec![
                Span::styled("Create ", Style::default().fg(theme.text)),
                Span::styled(
                    ".hunkr/",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ?", Style::default().fg(theme.text)),
            ]),
            Some(OnboardingStep::GitignoreChoice) => Line::from(vec![
                Span::styled("Add ", Style::default().fg(theme.text)),
                Span::styled(
                    ".hunkr",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to ", Style::default().fg(theme.text)),
                Span::styled(
                    ".gitignore",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ?", Style::default().fg(theme.text)),
            ]),
            None => Line::from(Span::styled(
                "Loading workspace...",
                Style::default().fg(theme.text),
            )),
        };

        let mut lines = vec![
            question_line,
            Line::from(""),
            Line::from(vec![
                key_chip("Y", theme),
                Span::styled(" / ", Style::default().fg(theme.dimmed)),
                key_chip("N", theme),
            ]),
        ];
        if !self.runtime.status.is_empty() {
            let note_style = if self.runtime.status.contains("failed")
                || self.runtime.status.contains("Failed")
            {
                Style::default().fg(theme.issue)
            } else {
                Style::default().fg(theme.dimmed)
            };
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                self.runtime.status.clone(),
                note_style,
            )));
        }

        let widget = Paragraph::new(lines)
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().bg(theme.modal_bg));
        frame.render_widget(widget, inner);
    }

    pub(super) fn render_comment_modal(&mut self, frame: &mut Frame<'_>, theme: &UiTheme) {
        let area = centered_rect(56, 48, frame.area());
        frame.render_widget(Clear, area);

        let title = match self.preferences.input_mode {
            InputMode::CommentCreate => " NEW COMMENT ",
            InputMode::CommentEdit(_) => " EDIT COMMENT ",
            InputMode::Normal
            | InputMode::ShellCommand
            | InputMode::WorktreeSwitch
            | InputMode::DiffSearch
            | InputMode::ListSearch(_) => " COMMENT ",
        };
        let shell = Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(ratatui::layout::Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .style(Style::default().bg(theme.modal_bg))
            .border_style(Style::default().fg(theme.focus_border));
        frame.render_widget(shell.clone(), area);
        let inner = shell.inner(area);

        let sections = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(2),
                ratatui::layout::Constraint::Length(6),
                ratatui::layout::Constraint::Min(5),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);

        let mode_badge = match self.preferences.input_mode {
            InputMode::CommentCreate => "create",
            InputMode::CommentEdit(_) => "edit",
            InputMode::Normal
            | InputMode::ShellCommand
            | InputMode::WorktreeSwitch
            | InputMode::DiffSearch
            | InputMode::ListSearch(_) => "idle",
        };
        let status_style = if self.runtime.status.contains("Failed")
            || self.runtime.status.contains("failed")
            || self.runtime.status.contains("empty")
            || self.runtime.status.contains("No ")
        {
            Style::default().fg(theme.issue)
        } else {
            Style::default().fg(theme.muted)
        };
        let header = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("mode:", Style::default().fg(theme.dimmed)),
                Span::styled(
                    format!(" {mode_badge} "),
                    Style::default()
                        .fg(theme.panel_title_fg)
                        .bg(theme.panel_title_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{} chars", self.comment_editor.buffer.chars().count()),
                    Style::default().fg(theme.dimmed),
                ),
            ]),
            Line::from(vec![
                Span::styled("status: ", Style::default().fg(theme.dimmed)),
                Span::styled(self.runtime.status.clone(), status_style),
            ]),
        ])
        .style(Style::default().bg(theme.modal_bg));
        frame.render_widget(header, sections[0]);

        let context_block = Block::default()
            .title(" Context ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(Style::default().bg(theme.modal_bg))
            .border_style(Style::default().fg(theme.border));
        let context_inner = context_block.inner(sections[1]);
        let context_lines =
            self.comment_context_preview_lines(context_inner.height as usize, theme);
        frame.render_widget(context_block, sections[1]);
        frame.render_widget(
            Paragraph::new(context_lines).style(Style::default().fg(theme.text)),
            context_inner,
        );

        let editor_block = Block::default()
            .title(" Comment ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(Style::default().bg(theme.modal_editor_bg))
            .border_style(Style::default().fg(theme.border));
        let editor_inner = editor_block.inner(sections[2]);
        let modal_view = comment_modal_lines(
            &self.comment_editor.buffer,
            self.comment_editor.cursor,
            self.comment_editor.selection,
            editor_inner.height.saturating_sub(1) as usize,
            theme,
        );
        let CommentModalView {
            lines,
            line_ranges,
            view_start,
            text_offset,
        } = modal_view;
        self.comment_editor.rect = Some(editor_inner);
        self.comment_editor.line_ranges = line_ranges;
        self.comment_editor.view_start = view_start;
        self.comment_editor.text_offset = text_offset;
        frame.render_widget(editor_block, sections[2]);
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(theme.text)),
            editor_inner,
        );

        let footer = Paragraph::new(Line::from(vec![
            key_chip("Enter", theme),
            Span::styled(" save  ", Style::default().fg(theme.muted)),
            key_chip("Esc", theme),
            Span::styled(" cancel  ", Style::default().fg(theme.muted)),
            key_chip("Alt+Enter", theme),
            Span::styled(" newline  ", Style::default().fg(theme.muted)),
            key_chip("Mouse", theme),
            Span::styled(" cursor/select", Style::default().fg(theme.muted)),
        ]))
        .style(Style::default().bg(theme.modal_bg));
        frame.render_widget(footer, sections[3]);
    }

    pub(super) fn render_shell_command_modal(&mut self, frame: &mut Frame<'_>, theme: &UiTheme) {
        let area = centered_rect(68, 44, frame.area());
        frame.render_widget(Clear, area);

        let command_failed = self.shell_command.finished.as_ref().is_some_and(|result| {
            result
                .exit_status
                .code()
                .map(|code| code != 0)
                .unwrap_or(true)
        });
        let border_color = if command_failed {
            theme.unreviewed
        } else if self.shell_command.finished.is_some() {
            blend_colors(theme.border, theme.reviewed, 116)
        } else {
            theme.focus_border
        };

        let shell = Block::default()
            .title(Span::styled(
                " SHELL COMMAND ",
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_alignment(ratatui::layout::Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .style(Style::default().bg(theme.modal_bg))
            .border_style(Style::default().fg(border_color));
        frame.render_widget(shell.clone(), area);
        let inner = shell.inner(area);

        let sections = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(2),
                ratatui::layout::Constraint::Length(4),
                ratatui::layout::Constraint::Min(6),
                ratatui::layout::Constraint::Length(1),
            ])
            .split(inner);

        let mode_badge = if self.shell_command.running.is_some() {
            "running"
        } else if self.shell_command.finished.is_some() {
            "done"
        } else if self.shell_command.reverse_search.is_some() {
            "search"
        } else {
            "input"
        };
        let status_text = if let Some(result) = self.shell_command.finished.as_ref() {
            match result.exit_status.code() {
                Some(code) => format!("exit code {code}"),
                None => "process terminated by signal".to_owned(),
            }
        } else if self.shell_command.running.is_some() {
            "streaming output…".to_owned()
        } else {
            "ready".to_owned()
        };
        let status_style = if command_failed {
            Style::default().fg(theme.unreviewed)
        } else {
            Style::default().fg(theme.muted)
        };

        let header = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("mode:", Style::default().fg(theme.dimmed)),
                Span::styled(
                    format!(" {mode_badge} "),
                    Style::default()
                        .fg(theme.panel_title_fg)
                        .bg(theme.panel_title_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{} commands", self.shell_command.history.len()),
                    Style::default().fg(theme.dimmed),
                ),
            ]),
            Line::from(vec![
                Span::styled("status: ", Style::default().fg(theme.dimmed)),
                Span::styled(status_text, status_style),
                Span::raw("  "),
                Span::styled(
                    "non-interactive",
                    Style::default()
                        .fg(theme.dimmed)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]),
        ])
        .style(Style::default().bg(theme.modal_bg));
        frame.render_widget(header, sections[0]);

        let command_block = Block::default()
            .title(" Command ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(Style::default().bg(theme.modal_bg))
            .border_style(Style::default().fg(theme.border));
        let command_inner = command_block.inner(sections[1]);
        frame.render_widget(command_block, sections[1]);

        let mut command_lines = Vec::new();
        let command_editable =
            self.shell_command.running.is_none() && self.shell_command.finished.is_none();
        if command_editable && let Some(search) = self.shell_command.reverse_search.as_ref() {
            let match_count = search.match_indexes.len();
            let marker = if match_count == 0 {
                "no match".to_owned()
            } else {
                format!("{}/{}", search.match_cursor.saturating_add(1), match_count)
            };
            command_lines.push(Line::from(vec![
                Span::styled("reverse-search: ", Style::default().fg(theme.dimmed)),
                sanitized_span(&search.query, Some(Style::default().fg(theme.accent))),
                Span::raw(" "),
                Span::styled(format!("({marker})"), Style::default().fg(theme.dimmed)),
            ]));
        }
        if command_editable {
            command_lines.push(shell_prompt_line(
                &self.shell_command.buffer,
                self.shell_command.cursor,
                theme,
            ));
        } else {
            command_lines.push(Line::from(vec![
                Span::styled("$ ", Style::default().fg(theme.dimmed)),
                sanitized_span(&self.shell_command.buffer, None),
            ]));
        }
        frame.render_widget(
            Paragraph::new(command_lines).style(Style::default().fg(theme.text)),
            command_inner,
        );

        let output_block = Block::default()
            .title(" Output ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(Style::default().bg(theme.modal_editor_bg))
            .border_style(Style::default().fg(theme.border));
        let output_inner = output_block.inner(sections[2]);
        self.shell_command.output_rect = Some(output_inner);
        self.shell_command.output_viewport = output_inner.height as usize;
        frame.render_widget(output_block, sections[2]);

        let output_rows = self.shell_output_rows();

        if self.shell_command.output_follow {
            let max_scroll = self.shell_output_max_scroll();
            self.set_shell_output_scroll(max_scroll);
        }
        self.sync_shell_output_visual_bounds();
        let viewport_rows = output_inner.height.max(1) as usize;

        if output_rows.is_empty() {
            frame.render_widget(
                Paragraph::new(vec![Line::from("Run any shell command and press Enter.")])
                    .style(Style::default().fg(theme.muted)),
                output_inner,
            );
        } else {
            self.shell_command.output_cursor = self
                .shell_command
                .output_cursor
                .min(output_rows.len().saturating_sub(1));
            let max_scroll = self.shell_output_max_scroll();
            self.shell_command.output_scroll = self.shell_command.output_scroll.min(max_scroll);
            let visual_range = self.shell_output_visual_range();

            let visible_rows = output_rows
                .iter()
                .enumerate()
                .skip(self.shell_command.output_scroll)
                .take(viewport_rows)
                .map(|(idx, row)| {
                    let in_visual =
                        visual_range.is_some_and(|(start, end)| idx >= start && idx <= end);
                    let is_cursor = idx == self.shell_command.output_cursor;
                    let base =
                        Line::from(Span::styled(row.clone(), Style::default().fg(theme.text)));
                    apply_row_highlight(
                        &base,
                        output_inner.width,
                        in_visual,
                        is_cursor,
                        theme.visual_bg,
                        theme.cursor_bg,
                        CursorSelectionPolicy::CursorWins,
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(visible_rows), output_inner);
        }

        if sections[2].width >= 3 && sections[2].height >= 3 && viewport_rows > 0 {
            let total_rows = output_rows.len().max(1);
            let (thumb_start, thumb_len) =
                scrollbar_thumb(total_rows, viewport_rows, self.shell_command.output_scroll);
            let x = sections[2]
                .x
                .saturating_add(sections[2].width.saturating_sub(2));
            let y = sections[2].y.saturating_add(1);
            let track_style = Style::default().fg(theme.dimmed);
            let thumb_style = Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::BOLD);
            let buffer = frame.buffer_mut();
            for row in 0..viewport_rows {
                buffer.set_string(x, y + row as u16, "│", track_style);
            }
            for row in thumb_start..thumb_start.saturating_add(thumb_len) {
                if row < viewport_rows {
                    buffer.set_string(x, y + row as u16, "█", thumb_style);
                }
            }
        }

        let footer_text = if self.shell_command.running.is_some() {
            Line::from(vec![
                key_chip("Esc", theme),
                Span::styled(" interrupt  ", Style::default().fg(theme.muted)),
                key_chip("Up/Down", theme),
                Span::styled(" move  ", Style::default().fg(theme.muted)),
                key_chip("v", theme),
                Span::styled(" range  ", Style::default().fg(theme.muted)),
                key_chip("y", theme),
                Span::styled(" copy output  ", Style::default().fg(theme.muted)),
                key_chip("Backspace", theme),
                Span::styled(" reset", Style::default().fg(theme.muted)),
            ])
        } else if self.shell_command.finished.is_some() {
            Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" continue  ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" close  ", Style::default().fg(theme.muted)),
                key_chip("v", theme),
                Span::styled(" range  ", Style::default().fg(theme.muted)),
                key_chip("y", theme),
                Span::styled(" copy output  ", Style::default().fg(theme.muted)),
                key_chip("Backspace", theme),
                Span::styled(" reset", Style::default().fg(theme.muted)),
            ])
        } else {
            Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" run  ", Style::default().fg(theme.muted)),
                key_chip("Ctrl-r", theme),
                Span::styled(" history search  ", Style::default().fg(theme.muted)),
                key_chip("Up/Down", theme),
                Span::styled(" history", Style::default().fg(theme.muted)),
            ])
        };
        frame.render_widget(
            Paragraph::new(footer_text).style(Style::default().bg(theme.modal_bg)),
            sections[3],
        );
    }

    fn comment_context_preview_lines(
        &self,
        max_rows: usize,
        theme: &UiTheme,
    ) -> Vec<Line<'static>> {
        let rows = max_rows.max(1);
        let mut lines = Vec::<Line<'static>>::new();
        let mut has_primary_context = false;

        match self.preferences.input_mode {
            InputMode::CommentEdit(id) => {
                if let Some(comment) = self.comments.comment_by_id(id) {
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            format!(
                                "{} {} ({} selected lines)",
                                comment.target.kind.as_str(),
                                comment_location_label(comment),
                                comment.target.selected_lines.len()
                            ),
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                    has_primary_context = !comment.target.selected_lines.is_empty();
                    self.push_compact_selection_preview(
                        &mut lines,
                        &comment.target.selected_lines,
                        rows.saturating_sub(1),
                        theme,
                    );
                }
            }
            InputMode::CommentCreate => match self.comment_target_from_selection() {
                Ok(Some(target)) => {
                    let start =
                        format_anchor_lines(target.start.old_lineno, target.start.new_lineno);
                    let end = format_anchor_lines(target.end.old_lineno, target.end.new_lineno);
                    let span = if start == end {
                        start
                    } else {
                        format!("{start} -> {end}")
                    };
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            format!(
                                "{} {} ({span}; {} selected lines)",
                                target.kind.as_str(),
                                sanitize_terminal_text(&target.start.file_path),
                                target.selected_lines.len()
                            ),
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                    has_primary_context = !target.selected_lines.is_empty();
                    self.push_compact_selection_preview(
                        &mut lines,
                        &target.selected_lines,
                        rows.saturating_sub(1),
                        theme,
                    );
                }
                Ok(None) => {
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            "no anchor at cursor; showing local diff snippet",
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                }
                Err(err) => {
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            format!("failed to resolve target: {err:#}"),
                            Style::default().fg(theme.issue),
                        ),
                    ]));
                }
            },
            InputMode::Normal
            | InputMode::ShellCommand
            | InputMode::WorktreeSwitch
            | InputMode::DiffSearch
            | InputMode::ListSearch(_) => {}
        }

        if !has_primary_context && !self.rendered_diff.is_empty() {
            let cursor = self.diff_position.cursor;
            let start = cursor.saturating_sub(1);
            let end = (cursor + 1).min(self.rendered_diff.len().saturating_sub(1));
            for idx in start..=end {
                if lines.len() >= rows {
                    break;
                }
                let focused = idx == cursor;
                lines.push(Line::from(vec![
                    Span::styled(
                        if focused { "> " } else { "  " },
                        if focused {
                            Style::default().fg(theme.accent)
                        } else {
                            Style::default().fg(theme.dimmed)
                        },
                    ),
                    Span::raw(truncate(
                        &sanitize_terminal_text(&self.rendered_diff[idx].raw_text),
                        120,
                    )),
                ]));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No diff context available at cursor.",
                Style::default().fg(theme.muted),
            )));
        }
        lines.truncate(rows);
        lines
    }

    fn push_compact_selection_preview(
        &self,
        out: &mut Vec<Line<'static>>,
        snippets: &[String],
        max_rows: usize,
        theme: &UiTheme,
    ) {
        if max_rows == 0 || snippets.is_empty() {
            return;
        }
        if snippets.len() <= max_rows {
            for snippet in snippets.iter().take(max_rows) {
                out.push(Line::from(vec![
                    Span::styled("  ", Style::default().fg(theme.dimmed)),
                    Span::raw(truncate(snippet, 120)),
                ]));
            }
            return;
        }

        if max_rows == 1 {
            out.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    format!("… {} lines selected …", snippets.len()),
                    Style::default().fg(theme.muted),
                ),
            ]));
            return;
        }

        let preview_rows = max_rows.saturating_sub(1);
        let head = preview_rows / 2;
        let tail = preview_rows.saturating_sub(head);
        for snippet in snippets.iter().take(head) {
            out.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(theme.dimmed)),
                Span::raw(truncate(snippet, 120)),
            ]));
        }

        let omitted = snippets.len().saturating_sub(head + tail);
        out.push(Line::from(vec![
            Span::styled("  ", Style::default().fg(theme.dimmed)),
            Span::styled(
                format!("… {omitted} lines omitted …"),
                Style::default().fg(theme.muted),
            ),
        ]));

        for snippet in snippets.iter().skip(snippets.len().saturating_sub(tail)) {
            out.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(theme.dimmed)),
                Span::raw(truncate(snippet, 120)),
            ]));
        }
    }

    pub(super) fn render_help_overlay(&self, frame: &mut Frame<'_>, theme: &UiTheme) {
        let area = centered_rect(70, 62, frame.area());
        frame.render_widget(Clear, area);

        let help_lines = vec![
            Line::from(vec![Span::styled(
                "HUNKR QUICK GUIDE",
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                key_chip("1/2/3", theme),
                Span::raw(" focus commits/files/diff"),
            ]),
            Line::from(vec![
                key_chip("h/l", theme),
                Span::raw(" cycle panes prev/next"),
            ]),
            Line::from(vec![key_chip("space", theme), Span::raw(" select commits")]),
            Line::from(vec![
                key_chip("v", theme),
                Span::raw(" visual select (commits or diff)"),
            ]),
            Line::from(vec![
                key_chip("u/r/i/s", theme),
                Span::raw(" set commit status"),
            ]),
            Line::from(vec![
                key_chip("m", theme),
                Span::raw(" add comment to commit/hunk/range"),
            ]),
            Line::from(vec![
                key_chip("!", theme),
                Span::raw(" open shell command modal"),
            ]),
            Line::from(vec![
                key_chip("w", theme),
                Span::raw(" open worktree switcher modal"),
            ]),
            Line::from(vec![
                key_chip("/", theme),
                Span::raw(" diff search or live list filter"),
            ]),
            Line::from(vec![
                key_chip("Esc/Enter", theme),
                Span::raw(" search: clear / defocus"),
            ]),
            Line::from(vec![
                key_chip("n/N", theme),
                Span::raw(" repeat diff search next/prev"),
            ]),
            Line::from(vec![
                key_chip("[/]", theme),
                Span::raw(" previous/next diff hunk"),
            ]),
            Line::from(vec![
                key_chip("zz/zt/zb", theme),
                Span::raw(" center/top/bottom cursor"),
            ]),
            Line::from(vec![
                key_chip("e", theme),
                Span::raw(" commits pane: cycle status filter"),
            ]),
            Line::from(vec![
                key_chip("e", theme),
                Span::raw(" diff pane: edit comment under cursor"),
            ]),
            Line::from(vec![
                key_chip("D", theme),
                Span::raw(" delete comment under cursor"),
            ]),
            Line::from(vec![
                key_chip("y", theme),
                Span::raw(" copy visual selection"),
            ]),
            Line::from(vec![
                key_chip("Y", theme),
                Span::raw(" copy review-task file path"),
            ]),
            Line::from(vec![key_chip("t", theme), Span::raw(" toggle theme")]),
            Line::from(vec![
                key_chip("Ctrl-d/u", theme),
                Span::raw(" big jump in focused pane"),
            ]),
            Line::from(vec![
                key_chip("Ctrl-l", theme),
                Span::raw(" hard refresh terminal view"),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("UNREVIEWED", Style::default().fg(theme.unreviewed)),
                Span::raw("  "),
                Span::styled("REVIEWED", Style::default().fg(theme.reviewed)),
                Span::raw("  "),
                Span::styled("ISSUE_FOUND", Style::default().fg(theme.issue)),
                Span::raw("  "),
                Span::styled("RESOLVED", Style::default().fg(theme.resolved)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.muted)),
                key_chip("?", theme),
                Span::styled(" to close", Style::default().fg(theme.muted)),
            ]),
        ];

        let widget = Paragraph::new(help_lines).block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme.focus_border)),
        );
        frame.render_widget(widget, area);
    }

    pub(super) fn render_worktree_switcher_modal(
        &mut self,
        frame: &mut Frame<'_>,
        theme: &UiTheme,
    ) {
        let area = centered_rect(82, 66, frame.area());
        frame.render_widget(Clear, area);

        let shell = Block::default()
            .title(" Worktrees ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.focus_border))
            .style(Style::default().bg(theme.modal_bg));
        frame.render_widget(shell.clone(), area);
        let inner = shell.inner(area);

        let sections = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(2),
                ratatui::layout::Constraint::Min(5),
                ratatui::layout::Constraint::Length(2),
            ])
            .split(inner);

        let visible = self.visible_worktree_indices();
        let search = if self.worktree_switch.query.trim().is_empty() {
            "off".to_owned()
        } else {
            format!("/{}", self.worktree_switch.query)
        };
        let selected = self
            .selected_worktree_full_index()
            .and_then(|idx| self.worktree_switch.entries.get(idx))
            .map(|entry| short_path_label(&entry.path))
            .unwrap_or_else(|| "none".to_owned());
        let summary = Paragraph::new(Line::from(vec![
            Span::styled("source: ", Style::default().fg(theme.dimmed)),
            Span::styled(
                short_path_label(self.git.root()),
                Style::default().fg(theme.text),
            ),
            Span::raw("  "),
            Span::styled("shown: ", Style::default().fg(theme.dimmed)),
            Span::styled(
                format!("{}/{}", visible.len(), self.worktree_switch.entries.len()),
                Style::default().fg(theme.text),
            ),
            Span::raw("  "),
            Span::styled("filter: ", Style::default().fg(theme.dimmed)),
            sanitized_span(
                &search,
                Some(
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
            ),
            Span::raw("  "),
            Span::styled("selected: ", Style::default().fg(theme.dimmed)),
            Span::styled(selected, Style::default().fg(theme.text)),
        ]));
        frame.render_widget(summary, sections[0]);

        let rows: Vec<ListItem<'static>> = if visible.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "No matching worktrees",
                Style::default().fg(theme.muted),
            )))]
        } else {
            visible
                .iter()
                .filter_map(|idx| self.worktree_switch.entries.get(*idx))
                .map(|entry| {
                    let current = if entry.path == self.git.root() {
                        "*"
                    } else {
                        " "
                    };
                    let branch = entry.branch.as_deref().unwrap_or("detached");
                    let short_head = short_id(&entry.head);
                    let mut tags = Vec::<&str>::new();
                    if entry.locked_reason.is_some() {
                        tags.push("locked");
                    }
                    if entry.prunable_reason.is_some() {
                        tags.push("prunable");
                    }
                    let tags = if tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", tags.join(","))
                    };
                    let line = format!(
                        "{current} {}  {branch}  {short_head}{tags}",
                        entry.path.display()
                    );
                    ListItem::new(Line::from(sanitized_span(
                        &line,
                        Some(Style::default().fg(theme.text)),
                    )))
                })
                .collect()
        };

        let list = List::new(rows)
            .block(
                Block::default()
                    .title(" available ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.border)),
            )
            .highlight_style(Style::default().bg(theme.visual_bg))
            .highlight_symbol(list_highlight_symbol(self.preferences.nerd_fonts));
        self.worktree_switch.viewport_rows = sections[1].height.saturating_sub(2) as usize;
        frame.render_stateful_widget(list, sections[1], &mut self.worktree_switch.list_state);

        let footer = Paragraph::new(Line::from(vec![
            key_chip("Enter", theme),
            Span::styled(" switch ", Style::default().fg(theme.muted)),
            key_chip("r", theme),
            Span::styled(" refresh ", Style::default().fg(theme.muted)),
            key_chip("type", theme),
            Span::styled(" filter ", Style::default().fg(theme.muted)),
            key_chip("Esc", theme),
            Span::styled(" close ", Style::default().fg(theme.muted)),
        ]));
        frame.render_widget(footer, sections[2]);
    }
}
pub(super) fn footer_mode_label(
    input_mode: InputMode,
    commit_visual_active: bool,
    diff_visual_active: bool,
) -> &'static str {
    match input_mode {
        InputMode::CommentCreate | InputMode::CommentEdit(_) => "COMMENT",
        InputMode::ShellCommand => "SHELL",
        InputMode::WorktreeSwitch => "WORKTREE",
        InputMode::DiffSearch | InputMode::ListSearch(_) => "SEARCH",
        InputMode::Normal if commit_visual_active || diff_visual_active => "VISUAL",
        InputMode::Normal => "NORMAL",
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(7).collect()
}

fn shell_prompt_line(buffer: &str, cursor: usize, theme: &UiTheme) -> Line<'static> {
    let clamped = clamp_char_boundary(buffer, cursor.min(buffer.len()));
    let cursor_char = buffer[clamped..].chars().next();
    let cursor_end = cursor_char
        .map(|ch| clamped.saturating_add(ch.len_utf8()))
        .unwrap_or(clamped);

    let mut spans = Vec::new();
    spans.push(Span::styled("$ ", Style::default().fg(theme.dimmed)));
    spans.push(sanitized_span(&buffer[..clamped], None));

    match cursor_char {
        Some(ch) => spans.push(Span::styled(
            ch.to_string(),
            Style::default()
                .fg(theme.modal_cursor_fg)
                .bg(theme.modal_cursor_bg),
        )),
        None => spans.push(Span::styled(
            " ",
            Style::default()
                .fg(theme.modal_cursor_fg)
                .bg(theme.modal_cursor_bg),
        )),
    }

    if cursor_end < buffer.len() {
        spans.push(sanitized_span(&buffer[cursor_end..], None));
    }
    Line::from(spans)
}

fn footer_visual_scope_label(
    commit_visual_active: bool,
    diff_visual_active: bool,
) -> Option<&'static str> {
    if commit_visual_active {
        Some("COMMITS")
    } else if diff_visual_active {
        Some("DIFF")
    } else {
        None
    }
}

fn footer_mode_style(mode: &str, theme: &UiTheme) -> Style {
    let base = Style::default().fg(theme.panel_title_fg);
    match mode {
        "VISUAL" => base.bg(blend_colors(theme.visual_bg, theme.panel_title_bg, 170)),
        "SEARCH" => base.bg(blend_colors(theme.accent, theme.panel_title_bg, 92)),
        "COMMENT" => base.bg(blend_colors(theme.accent, theme.panel_title_bg, 138)),
        _ => base.bg(theme.panel_title_bg),
    }
}

fn footer_status_style(status: &str, theme: &UiTheme) -> Style {
    if status.contains("failed") || status.contains("Failed") || status.contains("new unreviewed") {
        Style::default()
            .fg(theme.unreviewed)
            .add_modifier(Modifier::BOLD)
    } else if status.contains("No ") || status.contains("empty") {
        Style::default().fg(theme.issue)
    } else {
        Style::default().fg(theme.text)
    }
}

fn footer_chip(label: &str, style: Style, modifier: Modifier) -> Span<'static> {
    Span::styled(format!(" {label} "), style.add_modifier(modifier))
}

fn footer_detail_chip(label: String, theme: &UiTheme) -> Span<'static> {
    footer_chip(
        &label,
        Style::default()
            .fg(theme.muted)
            .bg(blend_colors(theme.panel_title_bg, theme.border, 176)),
        Modifier::empty(),
    )
}

fn footer_separator(theme: &UiTheme) -> Span<'static> {
    Span::styled(" | ", Style::default().fg(theme.dimmed))
}
