use super::lifecycle_input::{
    clear_commit_selection, clear_commit_visual_anchor, diff_search_repeat_direction,
};
use super::lifecycle_render::{
    PaneCycleDirection, footer_mode_label, help_overlay_close_key, pane_focus_cycle_direction,
    theme_toggle_conflicts_with_diff_pending_op,
};
use super::shell_command::{shell_output_copy_payload_for_rows, shell_output_index_at};
use super::state::{format_uncommitted_summary, should_hide_deleted_file_content};
use super::ui::diff_pane::scrollbar_thumb;
use super::ui::list_panes::{
    ListLinePresenter, commit_push_chain_kinds, commit_status_filter_spans,
    effective_list_top_for_selection,
};
use super::ui::style::{
    CursorSelectionPolicy, apply_row_highlight, line_with_right, list_content_width,
    pad_line_to_width, resolve_row_background, status_style, tint_line_background,
};
use super::*;
use crate::model::{CommitDecoration, CommitDecorationKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;
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

fn sample_comment(start: CommentAnchor, end: CommentAnchor, text: &str) -> ReviewComment {
    ReviewComment {
        id: 7,
        target: CommentTarget {
            kind: CommentTargetKind::Hunk,
            start,
            end,
            commits: BTreeSet::from(["abc".to_owned()]),
            selected_lines: vec!["+x".to_owned()],
        },
        text: text.to_owned(),
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        updated_at: "2026-01-01T00:00:00Z".to_owned(),
    }
}

fn sample_commit_comment(anchor: CommentAnchor, text: &str) -> ReviewComment {
    ReviewComment {
        id: 9,
        target: CommentTarget {
            kind: CommentTargetKind::Commit,
            start: anchor.clone(),
            end: anchor.clone(),
            commits: BTreeSet::from([anchor.commit_id.clone()]),
            selected_lines: vec!["---- commit abc1234 add parser (1m)".to_owned()],
        },
        text: text.to_owned(),
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        updated_at: "2026-01-01T00:00:00Z".to_owned(),
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
fn gitignore_match_accepts_common_hunkr_variants() {
    assert!(gitignore_contains_entry(".hunkr\n", ".hunkr"));
    assert!(gitignore_contains_entry("/.hunkr/\n", ".hunkr"));
    assert!(gitignore_contains_entry(".hunkr/\n", "/.hunkr"));
    assert!(!gitignore_contains_entry("target\n", ".hunkr"));
}

#[test]
fn append_gitignore_entry_adds_once_and_skips_duplicates() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join(".gitignore");
    fs::write(&path, "target").expect("seed gitignore");

    assert_eq!(
        append_gitignore_entry(&path, ".hunkr").expect("append"),
        GitignoreUpdate::Added
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read after append"),
        "target\n.hunkr\n"
    );
    assert_eq!(
        append_gitignore_entry(&path, ".hunkr").expect("append duplicate"),
        GitignoreUpdate::AlreadyPresent
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read after duplicate"),
        "target\n.hunkr\n"
    );
}

#[test]
fn append_gitignore_entry_creates_file_when_missing() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join(".gitignore");

    assert_eq!(
        append_gitignore_entry(&path, ".hunkr").expect("append"),
        GitignoreUpdate::Added
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read created gitignore"),
        ".hunkr\n"
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
fn compose_commit_line_renders_git_decorations() {
    let mut row = commit_row("abc1234", false, ReviewStatus::Unreviewed);
    row.info.decorations = vec![
        CommitDecoration {
            kind: CommitDecorationKind::Head,
            label: "HEAD -> main".to_owned(),
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

    assert!(flattened.contains("refs:HEAD -> main"));
}

#[test]
fn compose_commit_line_places_status_markers_after_git_decorations() {
    let mut row = commit_row("abc1234", false, ReviewStatus::Unreviewed);
    row.info.decorations = vec![
        CommitDecoration {
            kind: CommitDecorationKind::Head,
            label: "HEAD -> main".to_owned(),
        },
        CommitDecoration {
            kind: CommitDecorationKind::LocalBranch,
            label: "main".to_owned(),
        },
    ];
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(180, 3_600, &theme, true);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    let refs_idx = flattened.find(" HEAD -> main, main").expect("refs");
    let status_idx = flattened.find("").expect("status badge");
    let unpushed_idx = flattened.find("󰜛").expect("unpushed marker");
    assert!(status_idx > refs_idx);
    assert!(unpushed_idx > status_idx);
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
fn compose_commit_line_allows_wider_git_decorations_before_truncating() {
    let mut row = commit_row("abc1234", false, ReviewStatus::Unreviewed);
    row.info.decorations = vec![CommitDecoration {
        kind: CommitDecorationKind::Head,
        label: "HEAD -> long-mainline-branch-name".to_owned(),
    }];
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(240, 3_600, &theme, false);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.contains("refs:HEAD -> long-mainline-branch-name"));
}

#[test]
fn compose_commit_line_uses_unpadded_age_without_column_hint() {
    let row = commit_row("abc1234", false, ReviewStatus::Unreviewed);
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, true);
    let rendered = presenter.commit_row_line(&row);
    let age_span = rendered.spans.last().expect("age span");

    assert_eq!(age_span.content.as_ref(), "1h");
}

#[test]
fn compose_commit_line_left_pads_age_with_column_hint() {
    let row = commit_row("abc1234", false, ReviewStatus::Unreviewed);
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, true).with_age_column_width(3);
    let rendered = presenter.commit_row_line(&row);
    let age_span = rendered.spans.last().expect("age span");

    assert_eq!(age_span.content.as_ref(), " 1h");
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
fn comment_cursor_line_and_col_track_multiline_positions() {
    let text = "one\ntwo\nthree";
    let cursor = text.find("three").expect("three start");
    assert_eq!(comment_cursor_line_col(text, cursor), (3, 1));
}

#[test]
fn comment_modal_lines_includes_cursor_marker() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let rendered = comment_modal_lines("abc", 1, None, 4, &theme);
    let flattened = rendered.lines[0]
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(!flattened.contains('|'));
    assert!(
        rendered.lines[0]
            .spans
            .iter()
            .any(|span| span.content == "b" && span.style.bg == Some(theme.modal_cursor_bg))
    );
    assert!(flattened.contains("ab"));
    assert!(flattened.contains("c"));
    assert!(rendered.text_offset > 0);
}

#[test]
fn delete_selection_range_cuts_selected_text() {
    let mut text = "alpha beta".to_owned();
    let mut cursor = text.len();
    let mut selection = Some((2, 8));

    assert!(delete_selection_range(
        &mut text,
        &mut cursor,
        &mut selection
    ));
    assert_eq!(text, "alta");
    assert_eq!(cursor, 2);
    assert!(selection.is_none());
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
fn vertical_cursor_movement_keeps_column_when_possible() {
    let text = "abcd\nxy\npqrst";
    let base = text.find("cd").expect("cd start") + 2;
    let up = move_cursor_up(text, base);
    let down = move_cursor_down(text, up);

    assert_eq!(up, base);
    assert_eq!(down, text.find("xy").expect("xy start") + 2);
    assert_eq!(
        move_cursor_down(text, down),
        text.find("pq").expect("pq start") + 2
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
fn file_tree_uses_file_icons_when_nerd_fonts_enabled() {
    let mut tree = FileTree::default();
    tree.insert("src/app.rs", 100);
    tree.insert("README.md", 200);
    let nerd_theme = NerdFontTheme::default();
    let rows = tree.flattened_rows(true, &nerd_theme);

    assert!(rows.iter().any(|r| r.label.contains(" src")));
    assert!(rows.iter().any(|r| r.label.contains(" app.rs")));
    assert!(rows.iter().any(|r| r.label.contains(" README.md")));
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
fn deleted_file_badge_uses_trash_icon_with_minus_count() {
    let badge = format_file_change_badge(
        &FileChangeSummary {
            kind: FileChangeKind::Deleted,
            old_path: None,
            additions: 0,
            deletions: 802,
        },
        true,
    );
    assert!(badge.contains(""));
    assert!(badge.contains("-802"));
    assert!(badge.contains("-802"));
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
fn diff_empty_state_message_explains_when_file_filter_hides_everything() {
    let message = diff_empty_state_message(false, 3, 0, "main")
        .expect("filtered-out file message should be present");
    assert_eq!(
        message,
        "Diff hidden: file tree filter /main hides all 3 changed file(s)"
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
fn rendered_file_banner_gates_nerd_icon_and_keeps_raw_text_stable() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let nerd_theme = NerdFontTheme::default();
    let plain =
        state::rendered_file_header_line("src/app.rs", 1, 2, None, &theme, false, &nerd_theme);
    let nerd =
        state::rendered_file_header_line("src/app.rs", 1, 2, None, &theme, true, &nerd_theme);

    let plain_text = plain
        .line
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();
    let nerd_text = nerd
        .line
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(plain_text.contains("src/app.rs"));
    assert!(nerd_text.contains(" src/app.rs"));
    assert_eq!(plain.raw_text, "==== file 1/2: src/app.rs ====");
    assert_eq!(nerd.raw_text, plain.raw_text);
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
    let flattened = rendered
        .line
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
fn rendered_file_banner_includes_change_badge_and_rename_source() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let nerd_theme = NerdFontTheme::default();
    let rendered = state::rendered_file_header_line(
        "src/new.rs",
        1,
        1,
        Some(&FileChangeSummary {
            kind: FileChangeKind::Renamed,
            old_path: Some("src/old.rs".to_owned()),
            additions: 12,
            deletions: 3,
        }),
        &theme,
        false,
        &nerd_theme,
    );
    let flattened = rendered
        .line
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.contains("(from src/old.rs)"));
    assert!(flattened.contains("· R +12 -3"));
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

    assert_eq!(separator.raw_text, "");
    assert!(separator.anchor.is_none());
    assert_eq!(separator.comment_id, None);
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
fn diff_column_maps_mouse_to_inner_content_column() {
    let rect = ratatui::layout::Rect::new(10, 5, 30, 6);
    assert_eq!(diff_column_at(10, rect), 0);
    assert_eq!(diff_column_at(11, rect), 0);
    assert_eq!(diff_column_at(14, rect), 3);
    assert_eq!(diff_column_at(200, rect), 27);
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
fn clipboard_copy_status_formats_success_and_failure() {
    let success = clipboard_copy_status(Ok("xclip"), "2 diff line(s)", "diff selection");
    assert_eq!(success, "Copied 2 diff line(s) via xclip");

    let failure = clipboard_copy_status(
        Err(anyhow::anyhow!("no backend available")),
        "2 diff line(s)",
        "diff selection",
    );
    assert!(failure.starts_with("Clipboard unavailable for diff selection ("));
}

#[test]
fn footer_mode_label_uses_visual_when_commit_range_active() {
    assert_eq!(footer_mode_label(InputMode::Normal, true, false), "VISUAL");
}

#[test]
fn footer_mode_label_uses_visual_when_diff_range_active() {
    assert_eq!(footer_mode_label(InputMode::Normal, false, true), "VISUAL");
}

#[test]
fn footer_mode_label_prioritizes_modal_states() {
    assert_eq!(
        footer_mode_label(InputMode::DiffSearch, true, true),
        "SEARCH"
    );
    assert_eq!(
        footer_mode_label(InputMode::CommentCreate, true, true),
        "COMMENT"
    );
    assert_eq!(
        footer_mode_label(InputMode::ShellCommand, true, true),
        "SHELL"
    );
    assert_eq!(
        footer_mode_label(InputMode::WorktreeSwitch, true, true),
        "WORKTREE"
    );
}

#[test]
fn help_overlay_close_key_matches_modal_close_actions() {
    assert!(help_overlay_close_key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::NONE
    )));
    assert!(help_overlay_close_key(KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE
    )));
    assert!(help_overlay_close_key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE
    )));
    assert!(!help_overlay_close_key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::CONTROL
    )));
    assert!(!help_overlay_close_key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::NONE
    )));
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
fn pane_focus_cycle_direction_supports_tab_and_shift_tab_variants() {
    assert_eq!(
        pane_focus_cycle_direction(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Some(PaneCycleDirection::Next)
    );
    assert_eq!(
        pane_focus_cycle_direction(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)),
        Some(PaneCycleDirection::Prev)
    );
    assert_eq!(
        pane_focus_cycle_direction(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)),
        Some(PaneCycleDirection::Prev)
    );
    assert_eq!(
        pane_focus_cycle_direction(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
        Some(PaneCycleDirection::Prev)
    );
    assert_eq!(
        pane_focus_cycle_direction(KeyEvent::new(
            KeyCode::Tab,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )),
        None
    );
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
fn absolute_nav_target_accepts_home_end_and_vim_aliases() {
    assert_eq!(
        absolute_nav_target(KeyCode::Home),
        Some(AbsoluteNavTarget::Start)
    );
    assert_eq!(
        absolute_nav_target(KeyCode::End),
        Some(AbsoluteNavTarget::End)
    );
    assert_eq!(
        absolute_nav_target(KeyCode::Char('g')),
        Some(AbsoluteNavTarget::Start)
    );
    assert_eq!(
        absolute_nav_target(KeyCode::Char('G')),
        Some(AbsoluteNavTarget::End)
    );
}

#[test]
fn absolute_nav_target_ignores_paging_keys() {
    assert_eq!(absolute_nav_target(KeyCode::PageUp), None);
    assert_eq!(absolute_nav_target(KeyCode::PageDown), None);
}

#[test]
fn list_content_width_accounts_for_border_and_highlight_symbol() {
    assert_eq!(list_content_width(20, 3), 15);
    assert_eq!(list_content_width(4, 3), 1);
}

#[test]
fn list_content_width_expands_when_nerd_highlight_symbol_hidden() {
    assert_eq!(
        list_content_width(20, list_highlight_symbol_width(true)),
        18
    );
}

#[test]
fn scrollbar_thumb_fills_viewport_when_content_fits() {
    assert_eq!(scrollbar_thumb(10, 20, 0), (0, 20));
}

#[test]
fn scrollbar_thumb_moves_from_top_to_bottom() {
    let (start_top, len) = scrollbar_thumb(100, 20, 0);
    let (start_bottom, len_bottom) = scrollbar_thumb(100, 20, 80);
    assert_eq!(len, 4);
    assert_eq!(len_bottom, 4);
    assert_eq!(start_top, 0);
    assert_eq!(start_bottom, 16);
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
    let anchor = CommentAnchor {
        commit_id: "abc123".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
    };
    let old_lines = vec![
        RenderedDiffLine {
            line: Line::from("context-a"),
            raw_text: "context-a".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("+target"),
            raw_text: "+target".to_owned(),
            anchor: Some(anchor.clone()),
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("context-b"),
            raw_text: "context-b".to_owned(),
            anchor: None,
            comment_id: None,
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
            raw_text: "inserted".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("context-a"),
            raw_text: "context-a".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("+target"),
            raw_text: "+target".to_owned(),
            anchor: Some(anchor),
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("context-b"),
            raw_text: "context-b".to_owned(),
            anchor: None,
            comment_id: None,
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
            raw_text: "repeat".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("repeat"),
            raw_text: "repeat".to_owned(),
            anchor: None,
            comment_id: None,
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
            raw_text: "repeat".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("other"),
            raw_text: "other".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("repeat"),
            raw_text: "repeat".to_owned(),
            anchor: None,
            comment_id: None,
        },
    ];

    let idx = find_index_for_line_locator(&new_lines, &locator).expect("match");
    assert_eq!(idx, 2);
}

#[test]
fn line_locator_disambiguates_duplicate_anchor_with_text_occurrence() {
    let anchor = CommentAnchor {
        commit_id: "abc123".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
    };
    let old_lines = vec![
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned(),
            anchor: Some(anchor.clone()),
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned(),
            anchor: Some(anchor.clone()),
            comment_id: None,
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
            raw_text: "dup".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned(),
            anchor: Some(anchor.clone()),
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("x"),
            raw_text: "x".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("dup"),
            raw_text: "dup".to_owned(),
            anchor: Some(anchor),
            comment_id: None,
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
            raw_text: String::new(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from(""),
            raw_text: String::new(),
            anchor: None,
            comment_id: None,
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
            raw_text: String::new(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("x"),
            raw_text: "x".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from(""),
            raw_text: String::new(),
            anchor: None,
            comment_id: None,
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
fn toggle_range_from_baseline_inverts_only_rows_inside_range() {
    let baseline = vec![true, false, true, false];
    let mut rows = vec![
        commit_row("a", baseline[0], ReviewStatus::Unreviewed),
        commit_row("b", baseline[1], ReviewStatus::Unreviewed),
        commit_row("c", baseline[2], ReviewStatus::Unreviewed),
        commit_row("d", baseline[3], ReviewStatus::Unreviewed),
    ];

    apply_toggle_range_from_baseline(&mut rows, &baseline, 1, 2);

    assert!(rows[0].selected);
    assert!(rows[1].selected);
    assert!(!rows[2].selected);
    assert!(!rows[3].selected);
}

#[test]
fn toggle_range_from_baseline_handles_reverse_bounds() {
    let baseline = vec![false, false, true, true];
    let mut rows = vec![
        commit_row("a", baseline[0], ReviewStatus::Unreviewed),
        commit_row("b", baseline[1], ReviewStatus::Unreviewed),
        commit_row("c", baseline[2], ReviewStatus::Unreviewed),
        commit_row("d", baseline[3], ReviewStatus::Unreviewed),
    ];

    apply_toggle_range_from_baseline(&mut rows, &baseline, 3, 1);

    assert!(!rows[0].selected);
    assert!(rows[1].selected);
    assert!(!rows[2].selected);
    assert!(!rows[3].selected);
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
fn compose_commit_line_preserves_age_column_on_narrow_width() {
    let row = commit_row("abc1234", false, ReviewStatus::IssueFound);
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(24, 3_600, &theme, false);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|s| s.content.to_string())
        .collect::<String>();
    assert!(flattened.ends_with("1h"));
}

#[test]
fn compose_commit_line_marks_selected_rows() {
    let row = commit_row("abc1234", true, ReviewStatus::Unreviewed);
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, false);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.starts_with("[x] "));
}

#[test]
fn compose_commit_line_uses_bold_status_badges() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, false);

    let unreviewed = presenter.commit_row_line(&commit_row("u1", false, ReviewStatus::Unreviewed));
    let issue = presenter.commit_row_line(&commit_row("i1", false, ReviewStatus::IssueFound));
    let reviewed = presenter.commit_row_line(&commit_row("r1", false, ReviewStatus::Reviewed));

    let unreviewed_badge = unreviewed
        .spans
        .iter()
        .find(|span| span.content == "U")
        .expect("U badge");
    let issue_badge = issue
        .spans
        .iter()
        .find(|span| span.content == "I")
        .expect("I badge");
    let reviewed_badge = reviewed
        .spans
        .iter()
        .find(|span| span.content == "R")
        .expect("R badge");

    assert!(unreviewed_badge.style.add_modifier.contains(Modifier::BOLD));
    assert!(issue_badge.style.add_modifier.contains(Modifier::BOLD));
    assert!(reviewed_badge.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn compose_commit_line_uses_nerd_symbols_when_enabled() {
    let row = commit_row("abc1234", true, ReviewStatus::Unreviewed);
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, true);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.starts_with(" "));
    assert!(flattened.contains(""));
    assert!(flattened.contains(" 󰜛"));
    assert!(flattened.ends_with("1h"));
}

#[test]
fn compose_uncommitted_line_uses_nerd_draft_badge() {
    let row = CommitRow {
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
    };
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme, true);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.contains("[ DRAFT]"));
    assert!(flattened.ends_with("draft"));
}

#[test]
fn format_uncommitted_summary_includes_file_count() {
    assert_eq!(
        format_uncommitted_summary(0),
        "Uncommitted changes (0 files)"
    );
    assert_eq!(
        format_uncommitted_summary(1),
        "Uncommitted changes (1 file)"
    );
    assert_eq!(
        format_uncommitted_summary(7),
        "Uncommitted changes (7 files)"
    );
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
    assert!(bg.is_some_and(|resolved| resolved != selection_bg && resolved != cursor_bg));
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
fn status_badges_use_exact_workflow_labels() {
    assert_eq!(status_short_label(ReviewStatus::Unreviewed), "UNREVIEWED");
    assert_eq!(status_short_label(ReviewStatus::Reviewed), "REVIEWED");
    assert_eq!(status_short_label(ReviewStatus::IssueFound), "ISSUE_FOUND");
    assert_eq!(status_short_label(ReviewStatus::Resolved), "RESOLVED");
}

#[test]
fn status_filter_title_spans_keep_all_muted() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let spans = commit_status_filter_spans(CommitStatusFilter::All, &theme);

    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content.as_ref(), "all");
    assert_eq!(spans[0].style, Style::default().fg(theme.muted));
}

#[test]
fn status_filter_title_spans_use_status_colors_for_grouped_filters() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);

    let unreviewed_issue =
        commit_status_filter_spans(CommitStatusFilter::UnreviewedOrIssueFound, &theme);
    assert_eq!(unreviewed_issue.len(), 3);
    assert_eq!(unreviewed_issue[0].content.as_ref(), "unreviewed");
    assert_eq!(
        unreviewed_issue[0].style,
        status_style(ReviewStatus::Unreviewed, &theme)
    );
    assert_eq!(unreviewed_issue[1].content.as_ref(), "|");
    assert_eq!(unreviewed_issue[1].style, Style::default().fg(theme.muted));
    assert_eq!(unreviewed_issue[2].content.as_ref(), "issue_found");
    assert_eq!(
        unreviewed_issue[2].style,
        status_style(ReviewStatus::IssueFound, &theme)
    );

    let reviewed_resolved =
        commit_status_filter_spans(CommitStatusFilter::ReviewedOrResolved, &theme);
    assert_eq!(reviewed_resolved.len(), 3);
    assert_eq!(reviewed_resolved[0].content.as_ref(), "reviewed");
    assert_eq!(
        reviewed_resolved[0].style,
        status_style(ReviewStatus::Reviewed, &theme)
    );
    assert_eq!(reviewed_resolved[1].content.as_ref(), "|");
    assert_eq!(reviewed_resolved[1].style, Style::default().fg(theme.muted));
    assert_eq!(reviewed_resolved[2].content.as_ref(), "resolved");
    assert_eq!(
        reviewed_resolved[2].style,
        status_style(ReviewStatus::Resolved, &theme)
    );
}

#[test]
fn commit_status_filter_cycles_in_expected_order() {
    assert_eq!(
        CommitStatusFilter::All.next(),
        CommitStatusFilter::UnreviewedOrIssueFound
    );
    assert_eq!(
        CommitStatusFilter::UnreviewedOrIssueFound.next(),
        CommitStatusFilter::ReviewedOrResolved
    );
    assert_eq!(
        CommitStatusFilter::ReviewedOrResolved.next(),
        CommitStatusFilter::All
    );
}

#[test]
fn commit_mouse_selection_mode_matches_modifier_intent() {
    assert_eq!(
        commit_mouse_selection_mode(KeyModifiers::NONE),
        CommitMouseSelectionMode::Replace
    );
    assert_eq!(
        commit_mouse_selection_mode(KeyModifiers::CONTROL),
        CommitMouseSelectionMode::Toggle
    );
    assert_eq!(
        commit_mouse_selection_mode(KeyModifiers::SUPER),
        CommitMouseSelectionMode::Toggle
    );
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
fn commit_status_filter_groups_rows_correctly() {
    let unreviewed = commit_row("a", false, ReviewStatus::Unreviewed);
    let issue = commit_row("b", false, ReviewStatus::IssueFound);
    let reviewed = commit_row("c", false, ReviewStatus::Reviewed);
    let resolved = commit_row("d", false, ReviewStatus::Resolved);
    let mut draft = commit_row("wip", false, ReviewStatus::Unreviewed);
    draft.is_uncommitted = true;

    assert!(CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&unreviewed));
    assert!(CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&issue));
    assert!(CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&draft));
    assert!(!CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&reviewed));
    assert!(!CommitStatusFilter::UnreviewedOrIssueFound.matches_row(&resolved));

    assert!(CommitStatusFilter::ReviewedOrResolved.matches_row(&reviewed));
    assert!(CommitStatusFilter::ReviewedOrResolved.matches_row(&resolved));
    assert!(!CommitStatusFilter::ReviewedOrResolved.matches_row(&unreviewed));
    assert!(!CommitStatusFilter::ReviewedOrResolved.matches_row(&issue));
    assert!(CommitStatusFilter::ReviewedOrResolved.matches_row(&draft));
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

    let deselected =
        deselect_rows_outside_status_filter(&mut rows, CommitStatusFilter::ReviewedOrResolved);
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
        commit_row("c", false, ReviewStatus::Resolved),
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
        selected_rows_hidden_by_status_filter(&rows, CommitStatusFilter::ReviewedOrResolved),
        1
    );
}

#[test]
fn relative_time_formats_expected_units() {
    assert_eq!(format_relative_time(100, 130), "30s");
    assert_eq!(format_relative_time(100, 220), "2m");
    assert_eq!(format_relative_time(100, 3700), "1h");
}

#[test]
fn next_poll_timeout_uses_nearest_deadline() {
    let timeout = next_poll_timeout(Duration::from_secs(1), Duration::from_secs(10), None);
    assert_eq!(timeout, Duration::from_secs(3));
}

#[test]
fn next_poll_timeout_zero_when_any_deadline_elapsed() {
    let timeout = next_poll_timeout(Duration::from_secs(5), Duration::from_secs(1), None);
    assert_eq!(timeout, Duration::from_secs(0));
}

#[test]
fn next_poll_timeout_after_refresh_waits_for_auto_refresh_window() {
    let timeout = next_poll_timeout(Duration::from_secs(0), Duration::from_secs(0), None);
    assert_eq!(timeout, AUTO_REFRESH_EVERY);
}

#[test]
fn next_poll_timeout_honors_selection_rebuild_deadline() {
    let timeout = next_poll_timeout(
        Duration::from_secs(0),
        Duration::from_secs(0),
        Some(Duration::from_millis(80)),
    );
    assert_eq!(timeout, Duration::from_millis(80));
}

#[test]
fn h_and_l_cycle_all_panes() {
    assert_eq!(focus_with_h(FocusPane::Commits), FocusPane::Diff);
    assert_eq!(focus_with_h(FocusPane::Files), FocusPane::Commits);
    assert_eq!(focus_with_h(FocusPane::Diff), FocusPane::Files);
    assert_eq!(focus_with_l(FocusPane::Commits), FocusPane::Files);
    assert_eq!(focus_with_l(FocusPane::Files), FocusPane::Diff);
    assert_eq!(focus_with_l(FocusPane::Diff), FocusPane::Commits);
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
    let commit_anchor = CommentAnchor {
        commit_id: "abc1234".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
        old_lineno: None,
        new_lineno: None,
    };
    let hunk_anchor = CommentAnchor {
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
        ..commit_anchor.clone()
    };

    assert!(is_commit_anchor(&commit_anchor));
    assert!(!is_commit_anchor(&hunk_anchor));
}

#[test]
fn comment_anchor_match_requires_exact_line_mapping() {
    let base = CommentAnchor {
        commit_id: "abc".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
    };
    let same = base.clone();
    let mut different = base.clone();
    different.new_lineno = Some(2);

    assert!(comment_anchor_matches(&base, &same));
    assert!(!comment_anchor_matches(&base, &different));
}

#[test]
fn comment_location_formats_range_when_bounds_differ() {
    let start = CommentAnchor {
        commit_id: "abc".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
    };
    let end = CommentAnchor {
        old_lineno: Some(3),
        new_lineno: Some(4),
        ..start.clone()
    };
    let comment = sample_comment(start, end, "check this");

    assert_eq!(
        comment_location_label(&comment),
        "range old 1/new 1 -> old 3/new 4"
    );
}

#[test]
fn comment_location_formats_commit_targets() {
    let anchor = CommentAnchor {
        commit_id: "abc1234deadbeef".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
        old_lineno: None,
        new_lineno: None,
    };
    let comment = sample_commit_comment(anchor, "commit-level note");

    assert_eq!(comment_location_label(&comment), "commit abc1234");
}

#[test]
fn comment_commit_membership_uses_commit_anchor() {
    let anchor = CommentAnchor {
        commit_id: "abc1234deadbeef".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
        old_lineno: None,
        new_lineno: None,
    };
    let comment = sample_commit_comment(anchor, "commit-level note");

    assert!(comment_targets_commit_end(
        &comment,
        "src/lib.rs",
        "abc1234deadbeef"
    ));
    assert!(!comment_targets_commit_end(
        &comment,
        "src/lib.rs",
        "fffffff"
    ));
}

#[test]
fn comment_hunk_membership_uses_end_anchor() {
    let start = CommentAnchor {
        commit_id: "start".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
    };
    let end = CommentAnchor {
        commit_id: "end".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -10,1 +10,1 @@".to_owned(),
        old_lineno: Some(10),
        new_lineno: Some(10),
    };
    let mut comment = sample_comment(start, end.clone(), "multi hunk");
    comment.id = 8;
    comment.target.commits = BTreeSet::from(["start".to_owned(), "end".to_owned()]);

    assert!(comment_targets_hunk_end(
        &comment,
        "src/lib.rs",
        "end",
        "@@ -10,1 +10,1 @@"
    ));
    assert!(!comment_targets_hunk_end(
        &comment,
        "src/lib.rs",
        "start",
        "@@ -1,1 +1,1 @@"
    ));
    assert!(!comment_targets_hunk_end(
        &comment,
        "src/other.rs",
        "end",
        "@@ -10,1 +10,1 @@"
    ));
}

#[test]
fn push_comment_lines_sets_comment_id_on_each_rendered_row() {
    let start = CommentAnchor {
        commit_id: "abc".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
    };
    let end = CommentAnchor {
        old_lineno: Some(2),
        new_lineno: Some(2),
        ..start.clone()
    };
    let comment = sample_comment(start, end, "line one\nline two");
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let mut rendered = Vec::new();

    push_comment_lines(&mut rendered, &comment, &theme, 0);

    assert_eq!(rendered.len(), 3);
    assert!(
        rendered
            .iter()
            .all(|line| line.comment_id == Some(comment.id))
    );
}

#[test]
fn push_comment_lines_sanitizes_comment_text() {
    let start = CommentAnchor {
        commit_id: "abc".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
    };
    let end = CommentAnchor {
        old_lineno: Some(2),
        new_lineno: Some(2),
        ..start.clone()
    };
    let comment = sample_comment(start, end, "one \u{1b}[31mred\u{1b}[0m\nnext\u{0}x");
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let mut rendered = Vec::new();

    push_comment_lines(&mut rendered, &comment, &theme, 0);

    let flattened = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .map(|span| span.content.to_string())
        .collect::<String>();
    assert!(!flattened.contains('\u{1b}'));
    assert!(!rendered[0].raw_text.contains('\u{1b}'));
    assert!(flattened.contains("one red"));
    assert!(flattened.contains("nextx"));
}

#[test]
fn push_comment_lines_for_anchor_injects_once_on_matching_end_anchor() {
    let start = CommentAnchor {
        commit_id: "abc".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
    };
    let end = CommentAnchor {
        old_lineno: Some(2),
        new_lineno: Some(2),
        ..start.clone()
    };
    let comment = sample_comment(start.clone(), end.clone(), "line one");
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let mut rendered = Vec::new();
    let comments = vec![&comment];
    let mut injected = BTreeSet::new();

    push_comment_lines_for_anchor(&mut rendered, &comments, &mut injected, &start, &theme, 0);
    assert!(rendered.is_empty());

    push_comment_lines_for_anchor(&mut rendered, &comments, &mut injected, &end, &theme, 0);
    let inserted_rows = rendered.len();
    assert!(inserted_rows > 0);

    push_comment_lines_for_anchor(&mut rendered, &comments, &mut injected, &end, &theme, 0);
    assert_eq!(rendered.len(), inserted_rows);
}

#[test]
fn diff_search_wraps_forward() {
    let lines = vec![
        RenderedDiffLine {
            line: Line::from("alpha"),
            raw_text: "alpha".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("beta"),
            raw_text: "beta".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("gamma"),
            raw_text: "gamma".to_owned(),
            anchor: None,
            comment_id: None,
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
            raw_text: "alpha".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("beta"),
            raw_text: "beta".to_owned(),
            anchor: None,
            comment_id: None,
        },
        RenderedDiffLine {
            line: Line::from("gamma"),
            raw_text: "gamma".to_owned(),
            anchor: None,
            comment_id: None,
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
        raw_text: "alpha alpha beta".to_owned(),
        anchor: None,
        comment_id: None,
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
        raw_text: "alpha alpha beta".to_owned(),
        anchor: None,
        comment_id: None,
    }];

    let found = find_diff_match_from_cursor(&lines, "alpha", false, 0, 6);
    assert_eq!(
        found.map(|entry| (entry.line_index, entry.char_col)),
        Some((0, 0))
    );
}

#[test]
fn hunk_header_detection_ignores_commit_banner() {
    let commit_anchor = CommentAnchor {
        commit_id: "abc1234".to_owned(),
        commit_summary: "summary".to_owned(),
        file_path: "src/lib.rs".to_owned(),
        hunk_header: COMMIT_ANCHOR_HEADER.to_owned(),
        old_lineno: None,
        new_lineno: None,
    };
    let hunk_anchor = CommentAnchor {
        hunk_header: "@@ -1,1 +1,1 @@".to_owned(),
        old_lineno: Some(1),
        new_lineno: Some(1),
        ..commit_anchor.clone()
    };

    let commit_line = RenderedDiffLine {
        line: Line::from("---- commit abc1234 summary"),
        raw_text: "---- commit abc1234 summary".to_owned(),
        anchor: Some(commit_anchor),
        comment_id: None,
    };
    let hunk_line = RenderedDiffLine {
        line: Line::from("@@ -1,1 +1,1 @@"),
        raw_text: "@@ -1,1 +1,1 @@".to_owned(),
        anchor: Some(hunk_anchor),
        comment_id: None,
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
