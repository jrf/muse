use ratatui::style::Color;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub border: Color,
    pub accent: Color,
    pub text: Color,
    pub text_bright: Color,
    pub text_dim: Color,
    pub text_muted: Color,
    pub time_text: Color,
    pub error: Color,
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn parse_color(val: &toml::Value) -> Option<Color> {
    match val {
        toml::Value::Integer(n) => {
            let idx = *n as u8;
            Some(Color::Indexed(idx))
        }
        toml::Value::String(s) => {
            // Support hex colors: "#RRGGBB"
            let s = s.trim().trim_start_matches('#');
            if s.len() == 6 {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Color::Rgb(r, g, b))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_theme(contents: &str) -> Option<Theme> {
    let doc: toml::Value = contents.parse().ok()?;
    let colors = doc.get("colors")?;
    let ui = doc.get("ui").and_then(toml::Value::as_table);

    let resolve = |ui_roles: &[&str], color_names: &[&str]| -> Option<Color> {
        for role in ui_roles {
            if let Some(value) = ui.and_then(|table| table.get(*role)) {
                if let Some(color) = parse_color(value) {
                    return Some(color);
                }
                if let Some(reference) = value.as_str() {
                    if let Some(color) = colors.get(reference).and_then(parse_color) {
                        return Some(color);
                    }
                }
            }
        }
        color_names
            .iter()
            .find_map(|name| colors.get(*name).and_then(parse_color))
    };

    Some(Theme {
        border: resolve(&["border"], &["border", "fg_muted", "surface1"])?,
        accent: resolve(&["accent"], &["accent", "magenta", "mauve", "pink"])?,
        text: resolve(&["text"], &["text", "fg"])?,
        text_bright: resolve(
            &["text_bright", "heading"],
            &["text_bright", "fg_bright", "text", "fg"],
        )?,
        text_dim: resolve(
            &["text_dim"],
            &["text_dim", "fg_dim", "subtext0", "comment"],
        )?,
        text_muted: resolve(
            &["text_muted"],
            &["text_muted", "fg_muted", "overlay0", "comment"],
        )?,
        time_text: resolve(
            &["heading", "text_bright"],
            &["time_text", "fg_bright", "text", "fg"],
        )?,
        error: resolve(&["error"], &["error", "red", "maroon"])?,
    })
}

/// Load the selected theme path and the explicit picker catalog.
pub fn load_themes() -> Vec<(String, Theme)> {
    let home = home_dir();
    let config_path = home.join(".config/muse/config.toml");
    let config = std::fs::read_to_string(config_path)
        .ok()
        .and_then(|contents| contents.parse::<toml::Value>().ok());
    let selected_path = config
        .as_ref()
        .and_then(|doc| doc.get("theme"))
        .and_then(toml::Value::as_str);
    let catalog_path = config
        .as_ref()
        .and_then(|doc| doc.get("theme_catalog"))
        .and_then(toml::Value::as_str);
    load_themes_from_paths(catalog_path, selected_path, &home)
}

fn load_themes_from_paths(
    catalog_path: Option<&str>,
    selected_path: Option<&str>,
    home: &Path,
) -> Vec<(String, Theme)> {
    let mut themes = Vec::new();

    if let Some(catalog_path) = catalog_path {
        for path in load_catalog_paths(&expand_home(home, catalog_path), home) {
            load_theme_path(&mut themes, &path);
        }
    }
    if let Some(selected_path) = selected_path {
        load_theme_path(&mut themes, &expand_home(home, selected_path));
    }
    themes.sort_by(|(left, _), (right, _)| left.cmp(right));

    themes
}

fn load_catalog_paths(catalog_path: &Path, home: &Path) -> Vec<PathBuf> {
    let Ok(contents) = std::fs::read_to_string(catalog_path) else {
        return Vec::new();
    };
    let Ok(catalog) = contents.parse::<toml::Value>() else {
        return Vec::new();
    };
    catalog
        .get("themes")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(|path| expand_home(home, path))
        .collect()
}

fn load_theme_path(themes: &mut Vec<(String, Theme)>, path: &Path) {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    let Some(theme) = parse_theme(&contents) else {
        return;
    };
    let name = theme_name(path);
    themes.retain(|(existing, _)| existing != &name);
    themes.push((name, theme));
}

fn expand_home(home: &Path, configured_path: &str) -> PathBuf {
    configured_path
        .strip_prefix("~/")
        .map(|rest| home.join(rest))
        .unwrap_or_else(|| PathBuf::from(configured_path))
}

fn theme_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("theme")
        .replace('-', " ")
}

pub fn configured_theme_name(configured_path: &str) -> String {
    theme_name(&expand_home(&home_dir(), configured_path))
}

pub fn default_theme() -> Theme {
    // Emergency fallback used only when the shared catalog cannot be loaded.
    Theme {
        border: Color::Indexed(75),
        accent: Color::Indexed(213),
        text: Color::Indexed(252),
        text_bright: Color::Indexed(255),
        text_dim: Color::Indexed(245),
        text_muted: Color::Indexed(240),
        time_text: Color::Indexed(255),
        error: Color::Indexed(196),
    }
}

pub fn find_theme(name: &str, themes: &[(String, Theme)]) -> Option<(usize, Theme)> {
    themes
        .iter()
        .enumerate()
        .find(|(_, (n, _))| n == name)
        .map(|(i, (_, t))| (i, *t))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{load_themes_from_paths, parse_theme};
    use ratatui::style::Color;

    const SHARED_THEME: &str = r##"
[colors]
base = "#1e1e2e"
surface1 = "#45475a"
text = "#cdd6f4"
subtext0 = "#a6adc8"
overlay0 = "#6c7086"
mauve = "#cba6f7"
blue = "#89b4fa"
red = "#f38ba8"

[ui]
border = "surface1"
accent = "mauve"
text = "text"
text_bright = "text"
text_dim = "subtext0"
text_muted = "overlay0"
heading = "blue"
error = "red"
"##;

    #[test]
    fn parses_shared_semantic_theme() {
        let theme = parse_theme(SHARED_THEME).expect("shared theme");

        assert_eq!(theme.border, Color::Rgb(0x45, 0x47, 0x5a));
        assert_eq!(theme.accent, Color::Rgb(0xcb, 0xa6, 0xf7));
        assert_eq!(theme.text, Color::Rgb(0xcd, 0xd6, 0xf4));
        assert_eq!(theme.time_text, Color::Rgb(0x89, 0xb4, 0xfa));
        assert_eq!(theme.error, Color::Rgb(0xf3, 0x8b, 0xa8));
    }

    #[test]
    fn catalog_loads_only_explicit_theme_paths() {
        let root = test_root();
        let themes_dir = root.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();
        std::fs::write(themes_dir.join("synthetic-theme.toml"), SHARED_THEME).unwrap();
        std::fs::write(
            themes_dir.join("unlisted.toml"),
            SHARED_THEME.replace("#cba6f7", "#abcdef"),
        )
        .unwrap();
        let catalog = root.join("catalog.toml");
        std::fs::write(
            &catalog,
            format!(
                "themes = [\"{}\"]\n",
                themes_dir.join("synthetic-theme.toml").display()
            ),
        )
        .unwrap();

        let themes = load_themes_from_paths(
            Some(catalog.to_str().unwrap()),
            Some(themes_dir.join("synthetic-theme.toml").to_str().unwrap()),
            &root,
        );
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].0, "synthetic theme");
        assert_eq!(themes[0].1.accent, Color::Rgb(0xcb, 0xa6, 0xf7));

        std::fs::remove_dir_all(root).unwrap();
    }

    fn test_root() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "muse-theme-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
