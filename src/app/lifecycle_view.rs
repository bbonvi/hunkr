//! Render pipeline and modal/footer presentation for the lifecycle flow.

use super::ui::contracts::PaneViewModelBuilder;
use super::ui::snapshot::{AppRenderSnapshot, FooterShellSnapshot, FooterWorktreeSnapshot};
use super::ui::view_models::{CommitPaneVmBuilder, FilePaneVmBuilder};
use crate::app::*;
use ratatui::widgets::{List, ListItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HelperChipBinding {
    label: &'static str,
    action: HelperClickAction,
}

fn helper_key(label: &'static str, code: KeyCode, modifiers: KeyModifiers) -> HelperChipBinding {
    HelperChipBinding {
        label,
        action: HelperClickAction::Key { code, modifiers },
    }
}

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
        let wt_label = format!(
            "{wt_prefix}{} ",
            short_path_label(&snapshot.header.repo_root)
        );
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
        let mut visible_rows = Vec::with_capacity(viewport_rows);
        for idx in sticky_banner_indexes.iter().take(sticky_rows) {
            visible_rows.push(DiffVisibleRow {
                line_index: *idx,
                wrapped_row_offset: 0,
            });
            if let Some(line) = self.highlight_visible_diff_line(*idx, theme) {
                line_overrides.insert(*idx, line);
            }
        }
        let target_rows = sticky_rows.saturating_add(body_rows);
        let mut line_idx = self.domain.diff_position.scroll;
        while visible_rows.len() < target_rows && line_idx < self.domain.rendered_diff.len() {
            if let Some(line) = self.highlight_visible_diff_line(line_idx, theme) {
                line_overrides.insert(line_idx, line);
            }

            let display_line = line_overrides
                .get(&line_idx)
                .unwrap_or(&self.domain.rendered_diff[line_idx].line);
            let wrapped_rows = wrapped_line_rows(display_line, inner_width).max(1);
            for wrapped_row_offset in 0..wrapped_rows {
                if visible_rows.len() >= target_rows {
                    break;
                }
                visible_rows.push(DiffVisibleRow {
                    line_index: line_idx,
                    wrapped_row_offset,
                });
            }
            line_idx += 1;
        }
        self.ui.diff_ui.visible_rows = visible_rows;
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
        &mut self,
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

        let (pane_line, pane_bindings) = match snapshot.footer.input_mode {
            InputMode::CommentCreate | InputMode::CommentEdit(_) => (
                Line::from(vec![
                    key_chip("Enter", theme),
                    Span::styled(" save ", Style::default().fg(theme.muted)),
                    key_chip("Alt+Enter", theme),
                    Span::styled(" newline ", Style::default().fg(theme.muted)),
                    key_chip("Esc", theme),
                    Span::styled(" cancel", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                    helper_key("Alt+Enter", KeyCode::Enter, KeyModifiers::ALT),
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                ],
            ),
            InputMode::ShellCommand => {
                if snapshot.footer.shell.running {
                    (
                        Line::from(vec![
                            key_chip("j", theme),
                            Span::styled(" move ", Style::default().fg(theme.muted)),
                            key_chip("y", theme),
                            Span::styled(" copy output ", Style::default().fg(theme.muted)),
                            key_chip("Esc", theme),
                            Span::styled(" interrupt ", Style::default().fg(theme.muted)),
                            key_chip("Backspace", theme),
                            Span::styled(" reset", Style::default().fg(theme.muted)),
                        ]),
                        vec![
                            helper_key("j", KeyCode::Char('j'), KeyModifiers::NONE),
                            helper_key("y", KeyCode::Char('y'), KeyModifiers::NONE),
                            helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                            helper_key("Backspace", KeyCode::Backspace, KeyModifiers::NONE),
                        ],
                    )
                } else if snapshot.footer.shell.finished {
                    (
                        Line::from(vec![
                            key_chip("Enter", theme),
                            Span::styled(" continue ", Style::default().fg(theme.muted)),
                            key_chip("y", theme),
                            Span::styled(" copy output ", Style::default().fg(theme.muted)),
                            key_chip("Esc", theme),
                            Span::styled(" close ", Style::default().fg(theme.muted)),
                            key_chip("Backspace", theme),
                            Span::styled(" reset", Style::default().fg(theme.muted)),
                        ]),
                        vec![
                            helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                            helper_key("y", KeyCode::Char('y'), KeyModifiers::NONE),
                            helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                            helper_key("Backspace", KeyCode::Backspace, KeyModifiers::NONE),
                        ],
                    )
                } else {
                    (
                        Line::from(vec![
                            key_chip("Enter", theme),
                            Span::styled(" run ", Style::default().fg(theme.muted)),
                            key_chip("Ctrl-r", theme),
                            Span::styled(" history search ", Style::default().fg(theme.muted)),
                            key_chip("Esc", theme),
                            Span::styled(" close", Style::default().fg(theme.muted)),
                        ]),
                        vec![
                            helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                            helper_key("Ctrl-r", KeyCode::Char('r'), KeyModifiers::CONTROL),
                            helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                        ],
                    )
                }
            }
            InputMode::WorktreeSwitch => {
                if snapshot.footer.worktree.search_active {
                    (
                        Line::from(vec![
                            key_chip("Enter", theme),
                            Span::styled(" apply ", Style::default().fg(theme.muted)),
                            key_chip("Esc", theme),
                            Span::styled(" clear ", Style::default().fg(theme.muted)),
                            key_chip("Backspace", theme),
                            Span::styled(" edit ", Style::default().fg(theme.muted)),
                            key_chip("q", theme),
                            Span::styled(" close", Style::default().fg(theme.muted)),
                        ]),
                        vec![
                            helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                            helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                            helper_key("Backspace", KeyCode::Backspace, KeyModifiers::NONE),
                            helper_key("q", KeyCode::Char('q'), KeyModifiers::NONE),
                        ],
                    )
                } else {
                    (
                        Line::from(vec![
                            key_chip("Enter", theme),
                            Span::styled(" switch ", Style::default().fg(theme.muted)),
                            key_chip("/", theme),
                            Span::styled(" filter ", Style::default().fg(theme.muted)),
                            key_chip("r", theme),
                            Span::styled(" refresh ", Style::default().fg(theme.muted)),
                            key_chip("Esc", theme),
                            Span::styled(" close", Style::default().fg(theme.muted)),
                        ]),
                        vec![
                            helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                            helper_key("/", KeyCode::Char('/'), KeyModifiers::NONE),
                            helper_key("r", KeyCode::Char('r'), KeyModifiers::NONE),
                            helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                        ],
                    )
                }
            }
            InputMode::DiffSearch => (
                Line::from(vec![
                    key_chip("Enter", theme),
                    Span::styled(" apply ", Style::default().fg(theme.muted)),
                    key_chip("Esc", theme),
                    Span::styled(" clear", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                ],
            ),
            InputMode::ListSearch(_) => (
                Line::from(vec![
                    key_chip("Enter", theme),
                    Span::styled(" apply ", Style::default().fg(theme.muted)),
                    key_chip("Esc", theme),
                    Span::styled(" clear ", Style::default().fg(theme.muted)),
                    key_chip("Backspace", theme),
                    Span::styled(" edit", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                    helper_key("Backspace", KeyCode::Backspace, KeyModifiers::NONE),
                ],
            ),
            InputMode::Normal => match snapshot.footer.focused {
                FocusPane::Files => (
                    Line::from(vec![
                        key_chip("j", theme),
                        Span::styled(" move ", Style::default().fg(theme.muted)),
                        key_chip("/", theme),
                        Span::styled(" filter ", Style::default().fg(theme.muted)),
                        key_chip("Enter", theme),
                        Span::styled(" focus diff", Style::default().fg(theme.muted)),
                    ]),
                    vec![
                        helper_key("j", KeyCode::Char('j'), KeyModifiers::NONE),
                        helper_key("/", KeyCode::Char('/'), KeyModifiers::NONE),
                        helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                    ],
                ),
                FocusPane::Commits => (
                    Line::from(vec![
                        key_chip("Space", theme),
                        Span::styled(" select ", Style::default().fg(theme.muted)),
                        key_chip("u", theme),
                        Span::styled(" Unreviewed ", Style::default().fg(theme.muted)),
                        key_chip("r", theme),
                        Span::styled(" Reviewed ", Style::default().fg(theme.muted)),
                        key_chip("i", theme),
                        Span::styled(" Issue Found ", Style::default().fg(theme.muted)),
                        key_chip("e", theme),
                        Span::styled(" Status Filter ", Style::default().fg(theme.muted)),
                        key_chip("/", theme),
                        Span::styled(" filter", Style::default().fg(theme.muted)),
                    ]),
                    vec![
                        helper_key("Space", KeyCode::Char(' '), KeyModifiers::NONE),
                        helper_key("u", KeyCode::Char('u'), KeyModifiers::NONE),
                        helper_key("r", KeyCode::Char('r'), KeyModifiers::NONE),
                        helper_key("i", KeyCode::Char('i'), KeyModifiers::NONE),
                        helper_key("e", KeyCode::Char('e'), KeyModifiers::NONE),
                        helper_key("/", KeyCode::Char('/'), KeyModifiers::NONE),
                    ],
                ),
                FocusPane::Diff => (
                    Line::from(vec![
                        key_chip("v", theme),
                        Span::styled(" range ", Style::default().fg(theme.muted)),
                        key_chip("m", theme),
                        Span::styled(" comment ", Style::default().fg(theme.muted)),
                        key_chip("/", theme),
                        Span::styled(" search ", Style::default().fg(theme.muted)),
                        key_chip("n", theme),
                        Span::styled(" next ", Style::default().fg(theme.muted)),
                        key_chip("N", theme),
                        Span::styled(" prev ", Style::default().fg(theme.muted)),
                        key_chip("[", theme),
                        Span::styled(" prev hunk ", Style::default().fg(theme.muted)),
                        key_chip("]", theme),
                        Span::styled(" next hunk", Style::default().fg(theme.muted)),
                    ]),
                    vec![
                        helper_key("v", KeyCode::Char('v'), KeyModifiers::NONE),
                        helper_key("m", KeyCode::Char('m'), KeyModifiers::NONE),
                        helper_key("/", KeyCode::Char('/'), KeyModifiers::NONE),
                        helper_key("n", KeyCode::Char('n'), KeyModifiers::NONE),
                        helper_key("N", KeyCode::Char('N'), KeyModifiers::SHIFT),
                        helper_key("[", KeyCode::Char('['), KeyModifiers::NONE),
                        helper_key("]", KeyCode::Char(']'), KeyModifiers::NONE),
                    ],
                ),
            },
        };

        let show_global_hints = rect.width >= 96;
        let (global_line, global_bindings) = if show_global_hints {
            (
                Line::from(vec![
                    key_chip("1", theme),
                    Span::styled(" commits ", Style::default().fg(theme.dimmed)),
                    key_chip("2", theme),
                    Span::styled(" files ", Style::default().fg(theme.dimmed)),
                    key_chip("3", theme),
                    Span::styled(" diff ", Style::default().fg(theme.dimmed)),
                    key_chip("Tab", theme),
                    Span::styled(" cycle ", Style::default().fg(theme.dimmed)),
                    key_chip("!", theme),
                    Span::styled(" shell ", Style::default().fg(theme.dimmed)),
                    key_chip("Ctrl-w", theme),
                    Span::styled(" worktrees ", Style::default().fg(theme.dimmed)),
                    key_chip("Ctrl-r", theme),
                    Span::styled(" refresh ", Style::default().fg(theme.dimmed)),
                    key_chip("?", theme),
                    Span::styled(" help ", Style::default().fg(theme.dimmed)),
                    key_chip("q", theme),
                    Span::styled(" quit", Style::default().fg(theme.dimmed)),
                ]),
                vec![
                    helper_key("1", KeyCode::Char('1'), KeyModifiers::NONE),
                    helper_key("2", KeyCode::Char('2'), KeyModifiers::NONE),
                    helper_key("3", KeyCode::Char('3'), KeyModifiers::NONE),
                    helper_key("Tab", KeyCode::Tab, KeyModifiers::NONE),
                    helper_key("!", KeyCode::Char('!'), KeyModifiers::SHIFT),
                    helper_key("Ctrl-w", KeyCode::Char('w'), KeyModifiers::CONTROL),
                    helper_key("Ctrl-r", KeyCode::Char('r'), KeyModifiers::CONTROL),
                    helper_key("?", KeyCode::Char('?'), KeyModifiers::NONE),
                    helper_key("q", KeyCode::Char('q'), KeyModifiers::NONE),
                ],
            )
        } else {
            (Line::from(""), Vec::new())
        };

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
        let hint_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border));
        let hint_lines = if show_global_hints {
            vec![pane_line.clone(), global_line.clone()]
        } else {
            vec![pane_line.clone(), Line::from("")]
        };
        let hint_widget = Paragraph::new(hint_lines)
            .style(Style::default().fg(theme.dimmed))
            .block(hint_block.clone());

        frame.render_widget(status_widget, chunks[0]);
        frame.render_widget(hint_widget, chunks[1]);

        let hint_inner = hint_block.inner(chunks[1]);
        self.register_helper_click_line(&pane_line, hint_inner, 0, &pane_bindings, theme);
        if show_global_hints {
            self.register_helper_click_line(&global_line, hint_inner, 1, &global_bindings, theme);
        }
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

        let footer_line = Line::from(vec![
            key_chip("Enter", theme),
            Span::styled(" save  ", Style::default().fg(theme.muted)),
            key_chip("Esc", theme),
            Span::styled(" cancel  ", Style::default().fg(theme.muted)),
            key_chip("Alt+Enter", theme),
            Span::styled(" newline", Style::default().fg(theme.muted)),
        ]);
        let footer = Paragraph::new(footer_line.clone()).style(Style::default().bg(theme.modal_bg));
        frame.render_widget(footer, sections[3]);
        self.register_helper_click_line(
            &footer_line,
            sections[3],
            0,
            &[
                helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                helper_key("Alt+Enter", KeyCode::Enter, KeyModifiers::ALT),
            ],
            theme,
        );
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

        let (footer_text, footer_bindings) = if self.ui.shell_command.running.is_some() {
            (
                Line::from(vec![
                    key_chip("Esc", theme),
                    Span::styled(" interrupt  ", Style::default().fg(theme.muted)),
                    key_chip("y", theme),
                    Span::styled(" copy output  ", Style::default().fg(theme.muted)),
                    key_chip("Backspace", theme),
                    Span::styled(" reset", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                    helper_key("y", KeyCode::Char('y'), KeyModifiers::NONE),
                    helper_key("Backspace", KeyCode::Backspace, KeyModifiers::NONE),
                ],
            )
        } else if self.ui.shell_command.finished.is_some() {
            (
                Line::from(vec![
                    key_chip("Enter", theme),
                    Span::styled(" continue  ", Style::default().fg(theme.muted)),
                    key_chip("Esc", theme),
                    Span::styled(" close  ", Style::default().fg(theme.muted)),
                    key_chip("y", theme),
                    Span::styled(" copy output  ", Style::default().fg(theme.muted)),
                    key_chip("Backspace", theme),
                    Span::styled(" reset", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                    helper_key("y", KeyCode::Char('y'), KeyModifiers::NONE),
                    helper_key("Backspace", KeyCode::Backspace, KeyModifiers::NONE),
                ],
            )
        } else {
            (
                Line::from(vec![
                    key_chip("Enter", theme),
                    Span::styled(" run  ", Style::default().fg(theme.muted)),
                    key_chip("Ctrl-r", theme),
                    Span::styled(" history search  ", Style::default().fg(theme.muted)),
                    key_chip("Esc", theme),
                    Span::styled(" close", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                    helper_key("Ctrl-r", KeyCode::Char('r'), KeyModifiers::CONTROL),
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                ],
            )
        };
        frame.render_widget(
            Paragraph::new(footer_text.clone()).style(Style::default().bg(theme.modal_bg)),
            sections[3],
        );
        self.register_helper_click_line(&footer_text, sections[3], 0, &footer_bindings, theme);
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

    pub(super) fn render_help_overlay(&mut self, frame: &mut Frame<'_>, theme: &UiTheme) {
        // Help is a modal overlay, so only help-local helper chips should be clickable.
        self.ui.helper_click_hitboxes.clear();
        let area = centered_rect(70, 62, frame.area());
        frame.render_widget(Clear, area);

        let section_style = Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD);
        let rows: Vec<(Line<'static>, Vec<HelperChipBinding>)> = vec![
            (
                Line::from(vec![Span::styled(
                    "HUNKR QUICK GUIDE",
                    Style::default()
                        .fg(theme.panel_title_fg)
                        .bg(theme.panel_title_bg)
                        .add_modifier(Modifier::BOLD),
                )]),
                Vec::new(),
            ),
            (Line::from(""), Vec::new()),
            (
                Line::from(Span::styled("Navigation", section_style)),
                Vec::new(),
            ),
            (
                Line::from(vec![
                    key_chip("1", theme),
                    Span::raw(" commits  "),
                    key_chip("2", theme),
                    Span::raw(" files  "),
                    key_chip("3", theme),
                    Span::raw(" diff  "),
                    key_chip("Tab", theme),
                    Span::raw(" cycle panes"),
                ]),
                vec![
                    helper_key("1", KeyCode::Char('1'), KeyModifiers::NONE),
                    helper_key("2", KeyCode::Char('2'), KeyModifiers::NONE),
                    helper_key("3", KeyCode::Char('3'), KeyModifiers::NONE),
                    helper_key("Tab", KeyCode::Tab, KeyModifiers::NONE),
                ],
            ),
            (
                Line::from(vec![
                    key_chip("j", theme),
                    Span::raw(" down  "),
                    key_chip("k", theme),
                    Span::raw(" up  "),
                    key_chip("Ctrl-d", theme),
                    Span::raw(" page down  "),
                    key_chip("Ctrl-u", theme),
                    Span::raw(" page up"),
                ]),
                vec![
                    helper_key("j", KeyCode::Char('j'), KeyModifiers::NONE),
                    helper_key("k", KeyCode::Char('k'), KeyModifiers::NONE),
                    helper_key("Ctrl-d", KeyCode::Char('d'), KeyModifiers::CONTROL),
                    helper_key("Ctrl-u", KeyCode::Char('u'), KeyModifiers::CONTROL),
                ],
            ),
            (
                Line::from(Span::styled("Review Flow", section_style)),
                Vec::new(),
            ),
            (
                Line::from(vec![
                    key_chip("Space", theme),
                    Span::raw(" toggle commit selection  "),
                    key_chip("e", theme),
                    Span::raw(" cycle Status Filter"),
                ]),
                vec![
                    helper_key("Space", KeyCode::Char(' '), KeyModifiers::NONE),
                    helper_key("e", KeyCode::Char('e'), KeyModifiers::NONE),
                ],
            ),
            (
                Line::from(vec![
                    key_chip("u", theme),
                    Span::raw(" mark Unreviewed  "),
                    key_chip("r", theme),
                    Span::raw(" mark Reviewed  "),
                    key_chip("i", theme),
                    Span::raw(" mark Issue Found"),
                ]),
                vec![
                    helper_key("u", KeyCode::Char('u'), KeyModifiers::NONE),
                    helper_key("r", KeyCode::Char('r'), KeyModifiers::NONE),
                    helper_key("i", KeyCode::Char('i'), KeyModifiers::NONE),
                ],
            ),
            (Line::from(Span::styled("Diff", section_style)), Vec::new()),
            (
                Line::from(vec![
                    key_chip("/", theme),
                    Span::raw(" Search  "),
                    key_chip("n", theme),
                    Span::raw(" next  "),
                    key_chip("N", theme),
                    Span::raw(" previous"),
                ]),
                vec![
                    helper_key("/", KeyCode::Char('/'), KeyModifiers::NONE),
                    helper_key("n", KeyCode::Char('n'), KeyModifiers::NONE),
                    helper_key("N", KeyCode::Char('N'), KeyModifiers::SHIFT),
                ],
            ),
            (
                Line::from(vec![
                    key_chip("v", theme),
                    Span::raw(" visual range  "),
                    key_chip("m", theme),
                    Span::raw(" add comment  "),
                    key_chip("[", theme),
                    Span::raw(" prev hunk  "),
                    key_chip("]", theme),
                    Span::raw(" next hunk"),
                ]),
                vec![
                    helper_key("v", KeyCode::Char('v'), KeyModifiers::NONE),
                    helper_key("m", KeyCode::Char('m'), KeyModifiers::NONE),
                    helper_key("[", KeyCode::Char('['), KeyModifiers::NONE),
                    helper_key("]", KeyCode::Char(']'), KeyModifiers::NONE),
                ],
            ),
            (Line::from(Span::styled("Tools", section_style)), Vec::new()),
            (
                Line::from(vec![
                    key_chip("!", theme),
                    Span::raw(" shell  "),
                    key_chip("Ctrl-w", theme),
                    Span::raw(" worktrees  "),
                    key_chip("Ctrl-r", theme),
                    Span::raw(" refresh  "),
                    key_chip("t", theme),
                    Span::raw(" toggle theme"),
                ]),
                vec![
                    helper_key("!", KeyCode::Char('!'), KeyModifiers::SHIFT),
                    helper_key("Ctrl-w", KeyCode::Char('w'), KeyModifiers::CONTROL),
                    helper_key("Ctrl-r", KeyCode::Char('r'), KeyModifiers::CONTROL),
                    helper_key("t", KeyCode::Char('t'), KeyModifiers::NONE),
                ],
            ),
            (Line::from(""), Vec::new()),
            (
                Line::from(vec![
                    Span::styled("Unreviewed", Style::default().fg(theme.unreviewed)),
                    Span::raw("  "),
                    Span::styled("Reviewed", Style::default().fg(theme.reviewed)),
                    Span::raw("  "),
                    Span::styled("Issue Found", Style::default().fg(theme.issue)),
                ]),
                Vec::new(),
            ),
            (
                Line::from(vec![
                    Span::styled("Press ", Style::default().fg(theme.muted)),
                    key_chip("Esc", theme),
                    Span::styled(" / ", Style::default().fg(theme.muted)),
                    key_chip("q", theme),
                    Span::styled(" / ", Style::default().fg(theme.muted)),
                    key_chip("?", theme),
                    Span::styled(" to close", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                    helper_key("q", KeyCode::Char('q'), KeyModifiers::NONE),
                    helper_key("?", KeyCode::Char('?'), KeyModifiers::NONE),
                ],
            ),
        ];

        let help_lines = rows
            .iter()
            .map(|(line, _)| line.clone())
            .collect::<Vec<_>>();
        let block = Block::default()
            .title(" Help ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.focus_border));
        frame.render_widget(Paragraph::new(help_lines).block(block.clone()), area);

        let inner = block.inner(area);
        for (idx, (line, bindings)) in rows.iter().enumerate() {
            self.register_helper_click_line(line, inner, idx as u16, bindings, theme);
        }
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

        let (footer_line, footer_bindings) = if self.ui.worktree_switch.search_active {
            (
                Line::from(vec![
                    key_chip("Enter", theme),
                    Span::styled(" apply ", Style::default().fg(theme.muted)),
                    key_chip("Esc", theme),
                    Span::styled(" clear ", Style::default().fg(theme.muted)),
                    key_chip("Backspace", theme),
                    Span::styled(" edit ", Style::default().fg(theme.muted)),
                    key_chip("q", theme),
                    Span::styled(" close", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                    helper_key("Backspace", KeyCode::Backspace, KeyModifiers::NONE),
                    helper_key("q", KeyCode::Char('q'), KeyModifiers::NONE),
                ],
            )
        } else {
            (
                Line::from(vec![
                    key_chip("Enter", theme),
                    Span::styled(" switch ", Style::default().fg(theme.muted)),
                    key_chip("/", theme),
                    Span::styled(" filter ", Style::default().fg(theme.muted)),
                    key_chip("r", theme),
                    Span::styled(" refresh ", Style::default().fg(theme.muted)),
                    key_chip("Esc", theme),
                    Span::styled(" close", Style::default().fg(theme.muted)),
                ]),
                vec![
                    helper_key("Enter", KeyCode::Enter, KeyModifiers::NONE),
                    helper_key("/", KeyCode::Char('/'), KeyModifiers::NONE),
                    helper_key("r", KeyCode::Char('r'), KeyModifiers::NONE),
                    helper_key("Esc", KeyCode::Esc, KeyModifiers::NONE),
                ],
            )
        };
        frame.render_widget(Paragraph::new(footer_line.clone()), sections[2]);
        self.register_helper_click_line(&footer_line, sections[2], 0, &footer_bindings, theme);
    }

    fn register_helper_click_line(
        &mut self,
        line: &Line<'_>,
        rect: ratatui::layout::Rect,
        line_offset: u16,
        bindings: &[HelperChipBinding],
        theme: &UiTheme,
    ) {
        if bindings.is_empty() || rect.width == 0 || rect.height == 0 {
            return;
        }
        let row = rect.y.saturating_add(line_offset);
        if row >= rect.y.saturating_add(rect.height) {
            return;
        }

        let chip_style = key_chip_style(theme);
        let mut cursor_x = rect.x;
        let x_limit = rect.x.saturating_add(rect.width);
        for span in &line.spans {
            if cursor_x >= x_limit {
                break;
            }
            let content = span.content.as_ref();
            let width = display_width(content) as u16;
            if span.style == chip_style {
                let label = content.trim();
                if let Some(binding) = bindings.iter().find(|binding| binding.label == label) {
                    let hitbox_width = width.min(x_limit.saturating_sub(cursor_x)).max(1);
                    self.ui.helper_click_hitboxes.push(HelperClickHitbox {
                        rect: ratatui::layout::Rect {
                            x: cursor_x,
                            y: row,
                            width: hitbox_width,
                            height: 1,
                        },
                        action: binding.action,
                    });
                }
            }
            cursor_x = cursor_x.saturating_add(width);
        }
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
