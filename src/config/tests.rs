//! Unit tests for config loading defaults and validation clamps.
use super::*;

#[test]
fn config_defaults_when_file_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("missing.yaml");
    let loaded = AppConfig::load_from_path(&path).expect("load");

    assert_eq!(loaded.startup_theme, StartupTheme::Dark);
    assert_eq!(loaded.diff_wheel_scroll_lines, 1);
    assert_eq!(loaded.list_wheel_coalesce_ms, 28);
    assert!(loaded.nerd_fonts);
    assert_eq!(loaded.history_limit, 400);
    assert_eq!(loaded.auto_refresh_every_secs, 2);
    assert_eq!(loaded.relative_time_redraw_every_secs, 30);
    assert_eq!(loaded.theme_reload_poll_every_ms, 1_000);
    assert_eq!(loaded.selection_rebuild_debounce_ms, 120);
    assert_eq!(loaded.terminal_clear_every_secs, 120);
    assert_eq!(loaded.diff_cursor_scroll_off_lines, 3);
}

#[test]
fn config_parses_lowercase_keys() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("config.yaml");
    fs::write(
            &path,
            "startup_theme: light\ndiff_wheel_scroll_lines: 3\nlist_wheel_coalesce_ms: 12\nnerd_fonts: false\nhistory_limit: 128\nauto_refresh_every_secs: 9\nrelative_time_redraw_every_secs: 17\ntheme_reload_poll_every_ms: 600\nselection_rebuild_debounce_ms: 90\nterminal_clear_every_secs: 40\ndiff_cursor_scroll_off_lines: 5\n",
        )
        .expect("write");

    let loaded = AppConfig::load_from_path(&path).expect("load");
    assert_eq!(loaded.startup_theme, StartupTheme::Light);
    assert_eq!(loaded.diff_wheel_scroll_lines, 3);
    assert_eq!(loaded.list_wheel_coalesce_ms, 12);
    assert!(!loaded.nerd_fonts);
    assert_eq!(loaded.history_limit, 128);
    assert_eq!(loaded.auto_refresh_every_secs, 9);
    assert_eq!(loaded.relative_time_redraw_every_secs, 17);
    assert_eq!(loaded.theme_reload_poll_every_ms, 600);
    assert_eq!(loaded.selection_rebuild_debounce_ms, 90);
    assert_eq!(loaded.terminal_clear_every_secs, 40);
    assert_eq!(loaded.diff_cursor_scroll_off_lines, 5);
}

#[test]
fn config_clamps_non_positive_wheel_scroll() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("config.yaml");
    fs::write(&path, "diff_wheel_scroll_lines: 0\n").expect("write");

    let loaded = AppConfig::load_from_path(&path).expect("load");
    assert_eq!(loaded.diff_wheel_scroll_lines, 1);
    assert!(loaded.nerd_fonts);
}

#[test]
fn config_clamps_zero_list_wheel_coalesce() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("config.yaml");
    fs::write(&path, "list_wheel_coalesce_ms: 0\n").expect("write");

    let loaded = AppConfig::load_from_path(&path).expect("load");
    assert_eq!(loaded.list_wheel_coalesce_ms, 28);
}

#[test]
fn config_clamps_zero_runtime_tuning_values() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("config.yaml");
    fs::write(
        &path,
        "history_limit: 0\nauto_refresh_every_secs: 0\nrelative_time_redraw_every_secs: 0\ntheme_reload_poll_every_ms: 0\nselection_rebuild_debounce_ms: 0\nterminal_clear_every_secs: 0\ndiff_cursor_scroll_off_lines: 0\n",
    )
    .expect("write");

    let loaded = AppConfig::load_from_path(&path).expect("load");
    assert_eq!(loaded.history_limit, 400);
    assert_eq!(loaded.auto_refresh_every_secs, 2);
    assert_eq!(loaded.relative_time_redraw_every_secs, 30);
    assert_eq!(loaded.theme_reload_poll_every_ms, 1_000);
    assert_eq!(loaded.selection_rebuild_debounce_ms, 120);
    assert_eq!(loaded.terminal_clear_every_secs, 120);
    assert_eq!(loaded.diff_cursor_scroll_off_lines, 3);
}
