use ratatui::style::Color;
use std::path::PathBuf;

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

const DEFAULT_THEME_NAME: &str = "synthwave";

/// Embedded default theme files (compiled into the binary).
const EMBEDDED_THEMES: &[(&str, &str)] = &[
    ("classic", include_str!("../themes/classic.toml")),
    ("fire", include_str!("../themes/fire.toml")),
    ("matrix", include_str!("../themes/matrix.toml")),
    ("monochrome", include_str!("../themes/monochrome.toml")),
    ("ocean", include_str!("../themes/ocean.toml")),
    ("purple", include_str!("../themes/purple.toml")),
    ("sunset", include_str!("../themes/sunset.toml")),
    ("synthwave", include_str!("../themes/synthwave.toml")),
    ("tokyo night moon", include_str!("../themes/tokyo-night-moon.toml")),
];

fn themes_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(".config").join("muse").join("themes")
}

/// Write embedded default themes to disk if the themes directory is empty or missing.
fn ensure_default_themes() {
    let dir = themes_dir();
    let _ = std::fs::create_dir_all(&dir);

    // Only write defaults if directory has no .toml files
    let has_themes = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "toml")
                        .unwrap_or(false)
                })
        })
        .unwrap_or(false);

    if !has_themes {
        for (name, contents) in EMBEDDED_THEMES {
            let filename = name.replace(' ', "-") + ".toml";
            let path = dir.join(filename);
            let _ = std::fs::write(path, contents);
        }
    }
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

    Some(Theme {
        border: parse_color(colors.get("border")?)?,
        accent: parse_color(colors.get("accent")?)?,
        text: parse_color(colors.get("text")?)?,
        text_bright: parse_color(colors.get("text_bright")?)?,
        text_dim: parse_color(colors.get("text_dim")?)?,
        text_muted: parse_color(colors.get("text_muted")?)?,
        time_text: parse_color(colors.get("time_text")?)?,
        error: parse_color(colors.get("error")?)?,
    })
}

/// Load all themes from `~/.config/muse/themes/`. Writes defaults on first run.
pub fn load_themes() -> Vec<(String, Theme)> {
    ensure_default_themes();

    let dir = themes_dir();
    let mut themes = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "toml")
                    .unwrap_or(false)
            })
            .collect();
        files.sort_by_key(|e| e.file_name());

        for entry in files {
            let path = entry.path();
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .replace('-', " ");

            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Some(theme) = parse_theme(&contents) {
                    themes.push((name, theme));
                }
            }
        }
    }

    // Fallback: if no themes loaded, parse embedded defaults directly
    if themes.is_empty() {
        for (name, contents) in EMBEDDED_THEMES {
            if let Some(theme) = parse_theme(contents) {
                themes.push((name.to_string(), theme));
            }
        }
    }

    themes
}

pub fn default_theme() -> Theme {
    // Try to parse the embedded synthwave theme
    for (name, contents) in EMBEDDED_THEMES {
        if *name == DEFAULT_THEME_NAME {
            if let Some(theme) = parse_theme(contents) {
                return theme;
            }
        }
    }
    // Hardcoded fallback (should never be reached)
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
