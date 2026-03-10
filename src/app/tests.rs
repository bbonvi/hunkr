use super::input::policy::theme_toggle_conflicts_with_diff_pending_op;
use super::lifecycle_input::{
    clear_commit_selection, clear_commit_visual_anchor, diff_search_repeat_direction,
};
use super::shell_command::{shell_output_copy_payload_for_rows, shell_output_index_at};
use super::state::should_hide_deleted_file_content;
use super::ui::list_panes::{
    ListLinePresenter, commit_push_chain_kinds, effective_list_top_for_selection,
};
use super::ui::style::{
    CursorSelectionPolicy, apply_row_highlight, line_with_right, pad_line_to_width,
    resolve_row_background, tint_line_background,
};
use crate::app::*;
use crate::model::{CommitDecoration, CommitDecorationKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;
use std::time::Duration;
use tempfile::tempdir;

fn commit_row(id: &str, selected: bool, status: ReviewStatus) -> CommitRow {
    CommitRow {
        info: CommitInfo {
            id: id.to_owned(),
            short_id: id.chars().take(7).collect(),
            summary: format!("summary-{id}"),
            author: "dev".to_owned(),
            timestamp: 0,
            unpushed: true,
            decorations: Vec::new(),
        },
        selected,
        status,
        is_uncommitted: false,
    }
}

fn commit_info(id: &str, unpushed: bool) -> CommitInfo {
    CommitInfo {
        id: id.to_owned(),
        short_id: id.chars().take(7).collect(),
        summary: format!("summary-{id}"),
        author: "dev".to_owned(),
        timestamp: 0,
        unpushed,
        decorations: Vec::new(),
    }
}

#[test]
fn first_open_reviewed_commit_ids_excludes_unpushed_commits() {
    let commits = vec![
        commit_info("pushed-a", false),
        commit_info("unpushed-a", true),
        commit_info("pushed-b", false),
    ];

    assert_eq!(
        first_open_reviewed_commit_ids(&commits),
        vec!["pushed-a".to_owned(), "pushed-b".to_owned()]
    );
}

#[test]
fn ignore_match_accepts_common_hunkr_variants() {
    assert!(ignore_file_contains_entry(".hunkr\n", ".hunkr"));
    assert!(ignore_file_contains_entry("/.hunkr/\n", ".hunkr"));
    assert!(ignore_file_contains_entry(".hunkr/\n", "/.hunkr/"));
    assert!(!ignore_file_contains_entry("target\n", ".hunkr"));
}

#[test]
fn append_ignore_file_entry_adds_once_and_skips_duplicates() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("info").join("exclude");
    fs::create_dir_all(path.parent().expect("exclude parent")).expect("create info dir");
    fs::write(&path, "target").expect("seed exclude");

    assert_eq!(
        append_ignore_file_entry(&path, "/.hunkr/").expect("append"),
        IgnoreFileUpdate::Added
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read after append"),
        "target\n/.hunkr/\n"
    );
    assert_eq!(
        append_ignore_file_entry(&path, ".hunkr").expect("append duplicate"),
        IgnoreFileUpdate::AlreadyPresent
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read after duplicate"),
        "target\n/.hunkr/\n"
    );
}

#[test]
fn append_ignore_file_entry_creates_parent_and_file_when_missing() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("info").join("exclude");

    assert_eq!(
        append_ignore_file_entry(&path, "/.hunkr/").expect("append"),
        IgnoreFileUpdate::Added
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read created gitignore"),
        "/.hunkr/\n"
    );
}

#[test]
fn truncate_short_strings_unchanged() {
    assert_eq!(truncate("abc", 4), "abc");
}

#[test]
fn truncate_long_strings_adds_ellipsis() {
    assert_eq!(truncate("abcdef", 4), "abc…");
}

#[test]
fn truncate_uses_terminal_cell_width_for_wide_glyphs() {
    assert_eq!(truncate("你你你", 4), "你…");
}

#[test]
fn sanitize_terminal_text_strips_csi_sequences() {
    assert_eq!(
        sanitize_terminal_text("ok \u{1b}[31mred\u{1b}[0m done"),
        "ok red done"
    );
}

#[test]
fn sanitize_terminal_text_strips_osc_sequences() {
    assert_eq!(
        sanitize_terminal_text("before\u{1b}]0;title\u{7}after"),
        "beforeafter"
    );
}

#[test]
fn sanitize_terminal_text_removes_other_control_bytes() {
    assert_eq!(sanitize_terminal_text("a\u{0}\u{8}b\tc\nd"), "ab\tc\nd");
}

#[test]
fn compose_commit_line_sanitizes_untrusted_summary_text() {
    let mut row = commit_row("abc1234", false, ReviewStatus::Unreviewed);
    row.info.summary = "clean \u{1b}[31mred\u{1b}[0m text".to_owned();
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, false);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.contains("clean red text"));
    assert!(!flattened.contains('\u{1b}'));
}

#[test]
fn compose_commit_line_keeps_concise_git_decorations() {
    let mut row = commit_row("abc1234", false, ReviewStatus::Unreviewed);
    row.info.decorations = vec![
        CommitDecoration {
            kind: CommitDecorationKind::Head,
            label: "main*".to_owned(),
        },
        CommitDecoration {
            kind: CommitDecorationKind::RemoteBranch,
            label: "origin/main".to_owned(),
        },
    ];
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(240, 3_600, &theme, false);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.contains("refs:main*,@origin/main"));
    assert!(!flattened.contains("HEAD ->"));
}

#[test]
fn commit_push_chain_kinds_mark_top_and_boundary_segments() {
    let mut top_unpushed = commit_row("u-top", false, ReviewStatus::Unreviewed);
    top_unpushed.info.unpushed = true;
    let mut middle_unpushed = commit_row("u-mid", false, ReviewStatus::Unreviewed);
    middle_unpushed.info.unpushed = true;
    let mut first_pushed = commit_row("p-first", false, ReviewStatus::Reviewed);
    first_pushed.info.unpushed = false;
    let mut pushed = commit_row("p-tail", false, ReviewStatus::Reviewed);
    pushed.info.unpushed = false;
    let rows = vec![top_unpushed, middle_unpushed, first_pushed, pushed];

    let kinds = commit_push_chain_kinds(&rows);

    assert_eq!(kinds[0], Some(CommitPushChainMarkerKind::TopUnpushed));
    assert_eq!(kinds[1], Some(CommitPushChainMarkerKind::Unpushed));
    assert_eq!(kinds[2], Some(CommitPushChainMarkerKind::Pushed));
    assert_eq!(kinds[3], Some(CommitPushChainMarkerKind::FirstPushed));
}

#[test]
fn commit_push_chain_kinds_skip_uncommitted_rows_for_top_marker() {
    let mut draft = commit_row("draft", false, ReviewStatus::Unreviewed);
    draft.is_uncommitted = true;
    draft.info.unpushed = false;
    let mut top_pushed = commit_row("p-top", false, ReviewStatus::Reviewed);
    top_pushed.info.unpushed = false;
    let mut first_unpushed = commit_row("u-first", false, ReviewStatus::Unreviewed);
    first_unpushed.info.unpushed = true;
    let rows = vec![draft, top_pushed, first_unpushed];

    let kinds = commit_push_chain_kinds(&rows);

    assert_eq!(kinds[0], None);
    assert_eq!(kinds[1], Some(CommitPushChainMarkerKind::TopPushed));
    assert_eq!(kinds[2], Some(CommitPushChainMarkerKind::FirstUnpushed));
}

#[test]
fn file_row_line_sanitizes_untrusted_path_label() {
    let row = TreeRow {
        label: "[F] src/\u{1b}[31mapp.rs".to_owned(),
        path: Some("src/app.rs".to_owned()),
        depth: 0,
        selectable: true,
        modified_ts: Some(0),
        change: None,
    };
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, false);
    let rendered = presenter.file_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.contains("[F] src/app.rs"));
    assert!(!flattened.contains('\u{1b}'));
}

#[test]
fn word_boundaries_skip_whitespace_and_symbols() {
    let text = "alpha  + beta";
    let cursor = text.len();
    assert_eq!(prev_word_boundary(text, cursor), 9);
    assert_eq!(prev_word_boundary(text, 10), 9);
    assert_eq!(next_word_boundary(text, 0), 5);
    assert_eq!(next_word_boundary(text, 5), 8);
}

#[test]
fn word_at_char_column_returns_word_under_cursor() {
    assert_eq!(
        word_at_char_column("alpha beta", 2).as_deref(),
        Some("alpha")
    );
    assert_eq!(
        word_at_char_column("alpha beta", 9).as_deref(),
        Some("beta")
    );
    assert_eq!(
        word_at_char_column("alpha beta", 99).as_deref(),
        Some("beta")
    );
    assert_eq!(word_at_char_column("alpha + beta", 6), None);
    assert_eq!(
        word_at_char_column("  43 +     pub block_cursor_col: usize,", 20).as_deref(),
        Some("block_cursor_col")
    );
}

#[test]
fn vim_word_motions_distinguish_word_and_word_semantics() {
    let text = "foo::bar baz";

    assert_eq!(vim_next_word_start_column(text, 0, false), Some(3));
    assert_eq!(vim_next_word_start_column(text, 0, true), Some(9));
    assert_eq!(vim_next_word_end_column(text, 0, false), Some(2));
    assert_eq!(vim_next_word_end_column(text, 0, true), Some(7));
    assert_eq!(vim_prev_word_start_column(text, 9, false), Some(5));
    assert_eq!(vim_prev_word_start_column(text, 9, true), Some(0));
}

#[test]
fn vim_word_end_moves_to_next_word_when_already_on_word_end() {
    assert_eq!(vim_next_word_end_column("foo bar", 2, false), Some(6));
    assert_eq!(vim_next_word_end_column("foo", 2, false), Some(2));
}

#[test]
fn line_column_helpers_cover_empty_and_whitespace_only_lines() {
    assert_eq!(line_last_char_column(""), None);
    assert_eq!(line_last_char_column("abc"), Some(2));
    assert_eq!(line_first_non_whitespace_column(""), None);
    assert_eq!(line_first_non_whitespace_column("   abc"), Some(3));
    assert_eq!(line_first_non_whitespace_column("   "), Some(0));
}

#[test]
fn delete_word_operations_respect_cursor() {
    let mut text = "alpha beta gamma".to_owned();
    let mut cursor = text.len();

    delete_prev_word(&mut text, &mut cursor);
    assert_eq!(text, "alpha beta ");
    assert_eq!(cursor, 11);

    delete_prev_word(&mut text, &mut cursor);
    assert_eq!(text, "alpha ");
    assert_eq!(cursor, 6);

    delete_next_word(&mut text, &mut cursor);
    assert_eq!(text, "alpha ");
    assert_eq!(cursor, 6);
}

#[test]
fn char_delete_handles_unicode_scalars() {
    let mut text = "A你B".to_owned();
    let mut cursor = text.len();

    delete_prev_char(&mut text, &mut cursor);
    assert_eq!(text, "A你");
    assert_eq!(cursor, "A你".len());

    delete_prev_char(&mut text, &mut cursor);
    assert_eq!(text, "A");
    assert_eq!(cursor, 1);
}

#[test]
fn ctrl_u_and_ctrl_k_style_deletes_stay_within_current_line() {
    let mut text = "one\ntwo words\nthree".to_owned();
    let mut cursor = text.find("words").expect("cursor on words");

    delete_to_line_start(&mut text, &mut cursor);
    assert_eq!(text, "one\nwords\nthree");
    assert_eq!(cursor, "one\n".len());

    cursor = text.find("rds").expect("cursor in words");
    delete_to_line_end(&mut text, &mut cursor);
    assert_eq!(text, "one\nwo\nthree");
}

#[test]
fn single_line_edit_key_supports_shell_style_hotkeys() {
    let mut text = "alpha beta".to_owned();
    let mut cursor = text.len();

    assert_eq!(
        apply_single_line_edit_key(
            &mut text,
            &mut cursor,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        ),
        SingleLineEditOutcome::CursorMoved
    );
    assert_eq!(cursor, 0);

    assert_eq!(
        apply_single_line_edit_key(
            &mut text,
            &mut cursor,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
        ),
        SingleLineEditOutcome::BufferChanged
    );
    assert_eq!(text, " beta");
    assert_eq!(cursor, 0);

    assert_eq!(
        apply_single_line_edit_key(
            &mut text,
            &mut cursor,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        ),
        SingleLineEditOutcome::NotHandled
    );
}

#[test]
fn single_line_edit_key_backspace_at_start_is_not_handled() {
    let mut text = String::new();
    let mut cursor = 0usize;
    assert_eq!(
        apply_single_line_edit_key(
            &mut text,
            &mut cursor,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        ),
        SingleLineEditOutcome::NotHandled
    );
}

#[test]
fn file_tree_builds_directories_and_files() {
    let mut tree = FileTree::default();
    tree.insert("src/app.rs", 100);
    tree.insert("src/ui/view.rs", 200);
    let nerd_theme = NerdFontTheme::default();
    let rows = tree.flattened_rows(false, &nerd_theme);

    assert!(rows.iter().any(|r| r.label.contains("[D] src")));
    assert!(rows.iter().any(|r| r.label.contains("[F] app.rs")));
    assert!(rows.iter().any(|r| r.label.contains("[D] ui")));
    assert!(rows.iter().any(|r| r.label.contains("[F] view.rs")));
}

#[test]
fn file_tree_file_labels_include_change_badges_and_line_stats() {
    let mut tree = FileTree::default();
    tree.insert_with_change(
        "src/app.rs",
        100,
        Some(FileChangeSummary {
            kind: FileChangeKind::Modified,
            old_path: None,
            additions: 4,
            deletions: 1,
        }),
    );
    let nerd_theme = NerdFontTheme::default();
    let rows = tree.flattened_rows(false, &nerd_theme);
    let row = rows
        .iter()
        .find(|row| row.path.as_deref() == Some("src/app.rs"))
        .expect("file row");
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, false);
    let rendered = presenter.file_row_line(row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.contains("[F] app.rs"));
    assert!(flattened.contains("M +4 -1"));
}

#[test]
fn file_filter_keeps_parent_directories_for_matching_files() {
    let mut tree = FileTree::default();
    tree.insert("src/app/main.rs", 100);
    tree.insert("src/lib.rs", 110);
    tree.insert("tests/main_test.rs", 120);
    let nerd_theme = NerdFontTheme::default();
    let rows = tree.flattened_rows(false, &nerd_theme);

    let visible = matching_file_indices_with_parent_dirs(&rows, "main");
    let visible_labels = visible
        .iter()
        .map(|idx| rows[*idx].label.clone())
        .collect::<Vec<_>>();

    assert!(visible_labels.iter().any(|label| label.contains("[D] src")));
    assert!(visible_labels.iter().any(|label| label.contains("[D] app")));
    assert!(
        visible_labels
            .iter()
            .any(|label| label.contains("[F] main.rs"))
    );
    assert!(
        visible_labels
            .iter()
            .any(|label| label.contains("[D] tests"))
    );
    assert!(
        visible_labels
            .iter()
            .any(|label| label.contains("[F] main_test.rs"))
    );
    assert!(
        !visible_labels
            .iter()
            .any(|label| label.contains("[F] lib.rs"))
    );
}

#[test]
fn diff_empty_state_message_is_absent_without_active_file_filter() {
    assert!(diff_empty_state_message(false, 3, 0, "").is_none());
    assert!(diff_empty_state_message(false, 3, 0, "   ").is_none());
    assert!(diff_empty_state_message(false, 0, 0, "main").is_none());
    assert!(diff_empty_state_message(false, 3, 1, "main").is_none());
    assert!(diff_empty_state_message(true, 3, 0, "main").is_none());
}

#[test]
fn rendered_file_banner_sanitizes_untrusted_path_text() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let nerd_theme = NerdFontTheme::default();
    let rendered = state::rendered_file_header_line(
        "src/\u{1b}[31mapp.rs",
        1,
        1,
        None,
        &theme,
        false,
        &nerd_theme,
    );
    let flattened = render_diff_line(&rendered, &theme)
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.contains("src/app.rs"));
    assert!(rendered.raw_text.contains("src/app.rs"));
    assert!(!flattened.contains('\u{1b}'));
    assert!(!rendered.raw_text.contains('\u{1b}'));
}

#[test]
fn deleted_file_content_is_hidden_from_diff_payload() {
    let deleted = FileChangeSummary {
        kind: FileChangeKind::Deleted,
        old_path: None,
        additions: 0,
        deletions: 42,
    };
    let modified = FileChangeSummary {
        kind: FileChangeKind::Modified,
        old_path: None,
        additions: 1,
        deletions: 1,
    };

    assert!(should_hide_deleted_file_content(Some(&deleted)));
    assert!(!should_hide_deleted_file_content(Some(&modified)));
    assert!(!should_hide_deleted_file_content(None));
}

#[test]
fn rendered_separator_line_keeps_empty_raw_text_without_placeholder_glyphs() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let separator = state::rendered_separator_line(&theme);
    let content = separator
        .line
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert_eq!(separator.raw_text.as_ref(), "");
    assert!(separator.anchor.is_none());
    assert_eq!(content, "");
}

#[test]
fn list_index_skips_border_rows() {
    let rect = ratatui::layout::Rect::new(0, 0, 10, 6);
    assert_eq!(list_index_at(0, rect, 3), None);
    assert_eq!(list_index_at(5, rect, 3), None);
    assert_eq!(list_index_at(1, rect, 3), Some(3));
}

#[test]
fn list_drag_scroll_delta_marks_top_and_bottom_edges() {
    let rect = ratatui::layout::Rect::new(0, 0, 20, 8);
    assert_eq!(list_drag_scroll_delta(1, rect, 1), -1);
    assert_eq!(list_drag_scroll_delta(4, rect, 1), 0);
    assert_eq!(list_drag_scroll_delta(6, rect, 1), 1);
}

#[test]
fn list_wheel_event_duplicate_detection_matches_pane_direction_and_interval() {
    let base = Instant::now();
    let last = Some((FocusPane::Commits, 1, base));
    let min = Duration::from_millis(45);

    assert!(list_wheel_event_is_duplicate(
        last,
        FocusPane::Commits,
        1,
        base + Duration::from_millis(20),
        min
    ));
    assert!(!list_wheel_event_is_duplicate(
        last,
        FocusPane::Commits,
        -1,
        base + Duration::from_millis(20),
        min
    ));
    assert!(!list_wheel_event_is_duplicate(
        last,
        FocusPane::Files,
        1,
        base + Duration::from_millis(20),
        min
    ));
    assert!(!list_wheel_event_is_duplicate(
        last,
        FocusPane::Commits,
        1,
        base + Duration::from_millis(60),
        min
    ));
}

#[test]
fn diff_index_maps_sticky_row_to_banner_line() {
    let rect = ratatui::layout::Rect::new(0, 0, 20, 8);
    let sticky = vec![7];
    assert_eq!(diff_index_at(1, rect, 20, &sticky), Some(7));
    assert_eq!(diff_index_at(2, rect, 20, &sticky), Some(20));
    assert_eq!(diff_index_at(3, rect, 20, &sticky), Some(21));
}

#[test]
fn diff_index_maps_multiple_sticky_rows_in_order() {
    let rect = ratatui::layout::Rect::new(0, 0, 20, 8);
    let sticky = vec![4, 9];
    assert_eq!(diff_index_at(1, rect, 20, &sticky), Some(4));
    assert_eq!(diff_index_at(2, rect, 20, &sticky), Some(9));
    assert_eq!(diff_index_at(3, rect, 20, &sticky), Some(20));
}

#[test]
fn diff_index_matches_list_behavior_without_sticky_banner() {
    let rect = ratatui::layout::Rect::new(0, 0, 20, 8);
    assert_eq!(diff_index_at(1, rect, 20, &[]), Some(20));
    assert_eq!(diff_index_at(2, rect, 20, &[]), Some(21));
    assert_eq!(diff_index_at(7, rect, 20, &[]), None);
}

#[test]
fn compose_sticky_banner_indexes_orders_file_commit_hunk() {
    let sticky = compose_sticky_banner_indexes(Some(3), Some(7), Some(12), 8);
    assert_eq!(sticky, vec![3, 7, 12]);
}

#[test]
fn compose_sticky_banner_indexes_dedupes_and_honors_viewport_budget() {
    let deduped = compose_sticky_banner_indexes(Some(3), Some(3), Some(9), 3);
    assert_eq!(deduped, vec![3, 9]);

    let capped = compose_sticky_banner_indexes(Some(3), Some(7), Some(12), 2);
    assert_eq!(capped, vec![3]);
}

#[test]
fn wrapped_line_rows_counts_soft_wrapped_height() {
    let line = Line::from(vec![Span::raw("123456789")]);
    assert_eq!(wrapped_line_rows(&line, 4), 3);
    assert_eq!(wrapped_line_rows(&line, 9), 1);
}

#[test]
fn diff_column_maps_mouse_to_inner_content_column() {
    let rect = ratatui::layout::Rect::new(10, 5, 30, 6);
    assert_eq!(diff_column_at(10, rect), 0);
    assert_eq!(diff_column_at(11, rect), 0);
    assert_eq!(diff_column_at(14, rect), 3);
    assert_eq!(diff_column_at(200, rect), 27);
}

#[test]
fn diff_column_for_rendered_code_line_has_no_hidden_left_padding() {
    let rect = ratatui::layout::Rect::new(10, 5, 60, 6);
    let line = RenderedDiffLine {
        line: Line::from(vec![Span::raw(""), Span::raw("target")]),
        raw_text: "+target".to_owned().into(),
        anchor: Some(DiffLineAnchor::new(
            "abc",
            "summary",
            "src/main.rs",
            "@@ -1,1 +1,1 @@",
            Some(12),
            Some(12345),
        )),
    };
    let content_left = rect.x + 1;

    assert_eq!(
        diff_column_at_for_rendered_line(content_left, rect, 0, Some(&line)),
        0,
        "first visible cell should map to payload column zero",
    );
    assert_eq!(
        diff_column_at_for_rendered_line(content_left + 1, rect, 0, Some(&line)),
        1,
        "next visible cell should map to payload column one",
    );
}

#[test]
fn diff_column_for_rendered_non_code_line_uses_display_column() {
    let rect = ratatui::layout::Rect::new(10, 5, 60, 6);
    let line = RenderedDiffLine {
        line: Line::from(vec![Span::raw("@@ "), Span::raw("-1 +1 @@")]),
        raw_text: "@@ -1 +1 @@".to_owned().into(),
        anchor: None,
    };

    assert_eq!(
        diff_column_at_for_rendered_line(23, rect, 0, Some(&line)),
        diff_column_at(23, rect)
    );
}

#[test]
fn diff_column_for_wrapped_row_applies_row_offset_before_raw_mapping() {
    let rect = ratatui::layout::Rect::new(10, 5, 60, 6);
    let line = RenderedDiffLine {
        line: Line::from(vec![Span::raw(""), Span::raw("target")]),
        raw_text: "+target".to_owned().into(),
        anchor: Some(DiffLineAnchor::new(
            "abc",
            "summary",
            "src/main.rs",
            "@@ -1,1 +1,1 @@",
            Some(12),
            Some(12345),
        )),
    };

    let wrapped_row_offset = 1;
    assert_eq!(
        diff_column_at_for_rendered_line(rect.x + 1, rect, wrapped_row_offset, Some(&line)),
        58,
    );
}

#[test]
fn shell_output_copy_payload_uses_visual_range_when_present() {
    let rows = vec![
        "$ git status".to_owned(),
        "On branch main".to_owned(),
        "nothing to commit".to_owned(),
    ];
    let payload = shell_output_copy_payload_for_rows(&rows, Some((1, 2))).expect("visual payload");
    assert_eq!(payload, "On branch main\nnothing to commit");
}

#[test]
fn shell_output_copy_payload_defaults_to_all_rows() {
    let rows = vec!["line a".to_owned(), "line b".to_owned()];
    let payload = shell_output_copy_payload_for_rows(&rows, None).expect("full payload");
    assert_eq!(payload, "line a\nline b");
}

#[test]
fn shell_output_copy_payload_clamps_out_of_bounds_visual_range() {
    let rows = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
    let payload =
        shell_output_copy_payload_for_rows(&rows, Some((1, 99))).expect("clamped payload");
    assert_eq!(payload, "b\nc");
}

#[test]
fn shell_output_copy_payload_preserves_trailing_blank_row() {
    let rows = vec!["$ cmd".to_owned(), "done".to_owned(), String::new()];
    let payload = shell_output_copy_payload_for_rows(&rows, None).expect("payload");
    assert_eq!(payload, "$ cmd\ndone\n");
}

#[test]
fn shell_output_index_maps_viewport_row_to_scrolled_line() {
    let rect = ratatui::layout::Rect::new(10, 5, 30, 6);
    let idx = shell_output_index_at(rect, 12, 7, 20, 100).expect("line index");
    assert_eq!(idx, 22);
}

#[test]
fn shell_output_index_clamps_to_last_line() {
    let rect = ratatui::layout::Rect::new(10, 5, 30, 6);
    let idx = shell_output_index_at(rect, 12, 9, 98, 100).expect("line index");
    assert_eq!(idx, 99);
}

#[test]
fn diff_visual_from_drag_anchor_requires_anchor() {
    assert!(diff_visual_from_drag_anchor(None, 14).is_none());
}

#[test]
fn diff_visual_from_drag_anchor_only_activates_after_cursor_moves() {
    assert!(diff_visual_from_drag_anchor(Some(14), 14).is_none());
    let visual = diff_visual_from_drag_anchor(Some(14), 17).expect("visual range");
    assert_eq!(visual.anchor, 14);
    assert_eq!(visual.origin, DiffVisualOrigin::Mouse);
}

#[test]
fn wheel_clears_only_keyboard_diff_visual_mode() {
    let keyboard = Some(DiffVisualSelection {
        anchor: 7,
        origin: DiffVisualOrigin::Keyboard,
    });
    let mouse = Some(DiffVisualSelection {
        anchor: 7,
        origin: DiffVisualOrigin::Mouse,
    });

    assert!(should_clear_diff_visual_on_wheel(keyboard));
    assert!(!should_clear_diff_visual_on_wheel(mouse));
    assert!(!should_clear_diff_visual_on_wheel(None));
}

#[test]
fn selection_copy_post_action_clears_when_visual_selection_exists() {
    let action = selection_copy_post_action(true, Some(Duration::from_millis(100)));
    assert_eq!(action, SelectionCopyPostAction::ClearNow);
}

#[test]
fn selection_copy_post_action_flashes_when_no_visual_selection() {
    let action = selection_copy_post_action(false, Some(Duration::from_millis(100)));
    assert_eq!(
        action,
        SelectionCopyPostAction::FlashThenClear(Duration::from_millis(100))
    );
}

#[test]
fn selection_copy_post_action_clears_when_no_visual_flash_policy() {
    let action = selection_copy_post_action(false, None);
    assert_eq!(action, SelectionCopyPostAction::ClearNow);
}

#[test]
fn clear_commit_visual_anchor_clears_existing_anchor() {
    let mut visual_anchor = Some(7);

    assert!(clear_commit_visual_anchor(&mut visual_anchor));
    assert_eq!(visual_anchor, None);
}

#[test]
fn clear_commit_visual_anchor_noops_when_disabled() {
    let mut visual_anchor = None;

    assert!(!clear_commit_visual_anchor(&mut visual_anchor));
    assert_eq!(visual_anchor, None);
}

#[test]
fn clear_commit_selection_clears_rows_and_anchors() {
    let mut rows = vec![
        commit_row("a1", true, ReviewStatus::Unreviewed),
        commit_row("b2", false, ReviewStatus::Reviewed),
    ];
    let mut visual_anchor = Some(1);
    let mut selection_anchor = Some(0);

    assert!(clear_commit_selection(
        &mut rows,
        &mut visual_anchor,
        &mut selection_anchor
    ));
    assert!(rows.iter().all(|row| !row.selected));
    assert_eq!(visual_anchor, None);
    assert_eq!(selection_anchor, None);
}

#[test]
fn clear_commit_selection_noops_when_already_empty() {
    let mut rows = vec![
        commit_row("a1", false, ReviewStatus::Unreviewed),
        commit_row("b2", false, ReviewStatus::Reviewed),
    ];
    let mut visual_anchor = None;
    let mut selection_anchor = None;

    assert!(!clear_commit_selection(
        &mut rows,
        &mut visual_anchor,
        &mut selection_anchor
    ));
    assert!(rows.iter().all(|row| !row.selected));
    assert_eq!(visual_anchor, None);
    assert_eq!(selection_anchor, None);
}

#[test]
fn theme_toggle_conflict_defers_t_to_diff_pending_z_op() {
    let t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
    assert!(theme_toggle_conflicts_with_diff_pending_op(
        t,
        FocusPane::Diff,
        Some(DiffPendingOp::Z)
    ));
    assert!(!theme_toggle_conflicts_with_diff_pending_op(
        t,
        FocusPane::Files,
        Some(DiffPendingOp::Z)
    ));
    assert!(!theme_toggle_conflicts_with_diff_pending_op(
        t,
        FocusPane::Diff,
        None
    ));
    assert!(!theme_toggle_conflicts_with_diff_pending_op(
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::SHIFT),
        FocusPane::Diff,
        Some(DiffPendingOp::Z)
    ));
}

#[test]
fn diff_search_repeat_direction_accepts_shifted_uppercase_n() {
    assert_eq!(
        diff_search_repeat_direction(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        Some(true)
    );
    assert_eq!(
        diff_search_repeat_direction(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT)),
        Some(false)
    );
    assert_eq!(
        diff_search_repeat_direction(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::SHIFT)),
        Some(false)
    );
}

#[test]
fn prune_diff_positions_removes_only_missing_paths() {
    let mut positions = HashMap::from([
        (
            "a.rs".to_owned(),
            DiffPosition {
                scroll: 10,
                cursor: 10,
            },
        ),
        (
            "b.rs".to_owned(),
            DiffPosition {
                scroll: 20,
                cursor: 21,
            },
        ),
        (
            "c.rs".to_owned(),
            DiffPosition {
                scroll: 30,
                cursor: 31,
            },
        ),
    ]);
    let existing = BTreeSet::from(["a.rs".to_owned(), "b.rs".to_owned()]);

    prune_diff_positions_for_missing_paths(&mut positions, &existing);

    assert_eq!(positions.len(), 2);
    let pos_a = positions.get("a.rs").expect("a.rs");
    assert_eq!(pos_a.scroll, 10);
    assert_eq!(pos_a.cursor, 10);

    let pos_b = positions.get("b.rs").expect("b.rs");
    assert_eq!(pos_b.scroll, 20);
    assert_eq!(pos_b.cursor, 21);
    assert!(!positions.contains_key("c.rs"));
}

#[test]
fn prune_diff_positions_keeps_existing_paths_even_if_content_changed() {
    let mut positions = HashMap::from([(
        "src/lib.rs".to_owned(),
        DiffPosition {
            scroll: 42,
            cursor: 45,
        },
    )]);
    let existing = BTreeSet::from(["src/lib.rs".to_owned()]);

    prune_diff_positions_for_missing_paths(&mut positions, &existing);

    let pos = positions.get("src/lib.rs").expect("src/lib.rs");
    assert_eq!(pos.scroll, 42);
    assert_eq!(pos.cursor, 45);
}

#[test]
fn pending_anchor_resolves_cursor_and_top_after_insertions() {
    let anchor = DiffLineAnchor::new(
        "abc123",
        "summary",
        "src/lib.rs",
        "@@ -1,1 +1,1 @@",
        Some(1),
        Some(1),
    );
    let old_lines = vec![
        RenderedDiffLine {
            line: Line::from("context-a"),
            raw_text: "context-a".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("+target"),
            raw_text: "+target".to_owned().into(),
            anchor: Some(anchor.clone()),
        },
        RenderedDiffLine {
            line: Line::from("context-b"),
            raw_text: "context-b".to_owned().into(),
            anchor: None,
        },
    ];
    let pending = capture_pending_diff_view_anchor(
        &old_lines,
        DiffPosition {
            scroll: 0,
            cursor: 1,
        },
    )
    .expect("pending");

    let new_lines = vec![
        RenderedDiffLine {
            line: Line::from("inserted"),
            raw_text: "inserted".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("context-a"),
            raw_text: "context-a".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("+target"),
            raw_text: "+target".to_owned().into(),
            anchor: Some(anchor),
        },
        RenderedDiffLine {
            line: Line::from("context-b"),
            raw_text: "context-b".to_owned().into(),
            anchor: None,
        },
    ];

    let cursor_idx = find_index_for_line_locator(&new_lines, &pending.cursor_line).expect("cursor");
    let top_idx = find_index_for_line_locator(&new_lines, &pending.top_line).expect("top");
    assert_eq!(cursor_idx, 2);
    assert_eq!(top_idx, 1);
}

#[test]
fn line_locator_falls_back_to_raw_text_occurrence() {
    let old_lines = vec![
        RenderedDiffLine {
            line: Line::from("repeat"),
            raw_text: "repeat".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("repeat"),
            raw_text: "repeat".to_owned().into(),
            anchor: None,
        },
    ];
    let locator = capture_pending_diff_view_anchor(
        &old_lines,
        DiffPosition {
            scroll: 1,
            cursor: 1,
        },
    )
    .expect("pending")
    .cursor_line;

    let new_lines = vec![
        RenderedDiffLine {
            line: Line::from("repeat"),
            raw_text: "repeat".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("other"),
            raw_text: "other".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("repeat"),
            raw_text: "repeat".to_owned().into(),
            anchor: None,
        },
    ];

    let idx = find_index_for_line_locator(&new_lines, &locator).expect("match");
    assert_eq!(idx, 2);
}

#[test]
fn line_locator_disambiguates_duplicate_anchor_with_text_occurrence() {
    let anchor = DiffLineAnchor::new(
        "abc123",
        "summary",
        "src/lib.rs",
        "@@ -1,1 +1,1 @@",
        Some(1),
        Some(1),
    );
    let old_lines = vec![
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned().into(),
            anchor: Some(anchor.clone()),
        },
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned().into(),
            anchor: Some(anchor.clone()),
        },
    ];
    let locator = capture_pending_diff_view_anchor(
        &old_lines,
        DiffPosition {
            scroll: 1,
            cursor: 1,
        },
    )
    .expect("pending")
    .cursor_line;

    let new_lines = vec![
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned().into(),
            anchor: Some(anchor.clone()),
        },
        RenderedDiffLine {
            line: Line::from("x"),
            raw_text: "x".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned().into(),
            anchor: Some(anchor),
        },
    ];

    let idx = find_index_for_line_locator(&new_lines, &locator).expect("match");
    assert_eq!(idx, 3);
}

#[test]
fn line_locator_handles_empty_raw_text_occurrence() {
    let old_lines = vec![
        RenderedDiffLine {
            line: Line::from(""),
            raw_text: String::new().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from(""),
            raw_text: String::new().into(),
            anchor: None,
        },
    ];
    let locator = capture_pending_diff_view_anchor(
        &old_lines,
        DiffPosition {
            scroll: 1,
            cursor: 1,
        },
    )
    .expect("pending")
    .cursor_line;

    let new_lines = vec![
        RenderedDiffLine {
            line: Line::from(""),
            raw_text: String::new().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("x"),
            raw_text: "x".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from(""),
            raw_text: String::new().into(),
            anchor: None,
        },
    ];

    let idx = find_index_for_line_locator(&new_lines, &locator).expect("match");
    assert_eq!(idx, 2);
}

#[test]
fn contains_checks_bounds() {
    let rect = ratatui::layout::Rect::new(5, 5, 4, 3);
    assert!(contains(rect, 5, 5));
    assert!(contains(rect, 8, 7));
    assert!(!contains(rect, 9, 7));
    assert!(!contains(rect, 4, 5));
}

#[test]
fn selected_ids_are_reported_oldest_first() {
    let rows = vec![
        commit_row("newest", true, ReviewStatus::Unreviewed),
        commit_row("middle", false, ReviewStatus::Reviewed),
        commit_row("oldest", true, ReviewStatus::Unreviewed),
    ];
    assert_eq!(
        selected_ids_oldest_first(&rows),
        vec!["oldest".to_owned(), "newest".to_owned()]
    );
}

#[test]
fn selected_ids_skip_uncommitted_entry() {
    let mut rows = vec![
        commit_row("newest", true, ReviewStatus::Unreviewed),
        commit_row("oldest", true, ReviewStatus::Unreviewed),
    ];
    rows.insert(
        0,
        CommitRow {
            info: CommitInfo {
                id: UNCOMMITTED_COMMIT_ID.to_owned(),
                short_id: UNCOMMITTED_COMMIT_SHORT.to_owned(),
                summary: UNCOMMITTED_COMMIT_SUMMARY.to_owned(),
                author: "local".to_owned(),
                timestamp: 0,
                unpushed: false,
                decorations: Vec::new(),
            },
            selected: true,
            status: ReviewStatus::Unreviewed,
            is_uncommitted: true,
        },
    );

    assert_eq!(
        selected_ids_oldest_first(&rows),
        vec!["oldest".to_owned(), "newest".to_owned()]
    );
}

#[test]
fn merge_aggregate_diff_combines_file_change_metadata_for_same_path() {
    let mut base = AggregatedDiff {
        files: BTreeMap::from([(
            "src/lib.rs".to_owned(),
            FilePatch {
                path: "src/lib.rs".to_owned(),
                hunks: Vec::new(),
            },
        )]),
        file_changes: BTreeMap::from([(
            "src/lib.rs".to_owned(),
            FileChangeSummary {
                kind: FileChangeKind::Renamed,
                old_path: Some("src/old_lib.rs".to_owned()),
                additions: 2,
                deletions: 1,
            },
        )]),
    };
    let next = AggregatedDiff {
        files: BTreeMap::from([(
            "src/lib.rs".to_owned(),
            FilePatch {
                path: "src/lib.rs".to_owned(),
                hunks: Vec::new(),
            },
        )]),
        file_changes: BTreeMap::from([(
            "src/lib.rs".to_owned(),
            FileChangeSummary {
                kind: FileChangeKind::Modified,
                old_path: None,
                additions: 3,
                deletions: 4,
            },
        )]),
    };

    merge_aggregate_diff(&mut base, next);
    let merged = base.file_changes.get("src/lib.rs").expect("merged change");
    assert_eq!(merged.kind, FileChangeKind::Renamed);
    assert_eq!(merged.old_path.as_deref(), Some("src/old_lib.rs"));
    assert_eq!(merged.additions, 5);
    assert_eq!(merged.deletions, 5);
}

#[test]
fn restore_list_index_prefers_previous_commit_id() {
    let rows = vec![
        commit_row("c1", false, ReviewStatus::Unreviewed),
        commit_row("c2", false, ReviewStatus::Unreviewed),
        commit_row("c3", false, ReviewStatus::Unreviewed),
    ];

    assert_eq!(
        restore_list_index_by_commit_id(&rows, Some("c2"), Some(0)),
        Some(1)
    );
}

#[test]
fn restore_list_index_falls_back_and_clamps() {
    let rows = vec![
        commit_row("c1", false, ReviewStatus::Unreviewed),
        commit_row("c2", false, ReviewStatus::Unreviewed),
    ];

    assert_eq!(
        restore_list_index_by_commit_id(&rows, Some("missing"), Some(9)),
        Some(1)
    );
}

#[test]
fn restore_list_index_returns_none_for_empty_rows() {
    assert_eq!(
        restore_list_index_by_commit_id(&[], Some("c1"), Some(0)),
        None
    );
}

#[test]
fn range_selection_handles_reverse_bounds() {
    let mut rows = vec![
        commit_row("a", false, ReviewStatus::Unreviewed),
        commit_row("b", false, ReviewStatus::Reviewed),
        commit_row("c", false, ReviewStatus::Unreviewed),
    ];
    apply_range_selection(&mut rows, 2, 0);
    assert!(rows.iter().all(|row| row.selected));
}

#[test]
fn commit_mouse_selection_mode_defaults_to_replace_without_shift() {
    assert_eq!(
        commit_mouse_selection_mode(KeyModifiers::NONE),
        CommitMouseSelectionMode::Replace
    );
    assert_eq!(
        commit_mouse_selection_mode(KeyModifiers::CONTROL),
        CommitMouseSelectionMode::Replace
    );
}

#[test]
fn commit_mouse_selection_mode_uses_range_with_shift() {
    assert_eq!(
        commit_mouse_selection_mode(KeyModifiers::SHIFT),
        CommitMouseSelectionMode::Range
    );
    assert_eq!(
        commit_mouse_selection_mode(KeyModifiers::SHIFT | KeyModifiers::CONTROL),
        CommitMouseSelectionMode::Range
    );
}

#[test]
fn range_anchor_for_space_prefers_existing_anchor() {
    let rows = vec![
        commit_row("a", false, ReviewStatus::Unreviewed),
        commit_row("b", true, ReviewStatus::Unreviewed),
        commit_row("c", false, ReviewStatus::Unreviewed),
    ];

    assert_eq!(range_anchor_for_space(&rows, Some(1), 2), 1);
}

#[test]
fn range_anchor_for_space_uses_closest_selected_when_anchor_absent() {
    let rows = vec![
        commit_row("a", true, ReviewStatus::Unreviewed),
        commit_row("b", false, ReviewStatus::Unreviewed),
        commit_row("c", false, ReviewStatus::Unreviewed),
        commit_row("d", true, ReviewStatus::Unreviewed),
    ];

    assert_eq!(range_anchor_for_space(&rows, None, 2), 3);
}

#[test]
fn range_anchor_for_space_falls_back_to_cursor_without_selection() {
    let rows = vec![
        commit_row("a", false, ReviewStatus::Unreviewed),
        commit_row("b", false, ReviewStatus::Unreviewed),
    ];

    assert_eq!(range_anchor_for_space(&rows, None, 1), 1);
}

#[test]
fn select_only_index_keeps_only_target_selected() {
    let mut rows = vec![
        commit_row("a", true, ReviewStatus::Unreviewed),
        commit_row("b", true, ReviewStatus::Reviewed),
        commit_row("c", false, ReviewStatus::Unreviewed),
    ];

    select_only_index(&mut rows, 1);

    assert!(!rows[0].selected);
    assert!(rows[1].selected);
    assert!(!rows[2].selected);
}

#[test]
fn apply_status_ids_changes_only_targeted_commits() {
    let mut rows = vec![
        commit_row("a", true, ReviewStatus::Unreviewed),
        commit_row("b", true, ReviewStatus::Reviewed),
    ];
    let ids = BTreeSet::from(["b".to_owned()]);

    apply_status_ids(&mut rows, &ids, ReviewStatus::IssueFound);

    assert_eq!(rows[0].status, ReviewStatus::Unreviewed);
    assert_eq!(rows[1].status, ReviewStatus::IssueFound);
}

#[test]
fn reviewed_status_keeps_selection_intact() {
    let mut rows = vec![
        commit_row("a", true, ReviewStatus::Unreviewed),
        commit_row("b", true, ReviewStatus::IssueFound),
        commit_row("c", false, ReviewStatus::Unreviewed),
    ];
    let ids = BTreeSet::from(["b".to_owned(), "c".to_owned()]);

    apply_status_transition(&mut rows, &ids, ReviewStatus::Reviewed);

    assert!(rows[0].selected);
    assert!(rows[1].selected);
    assert!(!rows[2].selected);
    assert_eq!(rows[1].status, ReviewStatus::Reviewed);
    assert_eq!(rows[2].status, ReviewStatus::Reviewed);
}

#[test]
fn issue_found_status_keeps_selection_intact() {
    let mut rows = vec![
        commit_row("a", true, ReviewStatus::Unreviewed),
        commit_row("b", false, ReviewStatus::Reviewed),
    ];
    let ids = BTreeSet::from(["a".to_owned()]);

    apply_status_transition(&mut rows, &ids, ReviewStatus::IssueFound);

    assert!(rows[0].selected);
    assert_eq!(rows[0].status, ReviewStatus::IssueFound);
}

#[test]
fn line_with_right_keeps_right_text_visible() {
    let rendered = line_with_right(
        "[F] file.rs".to_owned(),
        Style::default(),
        "3h".to_owned(),
        Style::default(),
        24,
    );
    let flattened = rendered
        .spans
        .iter()
        .map(|s| s.content.to_string())
        .collect::<String>();
    assert!(flattened.ends_with("3h"));
}

#[test]
fn pad_line_to_width_extends_line_with_matching_style() {
    let style = Style::default()
        .fg(Color::Rgb(220, 220, 220))
        .bg(Color::Rgb(40, 60, 80));
    let line = Line::from(Span::styled("abc", style));

    let padded = pad_line_to_width(&line, 6, Style::default());
    let flattened = padded
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert_eq!(flattened, "abc   ");
    assert_eq!(
        padded.spans.last().map(|span| span.style.bg),
        Some(style.bg)
    );
}

#[test]
fn pad_line_to_width_uses_fallback_background_when_missing() {
    let line = Line::from(Span::styled("", Style::default().fg(Color::Yellow)));
    let fallback = Style::default().bg(Color::Rgb(12, 34, 56));

    let padded = pad_line_to_width(&line, 4, fallback);

    assert_eq!(
        padded.spans.last().and_then(|span| span.style.bg),
        fallback.bg
    );
}

#[test]
fn pad_line_to_width_noops_when_line_already_fits() {
    let line = Line::from("abcdef");
    let padded = pad_line_to_width(&line, 6, Style::default().bg(Color::Blue));
    let flattened = padded
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert_eq!(flattened, "abcdef");
}

#[test]
fn pad_line_to_width_uses_display_width_for_wide_glyphs() {
    let line = Line::from(Span::styled("中", Style::default().bg(Color::Green)));
    let padded = pad_line_to_width(&line, 4, Style::default());
    let flattened = padded
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert_eq!(flattened, "中  ");
}

#[test]
fn resolve_row_background_cursor_wins_policy_overrides_selection() {
    let selection_bg = Color::Rgb(10, 20, 30);
    let cursor_bg = Color::Rgb(40, 50, 60);
    let bg = resolve_row_background(
        true,
        true,
        selection_bg,
        cursor_bg,
        CursorSelectionPolicy::CursorWins,
    );
    assert_eq!(bg, Some(cursor_bg));
}

#[test]
fn resolve_row_background_blend_policy_combines_selection_and_cursor() {
    let selection_bg = Color::Rgb(10, 20, 30);
    let cursor_bg = Color::Rgb(220, 210, 200);
    let bg = resolve_row_background(
        true,
        true,
        selection_bg,
        cursor_bg,
        CursorSelectionPolicy::BlendCursorOverSelection { weight: 170 },
    );
    assert!(bg.is_some_and(|blended| blended != selection_bg && blended != cursor_bg));
}

#[test]
fn apply_row_highlight_cursor_wins_and_pads_row() {
    let cursor_bg = Color::Rgb(30, 70, 120);
    let line = Line::from(Span::styled("ab", Style::default().fg(Color::White)));
    let highlighted = apply_row_highlight(
        &line,
        5,
        true,
        true,
        Color::Rgb(10, 20, 30),
        cursor_bg,
        CursorSelectionPolicy::CursorWins,
    );
    let flattened = highlighted
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert_eq!(flattened, "ab   ");
    assert!(
        highlighted
            .spans
            .iter()
            .all(|span| span.style.bg == Some(cursor_bg))
    );
}

#[test]
fn apply_row_highlight_visual_only_keeps_original_width() {
    let line = Line::from(Span::styled("ab", Style::default().fg(Color::White)));
    let highlighted = apply_row_highlight(
        &line,
        5,
        true,
        false,
        Color::Rgb(90, 80, 70),
        Color::Rgb(20, 30, 40),
        CursorSelectionPolicy::CursorWins,
    );
    let flattened = highlighted
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert_eq!(flattened, "ab");
}

#[test]
fn commit_status_filter_cycles_in_expected_order() {
    assert_eq!(
        CommitStatusFilter::All.next(),
        CommitStatusFilter::UnreviewedOrIssueFound
    );
    assert_eq!(
        CommitStatusFilter::UnreviewedOrIssueFound.next(),
        CommitStatusFilter::Reviewed
    );
    assert_eq!(CommitStatusFilter::Reviewed.next(), CommitStatusFilter::All);
}

#[test]
fn commit_status_filter_groups_rows_correctly() {
    let unreviewed = commit_row("a", false, ReviewStatus::Unreviewed);
    let issue = commit_row("b", false, ReviewStatus::IssueFound);
    let reviewed = commit_row("c", false, ReviewStatus::Reviewed);
    let reviewed_peer = commit_row("d", false, ReviewStatus::Reviewed);
    let mut draft = commit_row("wip", false, ReviewStatus::Unreviewed);
    draft.is_uncommitted = true;

    assert!(CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&unreviewed));
    assert!(CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&issue));
    assert!(CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&draft));
    assert!(!CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&reviewed));
    assert!(!CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&reviewed_peer));

    assert!(CommitStatusFilter::Reviewed.matches_row(&reviewed));
    assert!(CommitStatusFilter::Reviewed.matches_row(&reviewed_peer));
    assert!(!CommitStatusFilter::Reviewed.matches_row(&unreviewed));
    assert!(!CommitStatusFilter::Reviewed.matches_row(&issue));
    assert!(CommitStatusFilter::Reviewed.matches_row(&draft));
}

#[test]
fn commit_search_matches_text_and_status_case_insensitively() {
    let row = commit_row("abc1234", false, ReviewStatus::IssueFound);
    assert!(commit_row_matches_query(&row, "ABC"));
    assert!(commit_row_matches_query(&row, "issue_found"));
    assert!(commit_row_matches_query(&row, "SUMMARY-abc1234"));
    assert!(!commit_row_matches_query(&row, "missing-value"));
}

#[test]
fn commit_search_matches_git_decorations() {
    let mut row = commit_row("abc1234", false, ReviewStatus::Unreviewed);
    row.info.decorations = vec![CommitDecoration {
        kind: CommitDecorationKind::RemoteBranch,
        label: "origin/main".to_owned(),
    }];

    assert!(commit_row_matches_query(&row, "origin/main"));
    assert!(commit_row_matches_query(&row, "origin"));
}

#[test]
fn uncommitted_row_bypasses_commit_query_filter() {
    let mut draft = commit_row("wip", false, ReviewStatus::Unreviewed);
    draft.is_uncommitted = true;
    let committed = commit_row("abc1234", false, ReviewStatus::IssueFound);

    assert!(commit_row_matches_filter_query(&draft, "no-match"));
    assert!(!commit_row_matches_filter_query(&committed, "no-match"));
    assert!(commit_row_matches_filter_query(&committed, "abc"));
}

#[test]
fn switching_status_filter_deselects_hidden_commits() {
    let mut unreviewed = commit_row("a", true, ReviewStatus::Unreviewed);
    let reviewed = commit_row("b", true, ReviewStatus::Reviewed);
    let mut draft = commit_row("wip", true, ReviewStatus::Unreviewed);
    draft.is_uncommitted = true;
    unreviewed.is_uncommitted = false;
    let mut rows = vec![unreviewed, reviewed, draft];

    let deselected =
        deselect_rows_outside_status_filter(&mut rows, CommitStatusFilter::UnreviewedOrIssueFound);
    assert_eq!(deselected, 1);
    assert!(rows[0].selected);
    assert!(!rows[1].selected);
    assert!(rows[2].selected);

    let deselected = deselect_rows_outside_status_filter(&mut rows, CommitStatusFilter::Reviewed);
    assert_eq!(deselected, 1);
    assert!(!rows[0].selected);
    assert!(!rows[1].selected);
    assert!(rows[2].selected);
}

#[test]
fn selected_rows_hidden_count_tracks_active_filter() {
    let mut rows = vec![
        commit_row("a", true, ReviewStatus::Unreviewed),
        commit_row("b", true, ReviewStatus::Reviewed),
        commit_row("c", false, ReviewStatus::Reviewed),
        commit_row("wip", true, ReviewStatus::Unreviewed),
    ];
    rows[3].is_uncommitted = true;
    rows[2].selected = true;

    assert_eq!(
        selected_rows_hidden_by_status_filter(&rows, CommitStatusFilter::All),
        0
    );
    assert_eq!(
        selected_rows_hidden_by_status_filter(&rows, CommitStatusFilter::UnreviewedOrIssueFound),
        2
    );
    assert_eq!(
        selected_rows_hidden_by_status_filter(&rows, CommitStatusFilter::Reviewed),
        1
    );
}

#[test]
fn next_poll_timeout_uses_nearest_deadline() {
    let auto_refresh_every = Duration::from_secs(4);
    let relative_time_redraw_every = Duration::from_secs(30);
    let timeout = next_poll_timeout(
        auto_refresh_every,
        relative_time_redraw_every,
        Duration::from_secs(1),
        Duration::from_millis(100),
        None,
        Some(Duration::from_millis(150)),
    );
    assert_eq!(timeout, Duration::from_millis(150));
}

#[test]
fn next_poll_timeout_zero_when_any_deadline_elapsed() {
    let auto_refresh_every = Duration::from_secs(4);
    let relative_time_redraw_every = Duration::from_secs(30);
    let timeout = next_poll_timeout(
        auto_refresh_every,
        relative_time_redraw_every,
        Duration::from_secs(5),
        Duration::from_secs(1),
        None,
        Some(Duration::from_secs(1)),
    );
    assert_eq!(timeout, Duration::from_secs(0));
}

#[test]
fn next_poll_timeout_after_refresh_uses_refresh_deadline_without_theme_fallback() {
    let auto_refresh_every = Duration::from_secs(4);
    let relative_time_redraw_every = Duration::from_secs(30);
    let timeout = next_poll_timeout(
        auto_refresh_every,
        relative_time_redraw_every,
        Duration::from_secs(0),
        Duration::from_secs(0),
        None,
        None,
    );
    assert_eq!(timeout, auto_refresh_every);
}

#[test]
fn next_poll_timeout_honors_selection_rebuild_deadline() {
    let auto_refresh_every = Duration::from_secs(4);
    let relative_time_redraw_every = Duration::from_secs(30);
    let timeout = next_poll_timeout(
        auto_refresh_every,
        relative_time_redraw_every,
        Duration::from_secs(0),
        Duration::from_secs(0),
        Some(Duration::from_millis(80)),
        Some(Duration::from_secs(1)),
    );
    assert_eq!(timeout, Duration::from_millis(80));
}

#[test]
fn viewport_scroll_preserves_cursor_offset() {
    let next = scrolled_diff_position_preserving_offset(
        DiffPosition {
            scroll: 10,
            cursor: 14,
        },
        3,
        200,
        250,
    );
    assert_eq!(next.scroll, 13);
    assert_eq!(next.cursor, 17);
}

#[test]
fn diff_scroll_with_scrolloff_keeps_scroll_when_cursor_stays_within_gutter() {
    let next_scroll = diff_scroll_with_scrolloff(26, 10, 20, 3);
    assert_eq!(next_scroll, 10);
}

#[test]
fn diff_scroll_with_scrolloff_scrolls_when_cursor_enters_bottom_gutter() {
    let next_scroll = diff_scroll_with_scrolloff(27, 10, 20, 3);
    assert_eq!(next_scroll, 11);
}

#[test]
fn diff_scroll_with_scrolloff_scrolls_when_cursor_enters_top_gutter() {
    let next_scroll = diff_scroll_with_scrolloff(12, 10, 20, 3);
    assert_eq!(next_scroll, 9);
}

#[test]
fn diff_scroll_with_scrolloff_clamps_for_small_viewports() {
    let next_scroll = diff_scroll_with_scrolloff(13, 10, 4, 3);
    assert_eq!(next_scroll, 11);
}

#[test]
fn diff_scroll_with_scrolloff_handles_single_row_viewports() {
    let next_scroll = diff_scroll_with_scrolloff(9, 5, 1, 3);
    assert_eq!(next_scroll, 9);
}

#[test]
fn effective_list_top_for_selection_keeps_existing_top_when_cursor_stays_visible() {
    assert_eq!(effective_list_top_for_selection(Some(12), 10, 8, 40), 10);
}

#[test]
fn effective_list_top_for_selection_scrolls_immediately_for_jump_below_viewport() {
    assert_eq!(effective_list_top_for_selection(Some(30), 10, 8, 40), 23);
}

#[test]
fn effective_list_top_for_selection_scrolls_immediately_for_jump_above_viewport() {
    assert_eq!(effective_list_top_for_selection(Some(2), 10, 8, 40), 2);
}

#[test]
fn effective_list_top_for_selection_clamps_to_bottom_limit() {
    assert_eq!(effective_list_top_for_selection(Some(99), 70, 8, 75), 67);
}

#[test]
fn viewport_scroll_clamps_at_top() {
    let next = scrolled_diff_position_preserving_offset(
        DiffPosition {
            scroll: 2,
            cursor: 5,
        },
        -10,
        200,
        250,
    );
    assert_eq!(next.scroll, 0);
    assert_eq!(next.cursor, 3);
}

#[test]
fn viewport_scroll_clamps_at_bottom_and_content_end() {
    let next = scrolled_diff_position_preserving_offset(
        DiffPosition {
            scroll: 90,
            cursor: 98,
        },
        10,
        95,
        99,
    );
    assert_eq!(next.scroll, 95);
    assert_eq!(next.cursor, 99);
}

#[test]
fn list_scroll_preserves_cursor_row_after_filter_mutation() {
    let next_top =
        list_scroll_preserving_cursor_to_top_offset(Some(14), 10, Some(7)).expect("top offset");
    assert_eq!(next_top, 3);
}

#[test]
fn list_scroll_preservation_clamps_to_top_when_next_cursor_is_near_start() {
    let next_top =
        list_scroll_preserving_cursor_to_top_offset(Some(6), 3, Some(2)).expect("top offset");
    assert_eq!(next_top, 0);
}

#[test]
fn commit_banner_renders_only_when_commit_changes() {
    let commits = ["a", "a", "b", "b", "a"];
    let mut previous: Option<&str> = None;
    let rendered = commits
        .iter()
        .map(|current| {
            let show = should_render_commit_banner(previous, current);
            previous = Some(current);
            show
        })
        .collect::<Vec<_>>();

    assert_eq!(rendered, vec![true, false, true, false, true]);
}

#[test]
fn commit_anchor_marker_is_detected() {
    let commit_anchor = DiffLineAnchor::new(
        "abc1234",
        "summary",
        "src/lib.rs",
        COMMIT_ANCHOR_HEADER,
        None,
        None,
    );
    let hunk_anchor = commit_anchor.with_hunk_header("@@ -1,1 +1,1 @@", Some(1), Some(1));

    assert!(is_commit_line_anchor(&commit_anchor));
    assert!(!is_commit_line_anchor(&hunk_anchor));
}

#[test]
fn diff_line_anchor_match_requires_exact_line_mapping() {
    let base = DiffLineAnchor::new(
        "abc",
        "summary",
        "src/lib.rs",
        "@@ -1,1 +1,1 @@",
        Some(1),
        Some(1),
    );
    let same = base.clone();
    let mut different = base.clone();
    different.new_lineno = Some(2);

    assert!(diff_line_anchor_matches(&base, &same));
    assert!(!diff_line_anchor_matches(&base, &different));
}

#[test]
fn diff_search_wraps_forward() {
    let lines = vec![
        RenderedDiffLine {
            line: Line::from("alpha"),
            raw_text: "alpha".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("beta"),
            raw_text: "beta".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("gamma"),
            raw_text: "gamma".to_owned().into(),
            anchor: None,
        },
    ];

    let found = find_diff_match_from_cursor(&lines, "alp", true, 2, 0);
    assert_eq!(
        found.map(|entry| (entry.line_index, entry.char_col)),
        Some((0, 0))
    );
}

#[test]
fn diff_search_wraps_backward() {
    let lines = vec![
        RenderedDiffLine {
            line: Line::from("alpha"),
            raw_text: "alpha".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("beta"),
            raw_text: "beta".to_owned().into(),
            anchor: None,
        },
        RenderedDiffLine {
            line: Line::from("gamma"),
            raw_text: "gamma".to_owned().into(),
            anchor: None,
        },
    ];

    let found = find_diff_match_from_cursor(&lines, "gam", false, 0, 0);
    assert_eq!(
        found.map(|entry| (entry.line_index, entry.char_col)),
        Some((2, 0))
    );
}

#[test]
fn diff_search_steps_between_occurrences_on_same_line() {
    let lines = vec![RenderedDiffLine {
        line: Line::from("alpha alpha beta"),
        raw_text: "alpha alpha beta".to_owned().into(),
        anchor: None,
    }];

    let found = find_diff_match_from_cursor(&lines, "alpha", true, 0, 0);
    assert_eq!(
        found.map(|entry| (entry.line_index, entry.char_col)),
        Some((0, 6))
    );

    let wrapped = find_diff_match_from_cursor(&lines, "alpha", true, 0, 6);
    assert_eq!(
        wrapped.map(|entry| (entry.line_index, entry.char_col)),
        Some((0, 0))
    );
}

#[test]
fn diff_search_steps_backward_between_occurrences_on_same_line() {
    let lines = vec![RenderedDiffLine {
        line: Line::from("alpha alpha beta"),
        raw_text: "alpha alpha beta".to_owned().into(),
        anchor: None,
    }];

    let found = find_diff_match_from_cursor(&lines, "alpha", false, 0, 6);
    assert_eq!(
        found.map(|entry| (entry.line_index, entry.char_col)),
        Some((0, 0))
    );
}

#[test]
fn hunk_header_detection_ignores_commit_banner() {
    let commit_anchor = DiffLineAnchor::new(
        "abc1234",
        "summary",
        "src/lib.rs",
        COMMIT_ANCHOR_HEADER,
        None,
        None,
    );
    let hunk_anchor = commit_anchor.with_hunk_header("@@ -1,1 +1,1 @@", Some(1), Some(1));

    let commit_line = RenderedDiffLine {
        line: Line::from("---- commit abc1234 summary"),
        raw_text: "---- commit abc1234 summary".to_owned().into(),
        anchor: Some(commit_anchor),
    };
    let hunk_line = RenderedDiffLine {
        line: Line::from("@@ -1,1 +1,1 @@"),
        raw_text: "@@ -1,1 +1,1 @@".to_owned().into(),
        anchor: Some(hunk_anchor),
    };

    assert!(!is_hunk_header_line(&commit_line));
    assert!(is_hunk_header_line(&hunk_line));
}

#[test]
fn cursor_tint_blends_existing_diff_background() {
    let line = Line::from(vec![Span::styled(
        "+ let x = 1",
        Style::default().bg(Color::Rgb(19, 51, 30)),
    )]);
    let tinted = tint_line_background(&line, Color::Rgb(52, 52, 62), true);
    let bg = tinted.spans[0].style.bg.expect("bg");

    assert_ne!(bg, Color::Rgb(19, 51, 30));
    assert_ne!(bg, Color::Rgb(52, 52, 62));
}
