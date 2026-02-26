
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
}

#[test]
fn config_parses_lowercase_keys() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("config.yaml");
    fs::write(
            &path,
            "startup_theme: light\ndiff_wheel_scroll_lines: 3\nlist_wheel_coalesce_ms: 12\nnerd_fonts: false\n",
        )
        .expect("write");

    let loaded = AppConfig::load_from_path(&path).expect("load");
    assert_eq!(loaded.startup_theme, StartupTheme::Light);
    assert_eq!(loaded.diff_wheel_scroll_lines, 3);
    assert_eq!(loaded.list_wheel_coalesce_ms, 12);
    assert!(!loaded.nerd_fonts);
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
