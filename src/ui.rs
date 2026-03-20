//! TUI rendering using ratatui widgets.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, Gauge, List, ListItem, ListState, Paragraph, Tabs},
    Frame,
};
use ratatui_image::{StatefulImage, protocol::StatefulProtocol};

use crate::backend::PlayerState;
use crate::state::{AppState, LibrarySubView, Tab};
use crate::theme::Theme;

pub fn draw(f: &mut Frame, state: &AppState, theme: &Theme, artwork: &mut Option<StatefulProtocol>) {
    let area = f.area();

    let box_w = if state.ui_width == 0 {
        area.width
    } else {
        area.width.min(state.ui_width)
    };
    let h_pad = (area.width.saturating_sub(box_w)) / 2;

    let inner = Rect {
        x: area.x + h_pad,
        y: area.y,
        width: box_w,
        height: area.height,
    };

    // Main border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            "  muse ♫ ",
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));

    f.render_widget(block, inner);

    let content_area = inner.inner(Margin::new(1, 1));

    // Split: player section | tab bar | gap | tab content | help line
    let chunks = Layout::vertical([
        Constraint::Length(player_height(state)),
        Constraint::Length(1), // tab bar
        Constraint::Length(1), // gap below tab bar
        Constraint::Min(3),   // tab content
        Constraint::Length(1), // help line
    ])
    .split(content_area);

    draw_player_section(f, chunks[0], state, theme, artwork);
    draw_tab_bar(f, chunks[1], state, theme);
    // chunks[2] is the gap (empty)
    draw_tab_content(f, chunks[3], state, theme);
    draw_help_line(f, chunks[4], state, theme);
}

fn player_height(state: &AppState) -> u16 {
    if !state.music_running {
        3
    } else if state.track.is_some() {
        10
    } else {
        2
    }
}

fn draw_player_section(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    theme: &Theme,
    artwork: &mut Option<StatefulProtocol>,
) {
    if !state.music_running {
        let lines = vec![
            Line::from(Span::styled(
                "Music.app is not running",
                Style::default().fg(theme.error),
            )),
            Line::from(Span::styled(
                "Open Music.app to get started",
                Style::default().fg(theme.text_dim),
            )),
        ];
        let p = Paragraph::new(lines).alignment(Alignment::Center);
        f.render_widget(p, area);
        return;
    }

    let Some(track) = &state.track else {
        let p = Paragraph::new(Span::styled(
            "No track playing",
            Style::default().fg(theme.text_dim),
        ))
        .alignment(Alignment::Center);
        f.render_widget(p, area);
        return;
    };

    // Always reserve space for artwork to prevent layout shifts during track changes.
    // The artwork column stays even while new artwork is loading.
    let wide_enough = state.show_artwork && area.width >= 30;
    let art_cols: u16 = 14;

    let (art_area, text_area) = if wide_enough {
        let cols = Layout::horizontal([
            Constraint::Length(art_cols),
            Constraint::Length(2), // gap between artwork and text
            Constraint::Min(20),
        ])
        .split(area);
        (Some(cols[0]), cols[2])
    } else {
        (None, area)
    };

    // Render artwork if available
    if let (Some(art_rect), Some(proto)) = (art_area, artwork.as_mut()) {
        let image = StatefulImage::default();
        f.render_stateful_widget(image, art_rect, proto);
    }

    // Text content
    let rows = Layout::vertical([
        Constraint::Length(1), // [0] blank (top pad)
        Constraint::Length(1), // [1] track name
        Constraint::Length(1), // [2] artist — album
        Constraint::Length(0), // [3] blank
        Constraint::Length(3), // [4] progress bar (bordered)
        Constraint::Length(0), // [5] blank
        Constraint::Length(1), // [6] controls
        Constraint::Min(0),    // [7] remaining space (bottom pad)
    ])
    .split(text_area);

    // Track name
    let fav = if state.current_track_favorited {
        " ♥"
    } else {
        ""
    };
    let title = format!("{}{}", track.name, fav);
    f.render_widget(
        Paragraph::new(Span::styled(
            title,
            Style::default()
                .fg(theme.text_bright)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        rows[1],
    );

    // Artist — Album
    let subtitle = format!("{} — {}", track.artist, track.album);
    f.render_widget(
        Paragraph::new(Span::styled(
            subtitle,
            Style::default().fg(theme.time_text),
        ))
        .alignment(Alignment::Center),
        rows[2],
    );

    // Progress bar
    let progress = if track.duration > 0.0 {
        (track.position / track.duration).min(1.0)
    } else {
        0.0
    };
    let time_label = format!(
        " {} / {} ",
        format_time(track.position),
        format_time(track.duration)
    );
    let progress_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(theme.border));
    let gauge = Gauge::default()
        .block(progress_block)
        .gauge_style(Style::default().fg(theme.accent))
        .ratio(progress)
        .label(Span::styled(
            time_label,
            Style::default().fg(theme.time_text),
        ));
    f.render_widget(gauge, rows[4]);

    // Controls
    let play_icon = if state.player_state == PlayerState::Playing {
        "▐▐"
    } else {
        " ▶"
    };
    let shuffle_str = if state.shuffle_enabled {
        "⤮ on "
    } else {
        "⤮ off"
    };
    let repeat_str = format!("⟳ {}", state.repeat_mode.label());
    let vol_str = format!("Vol: {}%", state.volume);
    let controls = format!(
        " ◂◂  {}  ▸▸   {}  {}  {} ",
        play_icon, shuffle_str, repeat_str, vol_str
    );
    f.render_widget(
        Paragraph::new(Span::styled(controls, Style::default().fg(theme.text)))
            .alignment(Alignment::Center),
        rows[6],
    );
}

fn draw_tab_bar(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| {
            if *t == state.active_tab {
                Line::from(Span::styled(
                    format!("[{}]", t.label()),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(
                    format!(" {} ", t.label()),
                    Style::default().fg(theme.text),
                ))
            }
        })
        .collect();

    let selected = Tab::ALL
        .iter()
        .position(|t| *t == state.active_tab)
        .unwrap_or(0);

    let tabs = Tabs::new(titles)
        .select(selected)
        .divider(Span::styled("  ", Style::default().fg(theme.text_muted)))
        .style(Style::default().fg(theme.text));

    f.render_widget(tabs, area);
}

fn draw_tab_content(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    if state.show_help {
        draw_help_overlay(f, area, theme);
        return;
    }
    if state.show_playlist_picker {
        draw_playlist_picker(f, area, state, theme);
        return;
    }
    if state.show_theme_picker {
        draw_theme_picker(f, area, state, theme);
        return;
    }
    match state.active_tab {
        Tab::Queue => draw_queue(f, area, state, theme),
        Tab::Library => draw_library(f, area, state, theme),
        Tab::Search => draw_search(f, area, state, theme),
        Tab::Lyrics => draw_lyrics(f, area, state, theme),
    }
}

fn draw_queue(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    if state.queue_tracks.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No queue — play a playlist to fill",
                Style::default().fg(theme.text_dim),
            ))
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = state
        .queue_tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let marker = if i == state.queue_selected {
                "▸ "
            } else {
                "  "
            };
            let dur = format_time(t.duration);
            let entry = format!("{}{} — {}", marker, t.name, t.artist);
            let style = if i == state.queue_selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(vec![
                Span::styled(entry, style),
                Span::styled(format!("  {}", dur), Style::default().fg(theme.text_muted)),
            ]))
        })
        .collect();

    let mut list_state = ListState::default().with_offset(state.queue_scroll);
    let list = List::new(items);
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_library(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    match &state.library_sub_view {
        LibrarySubView::Playlists => {
            if state.playlists.is_empty() {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        "Loading playlists…",
                        Style::default().fg(theme.text_dim),
                    ))
                    .alignment(Alignment::Center),
                    area,
                );
                return;
            }
            let items: Vec<ListItem> = state
                .playlists
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let marker = if i == state.library_selected {
                        "▸ "
                    } else {
                        "  "
                    };
                    let style = if i == state.library_selected {
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.text)
                    };
                    ListItem::new(Span::styled(format!("{}{}", marker, name), style))
                })
                .collect();

            let mut list_state = ListState::default().with_offset(state.library_scroll);
            f.render_stateful_widget(List::new(items), area, &mut list_state);
        }
        LibrarySubView::Tracks(playlist_name) => {
            let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);

            // Back header
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("← {}", playlist_name),
                    Style::default()
                        .fg(theme.text_dim)
                        .add_modifier(Modifier::DIM),
                )),
                rows[0],
            );

            if state.playlist_tracks.is_empty() {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        "Loading…",
                        Style::default().fg(theme.text_dim),
                    ))
                    .alignment(Alignment::Center),
                    rows[1],
                );
            } else {
                let items: Vec<ListItem> = state
                    .playlist_tracks
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let marker = if i == state.playlist_tracks_selected {
                            "▸ "
                        } else {
                            "  "
                        };
                        let dur = format_time(t.duration);
                        let style = if i == state.playlist_tracks_selected {
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.text)
                        };
                        ListItem::new(Line::from(vec![
                            Span::styled(format!("{}{} — {}", marker, t.name, t.artist), style),
                            Span::styled(
                                format!("  {}", dur),
                                Style::default().fg(theme.text_muted),
                            ),
                        ]))
                    })
                    .collect();

                let mut list_state =
                    ListState::default().with_offset(state.playlist_tracks_scroll);
                f.render_stateful_widget(List::new(items), rows[1], &mut list_state);
            }
        }
    }
}

fn draw_search(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);

    // Search input
    let prompt = format!("/ {}▏", state.search_query);
    f.render_widget(
        Paragraph::new(Span::styled(prompt, Style::default().fg(theme.text))),
        rows[0],
    );

    if state.search_results.is_empty() {
        if !state.search_query.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    "No results",
                    Style::default().fg(theme.text_dim),
                ))
                .alignment(Alignment::Center),
                rows[1],
            );
        }
        return;
    }

    let items: Vec<ListItem> = state
        .search_results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let marker = if i == state.search_selected {
                "▸ "
            } else {
                "  "
            };
            let style = if i == state.search_selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Span::styled(
                format!("{}{} — {}", marker, r.name, r.artist),
                style,
            ))
        })
        .collect();

    let mut list_state = ListState::default().with_offset(state.search_scroll);
    f.render_stateful_widget(List::new(items), rows[1], &mut list_state);
}

fn draw_lyrics(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    if state.lyrics_lines.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No lyrics available",
                Style::default().fg(theme.text_dim),
            ))
            .alignment(Alignment::Center),
            area,
        );
        return;
    }

    // Find current line index for synced lyrics
    let current_line = if state.lyrics_synced {
        state
            .track
            .as_ref()
            .and_then(|t| {
                state
                    .lyrics_lines
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, l)| l.time.map_or(false, |time| time <= t.position))
                    .map(|(i, _)| i)
            })
    } else {
        None
    };

    let lines: Vec<Line> = state
        .lyrics_lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let style = if Some(i) == current_line {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else if state.lyrics_synced && current_line.is_some() {
                Style::default().fg(theme.text_dim)
            } else {
                Style::default().fg(theme.text)
            };
            Line::styled(&line.text, style)
        })
        .collect();

    let paragraph = Paragraph::new(lines).scroll((state.lyrics_scroll as u16, 0));
    f.render_widget(paragraph, area);
}

fn draw_theme_picker(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(area);

    f.render_widget(
        Paragraph::new(Span::styled(
            "Themes",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        rows[0],
    );

    let items: Vec<ListItem> = state
        .themes
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            let marker = if i == state.theme_selected {
                "▸ "
            } else {
                "  "
            };
            let check = if *name == state.theme_name { " ✓" } else { "" };
            let style = if i == state.theme_selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Span::styled(format!("{}{}{}", marker, name, check), style))
        })
        .collect();

    let mut list_state = ListState::default().with_offset(state.theme_scroll);
    f.render_stateful_widget(List::new(items), rows[1], &mut list_state);
}

fn draw_help_overlay(f: &mut Frame, area: Rect, theme: &Theme) {
    let bindings = [
        ("Tab / Shift+Tab", "Cycle tabs"),
        ("l", "Library tab"),
        ("/", "Search tab"),
        ("space", "Play / Pause"),
        ("n", "Next track"),
        ("p", "Previous track"),
        ("+ / =", "Volume up"),
        ("-", "Volume down"),
        ("s", "Toggle shuffle"),
        ("r", "Cycle repeat"),
        ("C", "Clear queue"),
        ("f", "Toggle favorite"),
        ("P", "Add to playlist"),
        ("a", "Search artist"),
        ("A", "Search album"),
        ("o", "Open artist in Music"),
        ("O", "Open album in Music"),
        ("L", "Lyrics tab"),
        ("t", "Theme picker"),
        ("↑ / ↓", "Navigate list"),
        ("Enter", "Play / Browse"),
        ("Backspace", "Back / Clear"),
        ("?", "Toggle help"),
        ("q", "Quit"),
    ];

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for (key, desc) in &bindings {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<20}", key),
                Style::default()
                    .fg(theme.text_bright)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(*desc, Style::default().fg(theme.text)),
        ]));
    }

    let content_width = 40u16;
    let x = area.x + area.width.saturating_sub(content_width) / 2;
    let centered = Rect::new(x, area.y, content_width.min(area.width), area.height);

    // Title centered across full area
    let title = Paragraph::new(Line::from(Span::styled(
        "Keybindings",
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center);
    f.render_widget(title, Rect::new(area.x, area.y, area.width, 1));

    // Bindings left-aligned in centered block, offset past title + blank line
    let bindings_area = Rect::new(centered.x, centered.y + 2, centered.width, centered.height.saturating_sub(2));
    let p = Paragraph::new(lines[2..].to_vec());
    f.render_widget(p, bindings_area);
}

fn draw_playlist_picker(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(area);

    f.render_widget(
        Paragraph::new(Span::styled(
            "Add to Playlist",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        rows[0],
    );

    if state.playlists.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No playlists",
                Style::default().fg(theme.text_dim),
            ))
            .alignment(Alignment::Center),
            rows[1],
        );
        return;
    }

    let items: Vec<ListItem> = state
        .playlists
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let marker = if i == state.playlist_picker_selected {
                "▸ "
            } else {
                "  "
            };
            let style = if i == state.playlist_picker_selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Span::styled(format!("{}{}", marker, name), style))
        })
        .collect();

    let mut list_state = ListState::default().with_offset(state.playlist_picker_scroll);
    f.render_stateful_widget(List::new(items), rows[1], &mut list_state);
}

fn draw_help_line(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let left = "? Help · q Quit";
    let right = &state.lastfm_status;

    if right.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                left,
                Style::default().fg(theme.text_dim).add_modifier(Modifier::DIM),
            )),
            area,
        );
    } else {
        let padding = area.width as usize - left.len().min(area.width as usize) - right.len().min(area.width as usize);
        let line = Line::from(vec![
            Span::styled(left, Style::default().fg(theme.text_dim).add_modifier(Modifier::DIM)),
            Span::raw(" ".repeat(padding.max(1))),
            Span::styled(right.as_str(), Style::default().fg(theme.text_dim).add_modifier(Modifier::DIM)),
        ]);
        f.render_widget(Paragraph::new(line), area);
    }
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let m = total / 60;
    let s = total % 60;
    format!("{}:{:02}", m, s)
}
