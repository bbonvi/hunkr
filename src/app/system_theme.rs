//! System theme detection for `theme: auto`.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Result;

use super::ThemeMode;

/// Ordered system-theme probes for major Unix desktop environments.
pub(super) struct SystemThemeDetector;

impl SystemThemeDetector {
    pub(super) fn detect() -> Result<Option<ThemeMode>> {
        #[cfg(target_os = "macos")]
        if let Some(mode) = detect_macos_theme() {
            return Ok(Some(mode));
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            if let Some(mode) = detect_orbstack_host_theme() {
                return Ok(Some(mode));
            }
            if let Some(mode) = detect_portal_theme() {
                return Ok(Some(mode));
            }
            if let Some(mode) = detect_gnome_color_scheme() {
                return Ok(Some(mode));
            }
            if let Some(mode) = detect_kde_theme() {
                return Ok(Some(mode));
            }
            if let Some(mode) = detect_gtk_theme_name() {
                return Ok(Some(mode));
            }
        }

        Ok(None)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_orbstack_host_theme() -> Option<ThemeMode> {
    if !is_orbstack_guest() {
        return None;
    }
    detect_defaults_theme()
}

#[cfg(target_os = "macos")]
fn detect_macos_theme() -> Option<ThemeMode> {
    detect_defaults_theme()
}

fn detect_defaults_theme() -> Option<ThemeMode> {
    let output = Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;
    Some(resolve_macos_defaults_output(
        output.status.success(),
        &stdout,
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_orbstack_guest() -> bool {
    orbstack_guest_markers(
        Path::new("/opt/orbstack-guest").exists(),
        command_stdout("uname", &["-r"]).as_deref(),
    )
}

fn orbstack_guest_markers(orbstack_guest_dir_exists: bool, uname_release: Option<&str>) -> bool {
    orbstack_guest_dir_exists
        || uname_release
            .map(|release| release.to_ascii_lowercase().contains("orbstack"))
            .unwrap_or(false)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_portal_theme() -> Option<ThemeMode> {
    let gdbus = command_stdout(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.Read",
            "org.freedesktop.appearance",
            "color-scheme",
        ],
    )
    .and_then(|output| parse_portal_color_scheme(&output));
    if gdbus.is_some() {
        return gdbus;
    }

    command_stdout(
        "busctl",
        &[
            "--user",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "Read",
            "ss",
            "org.freedesktop.appearance",
            "color-scheme",
        ],
    )
    .and_then(|output| parse_portal_color_scheme(&output))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_gnome_color_scheme() -> Option<ThemeMode> {
    command_stdout(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "color-scheme"],
    )
    .and_then(|output| parse_gsettings_color_scheme(&output))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_kde_theme() -> Option<ThemeMode> {
    let kdeglobals = config_home().join("kdeglobals");
    let raw = fs::read_to_string(kdeglobals).ok()?;
    parse_kdeglobals_background(&raw)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_gtk_theme_name() -> Option<ThemeMode> {
    command_stdout(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "gtk-theme"],
    )
    .and_then(|output| parse_gtk_theme_name(&output))
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|stdout| stdout.trim().to_owned())
        .filter(|stdout| !stdout.is_empty())
}

fn config_home() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg);
    }

    if let Ok(home) = env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home).join(".config");
    }

    PathBuf::from(".config")
}

fn resolve_macos_defaults_output(command_succeeded: bool, output: &str) -> ThemeMode {
    if command_succeeded && output.trim() == "Dark" {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}

fn parse_portal_color_scheme(output: &str) -> Option<ThemeMode> {
    let code = output
        .split(|c: char| !c.is_ascii_digit())
        .filter(|segment| !segment.is_empty())
        .filter_map(|segment| segment.parse::<u8>().ok())
        .next_back()?;
    match code {
        1 => Some(ThemeMode::Dark),
        2 => Some(ThemeMode::Light),
        _ => None,
    }
}

fn parse_gsettings_color_scheme(output: &str) -> Option<ThemeMode> {
    match unquote(output).to_ascii_lowercase().as_str() {
        "prefer-dark" => Some(ThemeMode::Dark),
        "default" | "prefer-light" => Some(ThemeMode::Light),
        _ => None,
    }
}

fn parse_gtk_theme_name(output: &str) -> Option<ThemeMode> {
    let theme = unquote(output).to_ascii_lowercase();
    if theme.is_empty() {
        return None;
    }

    // Legacy GTK stacks often encode dark preference in the theme name itself
    // (`Adwaita-dark`, `Breeze:dark`, etc.) when no dedicated color-scheme key exists.
    if theme.contains("dark") {
        Some(ThemeMode::Dark)
    } else {
        Some(ThemeMode::Light)
    }
}

fn parse_kdeglobals_background(raw: &str) -> Option<ThemeMode> {
    let rgb = ini_value(raw, "Colors:Window", "BackgroundNormal")
        .or_else(|| ini_value(raw, "Colors:View", "BackgroundNormal"))?;
    classify_rgb_background(rgb)
}

fn ini_value<'a>(raw: &'a str, section_name: &str, key_name: &str) -> Option<&'a str> {
    let mut current_section = None::<&str>;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            current_section = Some(section.trim());
            continue;
        }
        if current_section != Some(section_name) {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        if key.trim() == key_name {
            return Some(value.trim());
        }
    }
    None
}

fn classify_rgb_background(rgb: &str) -> Option<ThemeMode> {
    let mut channels = rgb
        .split(',')
        .map(|channel| channel.trim().parse::<u16>().ok());
    let red = u32::from(channels.next()??);
    let green = u32::from(channels.next()??);
    let blue = u32::from(channels.next()??);
    if channels.next().is_some() || red > 255 || green > 255 || blue > 255 {
        return None;
    }

    let luminance = (2126 * red + 7152 * green + 722 * blue) / 10_000;
    if luminance <= 127 {
        Some(ThemeMode::Dark)
    } else {
        Some(ThemeMode::Light)
    }
}

fn unquote(value: &str) -> &str {
    value
        .trim()
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .unwrap_or_else(|| value.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_parser_maps_dark_and_light_preferences() {
        assert_eq!(
            parse_portal_color_scheme("(uint32 1,)"),
            Some(ThemeMode::Dark)
        );
        assert_eq!(parse_portal_color_scheme("v u 2"), Some(ThemeMode::Light));
        assert_eq!(parse_portal_color_scheme("(uint32 0,)"), None);
    }

    #[test]
    fn gsettings_color_scheme_parser_handles_known_values() {
        assert_eq!(
            parse_gsettings_color_scheme("'prefer-dark'"),
            Some(ThemeMode::Dark)
        );
        assert_eq!(
            parse_gsettings_color_scheme("'default'"),
            Some(ThemeMode::Light)
        );
        assert_eq!(
            parse_gsettings_color_scheme("'prefer-light'"),
            Some(ThemeMode::Light)
        );
    }

    #[test]
    fn gtk_theme_parser_uses_dark_name_heuristic() {
        assert_eq!(
            parse_gtk_theme_name("'Adwaita-dark'"),
            Some(ThemeMode::Dark)
        );
        assert_eq!(parse_gtk_theme_name("'Breeze'"), Some(ThemeMode::Light));
    }

    #[test]
    fn kdeglobals_parser_reads_window_background_luminance() {
        let dark = r#"
[Colors:Window]
BackgroundNormal=35,38,41
"#;
        let light = r#"
[Colors:Window]
BackgroundNormal=239,240,241
"#;
        assert_eq!(parse_kdeglobals_background(dark), Some(ThemeMode::Dark));
        assert_eq!(parse_kdeglobals_background(light), Some(ThemeMode::Light));
    }

    #[test]
    fn kdeglobals_parser_falls_back_to_view_background() {
        let raw = r#"
[Colors:View]
BackgroundNormal=32,34,36
"#;
        assert_eq!(parse_kdeglobals_background(raw), Some(ThemeMode::Dark));
    }

    #[test]
    fn macos_defaults_result_treats_missing_key_as_light() {
        assert_eq!(
            resolve_macos_defaults_output(true, "Dark\n"),
            ThemeMode::Dark
        );
        assert_eq!(resolve_macos_defaults_output(false, ""), ThemeMode::Light);
    }

    #[test]
    fn orbstack_markers_detect_guest_from_dir_or_kernel_release() {
        assert!(orbstack_guest_markers(true, None));
        assert!(orbstack_guest_markers(false, Some("6.17.8-orbstack-00308")));
        assert!(!orbstack_guest_markers(false, Some("6.12.0-generic")));
    }
}
