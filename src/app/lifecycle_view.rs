//! Render pipeline and modal/footer presentation for the lifecycle flow.

use super::ui::contracts::PaneViewModelBuilder;
use super::ui::snapshot::{AppRenderSnapshot, FooterShellSnapshot, FooterWorktreeSnapshot};
use super::ui::view_models::{CommitPaneVmBuilder, FilePaneVmBuilder};
use crate::app::*;
use ratatui::widgets::{List, ListItem};

impl App {
    pub(super) fn render_header(
        &self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
        snapshot: &AppRenderSnapshot,
    ) {
        let nerd_fonts = snapshot.nerd_fonts;
        let branch_prefix = branch_label_prefix(nerd_fonts);
        let wt_prefix = worktree_label_prefix(nerd_fonts);
        let branch_label = if nerd_fonts {
            format!("{branch_prefix} {} ", snapshot.header.branch_name)
        } else {
            format!("{branch_prefix}{} ", snapshot.header.branch_name)
        };
        let wt_label = if nerd_fonts {
            format!(
                "{wt_prefix} {} ",
                short_path_label(&snapshot.header.repo_root)
            )
        } else {
            format!(
                "{wt_prefix}{} ",
                short_path_label(&snapshot.header.repo_root)
            )
        };
        let headline = Line::from(vec![
            Span::styled(
                app_title_label(nerd_fonts),
                Style::default()
                    .fg(theme.panel_title_fg)
                    .bg(theme.panel_title_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(branch_label, Style::default().fg(theme.text)),
            Span::styled(wt_label, Style::default().fg(theme.muted)),
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
        snapshot: &AppRenderSnapshot,
    ) {
        let vm = FilePaneVmBuilder.build(&snapshot.files);
        ListPaneRenderer::new(
            theme,
            snapshot.focused,
            snapshot.nerd_fonts,
            snapshot.now_ts,
        )
        .render_files(
            frame,
            rect,
            FilePaneModel {
                file_rows: vm.file_rows,
                changed_files: vm.changed_files,
                shown_files: vm.shown_files,
                search_display: &vm.search_display,
                search_enabled: vm.search_enabled,
                file_list_state: &mut self.ui.file_ui.list_state,
            },
        );
    }

    pub(super) fn render_commits(
        &mut self,
        frame: &mut Frame<'_>,
        rect: ratatui::layout::Rect,
        theme: &UiTheme,
        snapshot: &AppRenderSnapshot,
    ) {
        let vm = CommitPaneVmBuilder.build(&snapshot.commits);
        ListPaneRenderer::new(
            theme,
            snapshot.focused,
            snapshot.nerd_fonts,
            snapshot.now_ts,
        )
        .render_commits(
            frame,
            rect,
            CommitPaneModel {
                commits: vm.commits,
                comment_badge_commit_ids: vm.comment_badge_commit_ids,
                status_counts: vm.status_counts,
                selected_total: vm.selected_total,
                shown_commits: vm.shown_commits,
                total_commits: vm.total_commits,
                status_filter: vm.status_filter,
                search_display: &vm.search_display,
                search_enabled: vm.search_enabled,
                commit_list_state: &mut self.ui.commit_ui.list_state,
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
        let visual_range = self
            .ui
            .diff_ui
            .visual_selection
            .and_then(|_| self.diff_selected_range());
        let viewport_rows = rect.height.saturating_sub(2).max(1) as usize;
        let sticky_banner_indexes =
            self.sticky_banner_indexes_for_scroll(self.domain.diff_position.scroll, viewport_rows);
        let sticky_rows = sticky_banner_indexes
            .len()
            .min(viewport_rows.saturating_sub(1));
        let body_rows = viewport_rows.saturating_sub(sticky_rows);
        let inner_width = rect.width.saturating_sub(2).max(1) as usize;
        let mut line_overrides = HashMap::new();
        let mut visible_row_to_line = Vec::with_capacity(viewport_rows);
        for idx in sticky_banner_indexes.iter().take(sticky_rows) {
            visible_row_to_line.push(*idx);
            if let Some(line) = self.highlight_visible_diff_line(*idx, theme) {
                line_overrides.insert(*idx, line);
            }
        }
        let target_rows = sticky_rows.saturating_add(body_rows);
        let mut line_idx = self.domain.diff_position.scroll;
        while visible_row_to_line.len() < target_rows && line_idx < self.domain.rendered_diff.len()
        {
            if let Some(line) = self.highlight_visible_diff_line(line_idx, theme) {
                line_overrides.insert(line_idx, line);
            }

            let display_line = line_overrides
                .get(&line_idx)
                .unwrap_or(&self.domain.rendered_diff[line_idx].line);
            let wrapped_rows = wrapped_line_rows(display_line, inner_width).max(1);
            for _ in 0..wrapped_rows {
                if visible_row_to_line.len() >= target_rows {
                    break;
                }
                visible_row_to_line.push(line_idx);
            }
            line_idx += 1;
        }
        self.ui.diff_ui.visible_row_to_line = visible_row_to_line;
        let empty_state_message = diff_empty_state_message(
            !self.domain.rendered_diff.is_empty(),
            self.domain.aggregate.files.len(),
            self.ui.diff_cache.file_ranges.len(),
            &self.ui.search.file_query,
        );
        let selected_file = self
            .ui
            .diff_cache
            .selected_file
            .as_deref()
            .filter(|path| self.ui.diff_cache.file_range_by_path.contains_key(*path));
        let title = DiffPaneTitle {
            selected_file,
            selected_file_progress: self.selected_file_progress(),
            nerd_fonts: self.ui.preferences.nerd_fonts,
            nerd_font_theme: &self.ui.preferences.nerd_font_theme,
            selected_lines,
        };
        let body = DiffPaneBody {
            rendered_diff: &self.domain.rendered_diff,
            diff_position: self.domain.diff_position,
            block_cursor_col: self.ui.diff_ui.block_cursor_col,
            search_query: self.ui.search.diff_query.as_deref(),
            visual_range,
            sticky_banner_indexes: &sticky_banner_indexes,
            empty_state_message: empty_state_message.as_deref(),
            line_overrides: &line_overrides,
        };
        DiffPaneRenderer::new(theme, self.ui.preferences.focused).render(frame, rect, title, body);
    }

    fn highlight_visible_diff_line(
        &mut self,
        idx: usize,
        theme: &UiTheme,
    ) -> Option<Line<'static>> {
        let rendered = self.domain.rendered_diff.get(idx)?;
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
        let mut highlighted = self.ui.diff_cache.highlighter.highlight_single_line(
            self.ui.preferences.theme_mode,
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
        snapshot: &AppRenderSnapshot,
    ) {
        let commit_visual_active = snapshot.footer.commit_visual_active;
        let diff_visual_active = snapshot.footer.diff_visual_active;
        let mode = footer_mode_label(
            snapshot.footer.input_mode,
            commit_visual_active,
            diff_visual_active,
        );

        let pane_line = match snapshot.footer.input_mode {
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
                if snapshot.footer.shell.running {
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
                } else if snapshot.footer.shell.finished {
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
            InputMode::WorktreeSwitch => {
                if snapshot.footer.worktree.search_active {
                    Line::from(vec![
                        key_chip("Enter", theme),
                        Span::styled(" defocus ", Style::default().fg(theme.muted)),
                        key_chip("Esc", theme),
                        Span::styled(" clear ", Style::default().fg(theme.muted)),
                        key_chip("Backspace", theme),
                        Span::styled(" edit ", Style::default().fg(theme.muted)),
                        key_chip("q", theme),
                        Span::styled(" close ", Style::default().fg(theme.muted)),
                    ])
                } else {
                    Line::from(vec![
                        key_chip("Enter", theme),
                        Span::styled(" switch ", Style::default().fg(theme.muted)),
                        key_chip("j/k", theme),
                        Span::styled(" move ", Style::default().fg(theme.muted)),
                        key_chip("Ctrl-d/u", theme),
                        Span::styled(" jump ", Style::default().fg(theme.muted)),
                        key_chip("/", theme),
                        Span::styled(" search ", Style::default().fg(theme.muted)),
                        key_chip("r", theme),
                        Span::styled(" refresh ", Style::default().fg(theme.muted)),
                        key_chip("Esc/q", theme),
                        Span::styled(" close", Style::default().fg(theme.muted)),
                    ])
                }
            }
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
            InputMode::Normal => match snapshot.footer.focused {
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
                    key_chip("u/r/i", theme),
                    Span::styled(" set status ", Style::default().fg(theme.muted)),
                    key_chip("e", theme),
                    Span::styled(" status filter ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" search ", Style::default().fg(theme.muted)),
                ]),
                FocusPane::Diff => Line::from(vec![
                    key_chip("v", theme),
                    Span::styled(" range ", Style::default().fg(theme.muted)),
                    key_chip("m/C-e", theme),
                    Span::styled(" add/edit comment ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" search ", Style::default().fg(theme.muted)),
                    key_chip("n/N", theme),
                    Span::styled(" search next/prev ", Style::default().fg(theme.muted)),
                    key_chip("p/P", theme),
                    Span::styled(" comment next/prev ", Style::default().fg(theme.muted)),
                    key_chip("w/e/b", theme),
                    Span::styled(" word ", Style::default().fg(theme.muted)),
                    key_chip("0/^/$/H/L", theme),
                    Span::styled(" line ", Style::default().fg(theme.muted)),
                    key_chip("[/]", theme),
                    Span::styled(" hunks ", Style::default().fg(theme.muted)),
                    key_chip("Ctrl-d/u", theme),
                    Span::styled(" jump ", Style::default().fg(theme.muted)),
                ]),
            },
        };

        let global_line = Line::from(vec![
            key_chip("1/2/3", theme),
            Span::styled(" panes ", Style::default().fg(theme.dimmed)),
            key_chip("Tab/S-Tab", theme),
            Span::styled(" cycle ", Style::default().fg(theme.dimmed)),
            key_chip("Left/Right", theme),
            Span::styled(" prev/next pane ", Style::default().fg(theme.dimmed)),
            key_chip("!", theme),
            Span::styled(" shell ", Style::default().fg(theme.dimmed)),
            key_chip("Ctrl-w", theme),
            Span::styled(" worktrees ", Style::default().fg(theme.dimmed)),
            key_chip("Ctrl-r/F5", theme),
            Span::styled(" refresh ", Style::default().fg(theme.dimmed)),
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
        let show_primary_status = !snapshot.footer.status.is_empty()
            && !matches!(
                snapshot.footer.input_mode,
                InputMode::DiffSearch | InputMode::ListSearch(_) | InputMode::WorktreeSwitch
            );
        if show_primary_status {
            status.push(Span::raw(" "));
            status.push(Span::styled(
                snapshot.footer.status.clone(),
                footer_status_style(&snapshot.footer.status, theme),
            ));
        }
        if should_show_footer_commit_metadata(snapshot.footer.input_mode, snapshot.footer.focused) {
            let metadata = super::ui::list_panes::focused_commit_metadata_summary(
                snapshot.footer.focused_commit.as_ref(),
                snapshot.nerd_fonts,
            );
            let available = footer_available_width_for_next_chip(&status, rect.width as usize);
            let metadata = truncate(&metadata, available.saturating_sub(2).max(8));
            status.push(footer_separator(theme));
            status.push(footer_detail_chip(metadata, theme));
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

        match snapshot.footer.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => {
                let line_count = snapshot.footer.comment_buffer.matches('\n').count() + 1;
                let (line, col) = comment_cursor_line_col(
                    &snapshot.footer.comment_buffer,
                    snapshot.footer.comment_cursor,
                );
                status.push(footer_separator(theme));
                status.push(footer_detail_chip(
                    format!("{} chars", snapshot.footer.comment_buffer.chars().count()),
                    theme,
                ));
                status.push(Span::raw(" "));
                status.push(footer_detail_chip(
                    format!("Ln {line}, Col {col}, {line_count} lines"),
                    theme,
                ));
            }
            InputMode::DiffSearch => {
                status.push(footer_separator(theme));
                status.extend(search_prompt_spans(
                    &snapshot.footer.diff_search_buffer,
                    snapshot.footer.diff_search_cursor,
                    theme,
                ));
            }
            InputMode::ListSearch(pane) => {
                let (query, cursor) = match pane {
                    FocusPane::Commits => (
                        snapshot.footer.commit_query.as_str(),
                        snapshot.footer.commit_cursor,
                    ),
                    FocusPane::Files => (
                        snapshot.footer.file_query.as_str(),
                        snapshot.footer.file_cursor,
                    ),
                    FocusPane::Diff => ("", 0),
                };
                status.push(footer_separator(theme));
                status.extend(search_prompt_spans(query, cursor, theme));
            }
            InputMode::ShellCommand => {
                status.push(footer_separator(theme));
                let label = footer_shell_mode_label(&snapshot.footer.shell);
                status.push(footer_chip(
                    &format!(
                        "{label}: {}",
                        truncate(&snapshot.footer.shell.command_label, 42)
                    ),
                    Style::default().fg(theme.accent).bg(blend_colors(
                        theme.panel_title_bg,
                        theme.border,
                        176,
                    )),
                    Modifier::BOLD,
                ));
            }
            InputMode::WorktreeSwitch => {
                if let Some(query) = footer_worktree_query_label(&snapshot.footer.worktree) {
                    status.push(footer_separator(theme));
                    status.push(footer_chip(
                        &format!(
                            "{query} {}/{}",
                            snapshot.footer.worktree.visible_count,
                            snapshot.footer.worktree.total_count
                        ),
                        Style::default().fg(theme.accent).bg(blend_colors(
                            theme.panel_title_bg,
                            theme.border,
                            176,
                        )),
                        Modifier::BOLD,
                    ));
                }
            }
            InputMode::Normal => {}
        }

        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(1),
                ratatui::layout::Constraint::Length(3),
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

        let title = match self.ui.preferences.input_mode {
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

        let mode_badge = match self.ui.preferences.input_mode {
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
                    format!("{} chars", self.ui.comment_editor.buffer.chars().count()),
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
        if matches!(self.ui.preferences.input_mode, InputMode::CommentCreate)
            && self.ui.comment_editor.create_target_cache.is_none()
        {
            self.refresh_comment_create_target_cache();
        }
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
            &self.ui.comment_editor.buffer,
            self.ui.comment_editor.cursor,
            self.ui.comment_editor.selection,
            editor_inner.height.saturating_sub(1) as usize,
            editor_inner.width as usize,
            theme,
        );
        let CommentModalView {
            lines,
            line_ranges,
            view_start,
            text_offset,
        } = modal_view;
        self.ui.comment_editor.rect = Some(editor_inner);
        self.ui.comment_editor.line_ranges = line_ranges;
        self.ui.comment_editor.view_start = view_start;
        self.ui.comment_editor.text_offset = text_offset;
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

        let command_failed = self
            .ui
            .shell_command
            .finished
            .as_ref()
            .is_some_and(|result| {
                result
                    .exit_status
                    .code()
                    .map(|code| code != 0)
                    .unwrap_or(true)
            });
        let border_color = if command_failed {
            theme.unreviewed
        } else if self.ui.shell_command.finished.is_some() {
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

        let mode_badge = if self.ui.shell_command.running.is_some() {
            "running"
        } else if self.ui.shell_command.finished.is_some() {
            "done"
        } else if self.ui.shell_command.reverse_search.is_some() {
            "search"
        } else {
            "input"
        };
        let status_text = if let Some(result) = self.ui.shell_command.finished.as_ref() {
            match result.exit_status.code() {
                Some(code) => format!("exit code {code}"),
                None => "process terminated by signal".to_owned(),
            }
        } else if self.ui.shell_command.running.is_some() {
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
                    format!("{} commands", self.ui.shell_command.history.len()),
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
            self.ui.shell_command.running.is_none() && self.ui.shell_command.finished.is_none();
        if command_editable && let Some(search) = self.ui.shell_command.reverse_search.as_ref() {
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
                &self.ui.shell_command.buffer,
                self.ui.shell_command.cursor,
                theme,
            ));
        } else {
            command_lines.push(Line::from(vec![
                Span::styled("$ ", Style::default().fg(theme.dimmed)),
                sanitized_span(&self.ui.shell_command.buffer, None),
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
        self.ui.shell_command.output_rect = Some(output_inner);
        self.ui.shell_command.output_viewport = output_inner.height as usize;
        frame.render_widget(output_block, sections[2]);

        let output_rows = self.shell_output_rows();

        if self.ui.shell_command.output_follow {
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
            self.ui.shell_command.output_cursor = self
                .ui
                .shell_command
                .output_cursor
                .min(output_rows.len().saturating_sub(1));
            let max_scroll = self.shell_output_max_scroll();
            self.ui.shell_command.output_scroll =
                self.ui.shell_command.output_scroll.min(max_scroll);
            let visual_range = self.shell_output_visual_range();

            let visible_rows = output_rows
                .iter()
                .enumerate()
                .skip(self.ui.shell_command.output_scroll)
                .take(viewport_rows)
                .map(|(idx, row)| {
                    let in_visual =
                        visual_range.is_some_and(|(start, end)| idx >= start && idx <= end);
                    let is_cursor = idx == self.ui.shell_command.output_cursor;
                    let base =
                        Line::from(Span::styled(row.clone(), Style::default().fg(theme.text)));
                    apply_row_highlight(
                        &base,
                        output_inner.width,
                        in_visual,
                        is_cursor,
                        theme.visual_bg,
                        theme.focused_cursor_bg,
                        CursorSelectionPolicy::BlendCursorOverSelection {
                            weight: theme.cursor_visual_overlap_weight,
                        },
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(visible_rows), output_inner);
        }

        if sections[2].width >= 3 && sections[2].height >= 3 && viewport_rows > 0 {
            let total_rows = output_rows.len().max(1);
            let (thumb_start, thumb_len) = scrollbar_thumb(
                total_rows,
                viewport_rows,
                self.ui.shell_command.output_scroll,
            );
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

        let footer_text = if self.ui.shell_command.running.is_some() {
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
        } else if self.ui.shell_command.finished.is_some() {
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

        match self.ui.preferences.input_mode {
            InputMode::CommentEdit(id) => {
                if let Some(comment) = self.deps.comments.comment_by_id(id) {
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
            InputMode::CommentCreate => match self.ui.comment_editor.create_target_cache.as_ref() {
                Some(CommentCreateTargetCache::Ready(target)) => match target.as_ref() {
                    Some(target) => {
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
                    None => {
                        lines.push(Line::from(vec![
                            Span::styled("target ", Style::default().fg(theme.dimmed)),
                            Span::styled(
                                "no anchor at cursor; showing local diff snippet",
                                Style::default().fg(theme.muted),
                            ),
                        ]));
                    }
                },
                Some(CommentCreateTargetCache::Error(err)) => {
                    lines.push(Line::from(vec![
                        Span::styled("target ", Style::default().fg(theme.dimmed)),
                        Span::styled(
                            format!("failed to resolve target: {err}"),
                            Style::default().fg(theme.issue),
                        ),
                    ]));
                }
                None => {}
            },
            InputMode::Normal
            | InputMode::ShellCommand
            | InputMode::WorktreeSwitch
            | InputMode::DiffSearch
            | InputMode::ListSearch(_) => {}
        }

        if !has_primary_context && !self.domain.rendered_diff.is_empty() {
            let cursor = self.domain.diff_position.cursor;
            let start = cursor.saturating_sub(1);
            let end = (cursor + 1).min(self.domain.rendered_diff.len().saturating_sub(1));
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
                        &sanitize_terminal_text(&self.domain.rendered_diff[idx].raw_text),
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
                key_chip("Left/Right", theme),
                Span::raw(" cycle panes prev/next"),
            ]),
            Line::from(vec![key_chip("space", theme), Span::raw(" select commits")]),
            Line::from(vec![
                key_chip("Esc", theme),
                Span::raw(" clear commit selection"),
            ]),
            Line::from(vec![
                key_chip("v", theme),
                Span::raw(" visual select (commits or diff)"),
            ]),
            Line::from(vec![
                key_chip("u/r/i", theme),
                Span::raw(" set status for selected/current commits"),
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
                key_chip("Ctrl-w", theme),
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
                key_chip("p/P", theme),
                Span::raw(" jump to next/previous comment"),
            ]),
            Line::from(vec![
                key_chip("*/#", theme),
                Span::raw(" search word under diff block cursor"),
            ]),
            Line::from(vec![
                key_chip("h/l", theme),
                Span::raw(" move diff block cursor left/right"),
            ]),
            Line::from(vec![
                key_chip("w/e/b", theme),
                Span::raw(" diff: next-start/end/prev-start word"),
            ]),
            Line::from(vec![
                key_chip("W/E/B", theme),
                Span::raw(" diff: WORD motions (whitespace-delimited)"),
            ]),
            Line::from(vec![
                key_chip("0/^/$/H/L", theme),
                Span::raw(" diff: line start / first non-space / line end"),
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
                key_chip("Ctrl-e", theme),
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
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(theme.muted)),
                key_chip("?", theme),
                Span::styled(" / ", Style::default().fg(theme.muted)),
                key_chip("q", theme),
                Span::styled(" / ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
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
        let search = if self.ui.worktree_switch.query.trim().is_empty() {
            if self.ui.worktree_switch.search_active {
                "/".to_owned()
            } else {
                "off".to_owned()
            }
        } else {
            format!("/{}", self.ui.worktree_switch.query)
        };
        let filter_style =
            if self.ui.worktree_switch.search_active || !self.ui.worktree_switch.query.is_empty() {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.dimmed)
            };
        let selected = self
            .selected_worktree_full_index()
            .and_then(|idx| self.ui.worktree_switch.entries.get(idx))
            .map(|entry| short_path_label(&entry.path))
            .unwrap_or_else(|| "none".to_owned());
        let summary = Paragraph::new(Line::from(vec![
            Span::styled("source: ", Style::default().fg(theme.dimmed)),
            Span::styled(
                short_path_label(self.deps.git.root()),
                Style::default().fg(theme.text),
            ),
            Span::raw("  "),
            Span::styled("shown: ", Style::default().fg(theme.dimmed)),
            Span::styled(
                format!(
                    "{}/{}",
                    visible.len(),
                    self.ui.worktree_switch.entries.len()
                ),
                Style::default().fg(theme.text),
            ),
            Span::raw("  "),
            Span::styled("filter: ", Style::default().fg(theme.dimmed)),
            sanitized_span(&search, Some(filter_style)),
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
            let now_ts = self.now_timestamp();
            visible
                .iter()
                .filter_map(|idx| self.ui.worktree_switch.entries.get(*idx))
                .map(|entry| {
                    let current = if entry.path == self.deps.git.root() {
                        "*"
                    } else {
                        " "
                    };
                    let branch = entry.branch.as_deref().unwrap_or("detached");
                    let short_head = short_id(&entry.head);
                    let latest = entry
                        .latest_commit_ts
                        .map(|ts| format_relative_time(ts, now_ts))
                        .unwrap_or_else(|| "unknown".to_owned());
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
                        "{current} {}  {latest}  {branch}  {short_head}{tags}",
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
            .highlight_style(Style::default().bg(theme.focused_cursor_bg))
            .highlight_symbol(list_highlight_symbol(self.ui.preferences.nerd_fonts));
        self.ui.worktree_switch.viewport_rows = sections[1].height.saturating_sub(2) as usize;
        frame.render_stateful_widget(list, sections[1], &mut self.ui.worktree_switch.list_state);

        let footer = if self.ui.worktree_switch.search_active {
            Paragraph::new(Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" defocus ", Style::default().fg(theme.muted)),
                key_chip("Esc", theme),
                Span::styled(" clear ", Style::default().fg(theme.muted)),
                key_chip("Backspace", theme),
                Span::styled(" edit ", Style::default().fg(theme.muted)),
                key_chip("q", theme),
                Span::styled(" close ", Style::default().fg(theme.muted)),
            ]))
        } else {
            Paragraph::new(Line::from(vec![
                key_chip("Enter", theme),
                Span::styled(" switch ", Style::default().fg(theme.muted)),
                key_chip("/", theme),
                Span::styled(" search ", Style::default().fg(theme.muted)),
                key_chip("r", theme),
                Span::styled(" refresh ", Style::default().fg(theme.muted)),
                key_chip("Esc/q", theme),
                Span::styled(" close ", Style::default().fg(theme.muted)),
            ]))
        };
        frame.render_widget(footer, sections[2]);
    }
}

fn footer_shell_mode_label(shell: &FooterShellSnapshot) -> &'static str {
    if shell.running {
        "running"
    } else if shell.finished {
        "done"
    } else if shell.reverse_search {
        "search"
    } else {
        "input"
    }
}

fn footer_worktree_query_label(worktree: &FooterWorktreeSnapshot) -> Option<String> {
    if !worktree.search_active && worktree.query.is_empty() {
        return None;
    }
    if worktree.query.is_empty() {
        return Some("/".to_owned());
    }
    Some(format!("/{}", worktree.query))
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

fn search_prompt_spans(buffer: &str, cursor: usize, theme: &UiTheme) -> Vec<Span<'static>> {
    let prompt_style = Style::default()
        .fg(theme.accent)
        .bg(blend_colors(theme.panel_title_bg, theme.border, 176))
        .add_modifier(Modifier::BOLD);
    let cursor_style = Style::default()
        .fg(theme.modal_cursor_fg)
        .bg(theme.modal_cursor_bg);
    let clamped = clamp_char_boundary(buffer, cursor.min(buffer.len()));
    let cursor_char = buffer[clamped..].chars().next();
    let cursor_end = cursor_char
        .map(|ch| clamped.saturating_add(ch.len_utf8()))
        .unwrap_or(clamped);

    let mut spans = Vec::new();
    spans.push(Span::styled(" ", prompt_style));
    spans.push(Span::styled("/", prompt_style));
    if clamped > 0 {
        spans.push(sanitized_span(&buffer[..clamped], Some(prompt_style)));
    }
    match cursor_char {
        Some(ch) => spans.push(Span::styled(ch.to_string(), cursor_style)),
        None => spans.push(Span::styled(" ", cursor_style)),
    }
    if cursor_end < buffer.len() {
        spans.push(sanitized_span(&buffer[cursor_end..], Some(prompt_style)));
    }
    spans.push(Span::styled(" ", prompt_style));
    spans
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
            .bg(resolve_footer_chip_bg(theme)),
        Modifier::empty(),
    )
}

fn footer_separator(theme: &UiTheme) -> Span<'static> {
    Span::styled(" | ", Style::default().fg(theme.dimmed))
}

fn should_show_footer_commit_metadata(input_mode: InputMode, focused: FocusPane) -> bool {
    matches!(input_mode, InputMode::Normal) && focused == FocusPane::Commits
}

fn footer_available_width_for_next_chip(
    current_spans: &[Span<'static>],
    total_width: usize,
) -> usize {
    let used = current_spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum::<usize>()
        + display_width(" | ");
    total_width.saturating_sub(used).max(1)
}

fn resolve_footer_chip_bg(theme: &UiTheme) -> Color {
    theme.footer_chip_bg
}

#[cfg(test)]
mod footer_contract_tests {
    use super::*;

    #[test]
    fn footer_shell_mode_label_contract() {
        let base = FooterShellSnapshot {
            running: false,
            finished: false,
            reverse_search: false,
            command_label: "cmd".to_owned(),
        };
        assert_eq!(footer_shell_mode_label(&base), "input");

        let running = FooterShellSnapshot {
            running: true,
            ..base
        };
        assert_eq!(footer_shell_mode_label(&running), "running");

        let finished = FooterShellSnapshot {
            running: false,
            finished: true,
            reverse_search: true,
            command_label: "cmd".to_owned(),
        };
        assert_eq!(footer_shell_mode_label(&finished), "done");

        let searching = FooterShellSnapshot {
            running: false,
            finished: false,
            reverse_search: true,
            command_label: "cmd".to_owned(),
        };
        assert_eq!(footer_shell_mode_label(&searching), "search");
    }

    #[test]
    fn footer_worktree_query_label_contract() {
        let idle = FooterWorktreeSnapshot {
            search_active: false,
            query: String::new(),
            visible_count: 0,
            total_count: 0,
        };
        assert_eq!(footer_worktree_query_label(&idle), None);

        let active_empty = FooterWorktreeSnapshot {
            search_active: true,
            query: String::new(),
            visible_count: 1,
            total_count: 2,
        };
        assert_eq!(
            footer_worktree_query_label(&active_empty).as_deref(),
            Some("/")
        );

        let with_query = FooterWorktreeSnapshot {
            search_active: false,
            query: "feat".to_owned(),
            visible_count: 1,
            total_count: 2,
        };
        assert_eq!(
            footer_worktree_query_label(&with_query).as_deref(),
            Some("/feat")
        );
    }

    #[test]
    fn resolve_footer_chip_bg_uses_reset_when_reset() {
        let theme = UiTheme::from_mode(ThemeMode::Dark);
        assert_eq!(resolve_footer_chip_bg(&theme), Color::Reset);
    }

    #[test]
    fn resolve_footer_chip_bg_uses_explicit_override() {
        let mut theme = UiTheme::from_mode(ThemeMode::Dark);
        theme.footer_chip_bg = Color::Rgb(1, 2, 3);
        assert_eq!(resolve_footer_chip_bg(&theme), Color::Rgb(1, 2, 3));
    }

    #[test]
    fn should_show_footer_commit_metadata_only_for_normal_commit_focus() {
        assert!(should_show_footer_commit_metadata(
            InputMode::Normal,
            FocusPane::Commits
        ));
        assert!(!should_show_footer_commit_metadata(
            InputMode::Normal,
            FocusPane::Files
        ));
        assert!(!should_show_footer_commit_metadata(
            InputMode::DiffSearch,
            FocusPane::Commits
        ));
    }
}
