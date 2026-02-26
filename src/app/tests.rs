use super::ui::diff_pane::{scrollbar_thumb, tint_line_background};
use super::ui::list_panes::ListLinePresenter;
use super::ui::style::{line_with_right, list_content_width, list_row_style};
use super::*;

fn commit_row(id: &str, selected: bool, status: ReviewStatus) -> CommitRow {
    CommitRow {
        info: CommitInfo {
            id: id.to_owned(),
            short_id: id.chars().take(7).collect(),
            summary: format!("summary-{id}"),
            author: "dev".to_owned(),
            timestamp: 0,
            unpushed: true,
        },
        selected,
        status,
        is_uncommitted: false,
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
            selected_lines: vec!["---- commit abc1234 add parser (1m ago)".to_owned()],
        },
        text: text.to_owned(),
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        updated_at: "2026-01-01T00:00:00Z".to_owned(),
    }
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
fn file_tree_builds_directories_and_files() {
    let mut tree = FileTree::default();
    tree.insert("src/app.rs", 100);
    tree.insert("src/ui/view.rs", 200);
    let rows = tree.flattened_rows();

    assert!(rows.iter().any(|r| r.label.contains("[D] src")));
    assert!(rows.iter().any(|r| r.label.contains("[F] app.rs")));
    assert!(rows.iter().any(|r| r.label.contains("[D] ui")));
    assert!(rows.iter().any(|r| r.label.contains("[F] view.rs")));
}

#[test]
fn list_index_skips_border_rows() {
    let rect = ratatui::layout::Rect::new(0, 0, 10, 6);
    assert_eq!(list_index_at(0, rect, 3), None);
    assert_eq!(list_index_at(5, rect, 3), None);
    assert_eq!(list_index_at(1, rect, 3), Some(3));
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
fn list_content_width_accounts_for_border_and_highlight_symbol() {
    assert_eq!(list_content_width(20), 15);
    assert_eq!(list_content_width(4), 1);
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
fn reviewed_status_auto_deselects_targeted_commits() {
    let mut rows = vec![
        commit_row("a", true, ReviewStatus::Unreviewed),
        commit_row("b", true, ReviewStatus::IssueFound),
        commit_row("c", false, ReviewStatus::Unreviewed),
    ];
    let ids = BTreeSet::from(["b".to_owned(), "c".to_owned()]);

    apply_status_transition(&mut rows, &ids, ReviewStatus::Reviewed);

    assert!(rows[0].selected);
    assert!(!rows[1].selected);
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
        "3h ago".to_owned(),
        Style::default(),
        24,
    );
    let flattened = rendered
        .spans
        .iter()
        .map(|s| s.content.to_string())
        .collect::<String>();
    assert!(flattened.ends_with("3h ago"));
}

#[test]
fn compose_commit_line_preserves_age_column_on_narrow_width() {
    let row = commit_row("abc1234", false, ReviewStatus::IssueFound);
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(24, 3_600, &theme);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|s| s.content.to_string())
        .collect::<String>();
    assert!(flattened.ends_with("1h ago"));
}

#[test]
fn compose_commit_line_marks_selected_rows() {
    let row = commit_row("abc1234", true, ReviewStatus::Unreviewed);
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let presenter = ListLinePresenter::new(80, 3_600, &theme);
    let rendered = presenter.commit_row_line(&row);
    let flattened = rendered
        .spans
        .iter()
        .map(|span| span.content.to_string())
        .collect::<String>();

    assert!(flattened.starts_with("[x] "));
}

#[test]
fn list_row_style_layers_cursor_over_selection() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let selected_only = list_row_style(true, false, false, Some(theme.cursor_bg), &theme);
    let cursor_only = list_row_style(false, true, true, Some(theme.cursor_bg), &theme);
    let selected_cursor = list_row_style(true, true, true, Some(theme.cursor_bg), &theme);

    assert_eq!(selected_only.bg, Some(theme.cursor_bg));
    assert_eq!(cursor_only.bg, Some(theme.visual_bg));
    assert!(selected_cursor.bg.is_some_and(|bg| bg != theme.cursor_bg));
}

#[test]
fn list_row_style_uses_focus_sensitive_cursor_colors() {
    let theme = UiTheme::from_mode(ThemeMode::Dark);
    let focused = list_row_style(false, true, true, None, &theme);
    let unfocused = list_row_style(false, true, false, None, &theme);

    assert_eq!(focused.bg, Some(theme.visual_bg));
    assert_eq!(unfocused.bg, Some(theme.cursor_bg));
}

#[test]
fn status_badges_use_exact_workflow_labels() {
    assert_eq!(status_short_label(ReviewStatus::Unreviewed), "UNREVIEWED");
    assert_eq!(status_short_label(ReviewStatus::Reviewed), "REVIEWED");
    assert_eq!(status_short_label(ReviewStatus::IssueFound), "ISSUE_FOUND");
    assert_eq!(status_short_label(ReviewStatus::Resolved), "RESOLVED");
}

#[test]
fn relative_time_formats_expected_units() {
    assert_eq!(format_relative_time(100, 130), "30s ago");
    assert_eq!(format_relative_time(100, 220), "2m ago");
    assert_eq!(format_relative_time(100, 3700), "1h ago");
}

#[test]
fn next_poll_timeout_uses_nearest_deadline() {
    let timeout = next_poll_timeout(Duration::from_secs(1), Duration::from_secs(10));
    assert_eq!(timeout, Duration::from_secs(3));
}

#[test]
fn next_poll_timeout_zero_when_any_deadline_elapsed() {
    let timeout = next_poll_timeout(Duration::from_secs(5), Duration::from_secs(1));
    assert_eq!(timeout, Duration::from_secs(0));
}

#[test]
fn next_poll_timeout_after_refresh_waits_for_auto_refresh_window() {
    let timeout = next_poll_timeout(Duration::from_secs(0), Duration::from_secs(0));
    assert_eq!(timeout, AUTO_REFRESH_EVERY);
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

    let found = find_diff_match_from_cursor(&lines, "alp", true, 2);
    assert_eq!(found, Some(0));
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

    let found = find_diff_match_from_cursor(&lines, "gam", false, 0);
    assert_eq!(found, Some(2));
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
