//! TUI rendering using ratatui widgets.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, Clear, List, ListItem, ListState, Paragraph, Tabs},
    Frame,
};
use ratatui_image::{StatefulImage, protocol::StatefulProtocol};

use crate::backend;
use crate::state::{AppState, LibrarySubView, Tab};
use crate::theme::Theme;

/// Entry point. Takes `&mut AppState` so we can apply the lyric auto-scroll
/// mutation in place, plus a separate `&mut` to the artwork (held in
/// run_app's local — see the comment on `PlayerData::artwork_key`). Keeping
/// artwork out of AppState lets us hold a stable mutable reference across
/// renders, which the kitty graphics protocol needs for its transmit-once
/// state to survive.
pub fn draw(
    f: &mut Frame,
    state: &mut AppState,
    theme: &Theme,
    artwork: &mut Option<StatefulProtocol>,
    elapsed: f64,
) {
    let effective_position = effective_track_position(state, elapsed);
    apply_lyrics_autoscroll(state, effective_position);
    do_draw(f, state, theme, artwork, effective_position);
}

fn effective_track_position(state: &AppState, elapsed: f64) -> Option<f64> {
    let track = state.player.track.as_ref()?;
    if state.player.playback == backend::PlayerState::Playing {
        Some((track.position + elapsed).min(track.duration))
    } else {
        Some(track.position)
    }
}

fn apply_lyrics_autoscroll(state: &mut AppState, effective_position: Option<f64>) {
    if !state.lyrics.synced || state.lyrics.manual_scroll {
        return;
    }
    let Some(pos) = effective_position else { return };
    let current_idx = state
        .lyrics
        .lines
        .iter()
        .enumerate()
        .rev()
        .find(|(_, l)| l.time.map_or(false, |t| t <= pos))
        .map(|(i, _)| i);
    let Some(idx) = current_idx else { return };
    let max_rows = 20; // approximate; corrected by actual render area
    let target = idx.saturating_sub(max_rows / 2);
    let max_scroll = state.lyrics.lines.len().saturating_sub(max_rows);
    state.lyrics.scroll = target.min(max_scroll);
}

fn do_draw(
    f: &mut Frame,
    state: &AppState,
    theme: &Theme,
    artwork: &mut Option<StatefulProtocol>,
    effective_position: Option<f64>,
) {
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

    draw_player_section(f, chunks[0], state, theme, artwork, effective_position);
    draw_tab_bar(f, chunks[1], state, theme);
    // chunks[2] is the gap (empty)
    draw_tab_content(f, chunks[3], state, theme, effective_position);
    draw_help_line(f, chunks[4], state, theme);
}

fn player_height(state: &AppState) -> u16 {
    if !state.player.music_running {
        3
    } else if state.player.track.is_some() {
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
    effective_position: Option<f64>,
) {
    if !state.player.music_running {
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

    let Some(track) = &state.player.track else {
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

    // Render artwork if available.
    //
    // We Clear the cells first, matching the ratatui-image `thread.rs`
    // example. Without Clear, the parent Block's border render leaves cell
    // styling in this area that interferes with the kitty graphics
    // protocol's unicode placeholders — kitty won't replace placeholder
    // cells that already have a foreground color from a previous widget.
    if let (Some(art_rect), Some(proto)) = (art_area, artwork.as_mut()) {
        f.render_widget(Clear, art_rect);
        let image = StatefulImage::default();
        f.render_stateful_widget(image, art_rect, proto);
    }

    // Text content
    let rows = Layout::vertical([
        Constraint::Length(1), // [0] blank (top pad)
        Constraint::Length(1), // [1] track name
        Constraint::Length(1), // [2] artist — album
        Constraint::Length(1), // [3] blank
        Constraint::Length(1), // [4] progress bar
        Constraint::Length(1), // [5] blank
        Constraint::Length(1), // [6] controls
        Constraint::Min(0),    // [7] remaining space (bottom pad)
    ])
    .split(text_area);

    // Track name
    let fav = if state.player.current_track_favorited {
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

    // Progress bar with elapsed / total flanking the bar
    let display_position = effective_position.unwrap_or(track.position);
    let progress = if track.duration > 0.0 {
        (display_position / track.duration).min(1.0)
    } else {
        0.0
    };
    let elapsed_str = format!("{} ", format_time(display_position));
    let total_str = format!(" {}", format_time(track.duration));
    let bar_width = (rows[4].width as usize)
        .saturating_sub(elapsed_str.len() + total_str.len());
    let filled_count = ((bar_width as f64) * progress).round() as usize;
    let unfilled_count = bar_width.saturating_sub(filled_count);

    let bar: String = std::iter::repeat('█')
        .take(filled_count)
        .chain(std::iter::repeat('░').take(unfilled_count))
        .collect();

    let spans = vec![
        Span::styled(elapsed_str, Style::default().fg(theme.time_text)),
        Span::styled(
            &bar[..bar.len().min(filled_count * '█'.len_utf8())],
            Style::default().fg(theme.accent),
        ),
        Span::styled(
            &bar[bar.len().min(filled_count * '█'.len_utf8())..],
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(total_str, Style::default().fg(theme.time_text)),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), rows[4]);

    // Controls — status-only (keybindings shown in help line)
    let shuffle_str = if state.player.shuffle_enabled {
        "⤮ on "
    } else {
        "⤮ off"
    };
    let repeat_str = format!("⟳ {}", state.player.repeat_mode.label());
    let vol_str = format!("Vol: {}%", state.player.volume);
    let controls = format!("{}  {}  {}", shuffle_str, repeat_str, vol_str);
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

fn draw_tab_content(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    theme: &Theme,
    effective_position: Option<f64>,
) {
    if state.overlays.show_help {
        draw_help_overlay(f, area, theme);
        return;
    }
    if state.overlays.playlist_picker_visible {
        draw_playlist_picker(f, area, state, theme);
        return;
    }
    match state.active_tab {
        Tab::Queue => draw_queue(f, area, state, theme),
        Tab::Library => draw_library(f, area, state, theme),
        Tab::Search => draw_search(f, area, state, theme),
        Tab::Lyrics => draw_lyrics(f, area, state, theme, effective_position),
    }
    if state.theme.picker_visible {
        draw_theme_picker(f, area, state, theme);
    }
    if let Some(msg) = &state.overlays.error_message {
        draw_error_overlay(f, area, msg, theme);
    }
}

fn draw_error_overlay(f: &mut Frame, area: Rect, msg: &str, theme: &Theme) {
    let lines: Vec<&str> = msg.lines().collect();
    let popup_w = lines
        .iter()
        .map(|l| l.chars().count() as u16)
        .max()
        .unwrap_or(40)
        .saturating_add(4)
        .max(30)
        .min(area.width.saturating_sub(4));
    let popup_h = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));

    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            " Error ",
            Style::default().fg(theme.error).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.error));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let mut body: Vec<Line> = lines
        .iter()
        .map(|l| Line::from(Span::styled(*l, Style::default().fg(theme.text))))
        .collect();
    body.push(Line::from(""));
    body.push(Line::from(Span::styled(
        "press any key to dismiss",
        Style::default().fg(theme.text_dim).add_modifier(Modifier::DIM),
    )));

    f.render_widget(
        Paragraph::new(body).alignment(Alignment::Center),
        inner,
    );
}

fn draw_filter_bar(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let line = Line::from(vec![
        Span::styled("/ ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        if state.filter.active {
            Span::styled(format!("{}▏", state.filter.query), Style::default().fg(theme.accent))
        } else {
            Span::styled(state.filter.query.clone(), Style::default().fg(theme.accent))
        },
        Span::styled("  Esc to clear", Style::default().fg(theme.text_dim).add_modifier(Modifier::DIM)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_queue(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    if state.queue.tracks.is_empty() {
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

    let has_filter = !state.filter.query.is_empty() || state.filter.active;
    let (filter_area, list_area) = if has_filter {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
        draw_filter_bar(f, rows[0], state, theme);
        (Some(rows[0]), rows[1])
    } else {
        (None, area)
    };
    let _ = filter_area;

    // Build filtered indices
    let filtered: Vec<usize> = if !state.filter.query.is_empty() {
        let q = state.filter.query.to_lowercase();
        state.queue.tracks.iter().enumerate()
            .filter(|(_, t)| t.name.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..state.queue.tracks.len()).collect()
    };

    if filtered.is_empty() && has_filter {
        f.render_widget(
            Paragraph::new(Span::styled("No matches", Style::default().fg(theme.text_dim)))
                .alignment(Alignment::Center),
            list_area,
        );
        return;
    }

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(display_idx, &real_idx)| {
            let t = &state.queue.tracks[real_idx];
            let is_selected = display_idx == state.queue.selected;
            let is_playing = state.queue.playing == Some(real_idx);
            let marker = if is_selected {
                "▸ "
            } else if is_playing {
                "♫ "
            } else {
                "  "
            };
            let dur = format_time(t.duration);
            let entry = format!("{}{} — {}", marker, t.name, t.artist);
            let style = if is_selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else if is_playing {
                Style::default()
                    .fg(theme.text_bright)
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

    let mut list_state = ListState::default().with_offset(state.queue.scroll);
    let list = List::new(items);
    f.render_stateful_widget(list, list_area, &mut list_state);
}

fn draw_library(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let has_filter = !state.filter.query.is_empty() || state.filter.active;

    match &state.library.sub_view {
        LibrarySubView::Playlists => {
            if state.library.playlists.is_empty() {
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

            let (list_area, filtered) = if has_filter {
                let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
                draw_filter_bar(f, rows[0], state, theme);
                let q = state.filter.query.to_lowercase();
                let indices: Vec<usize> = state.library.playlists.iter().enumerate()
                    .filter(|(_, s)| s.to_lowercase().contains(&q))
                    .map(|(i, _)| i)
                    .collect();
                (rows[1], indices)
            } else {
                (area, (0..state.library.playlists.len()).collect())
            };

            if filtered.is_empty() && has_filter {
                f.render_widget(
                    Paragraph::new(Span::styled("No matches", Style::default().fg(theme.text_dim)))
                        .alignment(Alignment::Center),
                    list_area,
                );
                return;
            }

            let items: Vec<ListItem> = filtered
                .iter()
                .enumerate()
                .map(|(display_idx, &real_idx)| {
                    let name = &state.library.playlists[real_idx];
                    let marker = if display_idx == state.library.selected {
                        "▸ "
                    } else {
                        "  "
                    };
                    let style = if display_idx == state.library.selected {
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.text)
                    };
                    ListItem::new(Span::styled(format!("{}{}", marker, name), style))
                })
                .collect();

            let mut list_state = ListState::default().with_offset(state.library.scroll);
            f.render_stateful_widget(List::new(items), list_area, &mut list_state);
        }
        LibrarySubView::Tracks(playlist_name) => {
            // Header rows: back button + optional filter bar
            let mut constraints: Vec<Constraint> = vec![Constraint::Length(1)];
            if has_filter {
                constraints.push(Constraint::Length(1));
            }
            constraints.push(Constraint::Min(1));
            let rows = Layout::vertical(constraints).split(area);

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

            let list_area = if has_filter {
                draw_filter_bar(f, rows[1], state, theme);
                rows[2]
            } else {
                rows[1]
            };

            if state.library.tracks.is_empty() {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        "Loading…",
                        Style::default().fg(theme.text_dim),
                    ))
                    .alignment(Alignment::Center),
                    list_area,
                );
            } else {
                let filtered: Vec<usize> = if !state.filter.query.is_empty() {
                    let q = state.filter.query.to_lowercase();
                    state.library.tracks.iter().enumerate()
                        .filter(|(_, t)| t.name.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q))
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    (0..state.library.tracks.len()).collect()
                };

                if filtered.is_empty() && has_filter {
                    f.render_widget(
                        Paragraph::new(Span::styled("No matches", Style::default().fg(theme.text_dim)))
                            .alignment(Alignment::Center),
                        list_area,
                    );
                    return;
                }

                let items: Vec<ListItem> = filtered
                    .iter()
                    .enumerate()
                    .map(|(display_idx, &real_idx)| {
                        let t = &state.library.tracks[real_idx];
                        let marker = if display_idx == state.library.tracks_selected {
                            "▸ "
                        } else {
                            "  "
                        };
                        let dur = format_time(t.duration);
                        let style = if display_idx == state.library.tracks_selected {
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
                    ListState::default().with_offset(state.library.tracks_scroll);
                f.render_stateful_widget(List::new(items), list_area, &mut list_state);
            }
        }
    }
}

fn draw_search(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).split(area);

    // Search input — accent color + cursor when editing, dimmed when browsing
    if state.search.editing {
        let search_line = Line::from(vec![
            Span::styled("/ ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}▏", state.search.query), Style::default().fg(theme.accent)),
            Span::styled("  Enter to browse · Esc to stop", Style::default().fg(theme.text_dim).add_modifier(Modifier::DIM)),
        ]);
        f.render_widget(Paragraph::new(search_line), rows[0]);
    } else {
        let search_line = Line::from(vec![
            Span::styled(format!("/ {}", state.search.query), Style::default().fg(theme.text_dim)),
            Span::styled("  / to edit", Style::default().fg(theme.text_dim).add_modifier(Modifier::DIM)),
        ]);
        f.render_widget(Paragraph::new(search_line), rows[0]);
    }

    if state.search.results.is_empty() {
        if !state.search.query.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    "No results",
                    Style::default().fg(theme.text_dim),
                ))
                .alignment(Alignment::Center),
                rows[2],
            );
        }
        return;
    }

    let items: Vec<ListItem> = state
        .search
        .results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let marker = if !state.search.editing && i == state.search.selected {
                "▸ "
            } else {
                "  "
            };
            let style = if state.search.editing {
                // Editing: dim all results to show focus is on the search bar
                Style::default().fg(theme.text_dim)
            } else if i == state.search.selected {
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

    let mut list_state = ListState::default().with_offset(state.search.scroll);
    f.render_stateful_widget(List::new(items), rows[2], &mut list_state);
}

fn draw_lyrics(
    f: &mut Frame,
    area: Rect,
    state: &AppState,
    theme: &Theme,
    effective_position: Option<f64>,
) {
    if state.lyrics.lines.is_empty() {
        let msg = if state.lyrics_enabled {
            "No lyrics available"
        } else {
            "Lyrics disabled in config"
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(theme.text_dim)))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    // Find current line index for synced lyrics
    let current_line = if state.lyrics.synced {
        effective_position.and_then(|pos| {
            state
                .lyrics
                .lines
                .iter()
                .enumerate()
                .rev()
                .find(|(_, l)| l.time.map_or(false, |time| time <= pos))
                .map(|(i, _)| i)
        })
    } else {
        None
    };

    let lines: Vec<Line> = state
        .lyrics
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let style = if Some(i) == current_line {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else if state.lyrics.synced && current_line.is_some() {
                Style::default().fg(theme.text_dim)
            } else {
                Style::default().fg(theme.text)
            };
            Line::styled(&line.text, style)
        })
        .collect();

    let paragraph = Paragraph::new(lines).scroll((state.lyrics.scroll as u16, 0));
    f.render_widget(paragraph, area);
}

fn draw_theme_picker(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    // Size the popup to fit content with padding
    let max_name_len = state.theme.all.iter().map(|(n, _)| n.len()).max().unwrap_or(10);
    let popup_w = (max_name_len as u16 + 8).min(area.width.saturating_sub(4)); // "▸ " + name + " ✓" + padding
    let popup_h = (state.theme.all.len() as u16 + 2).min(area.height.saturating_sub(2)); // +2 for border

    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    // Clear the area behind the popup
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            " Themes ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let items: Vec<ListItem> = state
        .theme
        .all
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            let marker = if i == state.theme.selected {
                "▸ "
            } else {
                "  "
            };
            let check = if *name == state.theme.name { " ✓" } else { "" };
            let style = if i == state.theme.selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Span::styled(format!("{}{}{}", marker, name, check), style))
        })
        .collect();

    let mut list_state = ListState::default().with_offset(state.theme.scroll);
    f.render_stateful_widget(List::new(items), inner, &mut list_state);
}

fn draw_help_overlay(f: &mut Frame, area: Rect, theme: &Theme) {
    let bindings = [
        ("Tab / Shift+Tab", "Cycle tabs"),
        ("l", "Library tab"),
        ("/", "Filter / Search"),
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
        ("Enter", "Play"),
        ("→", "Browse playlist"),
        ("Q", "Queue without playing"),
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

    if state.library.playlists.is_empty() {
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
        .library
        .playlists
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let marker = if i == state.overlays.playlist_picker_selected {
                "▸ "
            } else {
                "  "
            };
            let style = if i == state.overlays.playlist_picker_selected {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Span::styled(format!("{}{}", marker, name), style))
        })
        .collect();

    let mut list_state = ListState::default().with_offset(state.overlays.playlist_picker_scroll);
    f.render_stateful_widget(List::new(items), rows[1], &mut list_state);
}

fn draw_help_line(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let left = if state.active_tab == Tab::Search && state.search.editing {
        "Enter browse · Esc stop editing"
    } else {
        "? Help · q Quit"
    };
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
