use ratatui::style::Color;

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

impl Theme {
    pub const SYNTHWAVE: Theme = Theme {
        border: Color::Indexed(75),
        accent: Color::Indexed(213),
        text: Color::Indexed(252),
        text_bright: Color::Indexed(255),
        text_dim: Color::Indexed(245),
        text_muted: Color::Indexed(240),
        time_text: Color::Indexed(255),
        error: Color::Indexed(196),
    };
}

pub const ALL_THEMES: &[(&str, Theme)] = &[
    (
        "synthwave",
        Theme {
            border: Color::Indexed(75),
            accent: Color::Indexed(213),
            text: Color::Indexed(252),
            text_bright: Color::Indexed(255),
            text_dim: Color::Indexed(245),
            text_muted: Color::Indexed(240),
            time_text: Color::Indexed(255),
            error: Color::Indexed(196),
        },
    ),
    (
        "monochrome",
        Theme {
            border: Color::Indexed(245),
            accent: Color::Indexed(255),
            text: Color::Indexed(250),
            text_bright: Color::Indexed(255),
            text_dim: Color::Indexed(242),
            text_muted: Color::Indexed(238),
            time_text: Color::Indexed(255),
            error: Color::Indexed(196),
        },
    ),
    (
        "ocean",
        Theme {
            border: Color::Indexed(32),
            accent: Color::Indexed(39),
            text: Color::Indexed(153),
            text_bright: Color::Indexed(195),
            text_dim: Color::Indexed(67),
            text_muted: Color::Indexed(60),
            time_text: Color::Indexed(195),
            error: Color::Indexed(196),
        },
    ),
    (
        "sunset",
        Theme {
            border: Color::Indexed(208),
            accent: Color::Indexed(203),
            text: Color::Indexed(223),
            text_bright: Color::Indexed(230),
            text_dim: Color::Indexed(180),
            text_muted: Color::Indexed(137),
            time_text: Color::Indexed(230),
            error: Color::Indexed(196),
        },
    ),
    (
        "matrix",
        Theme {
            border: Color::Indexed(65),
            accent: Color::Indexed(114),
            text: Color::Indexed(151),
            text_bright: Color::Indexed(194),
            text_dim: Color::Indexed(108),
            text_muted: Color::Indexed(59),
            time_text: Color::Indexed(194),
            error: Color::Indexed(196),
        },
    ),
    (
        "tokyo night moon",
        Theme {
            border: Color::Indexed(61),
            accent: Color::Indexed(141),
            text: Color::Indexed(189),
            text_bright: Color::Indexed(195),
            text_dim: Color::Indexed(103),
            text_muted: Color::Indexed(60),
            time_text: Color::Indexed(195),
            error: Color::Indexed(210),
        },
    ),
];

pub fn default_theme() -> Theme {
    Theme::SYNTHWAVE
}

pub fn find_theme(name: &str) -> Option<(usize, Theme)> {
    ALL_THEMES
        .iter()
        .enumerate()
        .find(|(_, (n, _))| *n == name)
        .map(|(i, (_, t))| (i, *t))
}
