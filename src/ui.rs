//! TUI rendering using ratatui widgets.

use ratatui::{
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs},
    Frame,
};
use ratatui_image::StatefulImage;

use crate::artwork::{Artwork, KittyPlacement};
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
    artwork: &mut Option<Artwork>,
    elapsed: f64,
) -> Option<KittyPlacement> {
    let effective_position = effective_track_position(state, elapsed);
    apply_lyrics_autoscroll(state, effective_position);
    do_draw(f, state, theme, artwork, effective_position)
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
    let Some(pos) = effective_position else {
        return;
    };
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

const TAB_PADDING_LEFT: &str = " ";
const TAB_PADDING_RIGHT: &str = " ";
const TAB_DIVIDER: &str = "  ";

/// Return the rendered span of each tab title, including Ratatui's padding.
/// Kept beside `draw_tab_bar` so click hit-testing uses the same geometry.
fn tab_ranges(area: Rect) -> Vec<(Tab, u16, u16)> {
    let mut x = area.x;
    let padding = (TAB_PADDING_LEFT.chars().count() + TAB_PADDING_RIGHT.chars().count()) as u16;
    let divider = TAB_DIVIDER.chars().count() as u16;

    Tab::ALL
        .iter()
        .map(|tab| {
            let start = x;
            let width = tab.label().chars().count() as u16 + 2 + padding;
            let end = start.saturating_add(width);
            x = end.saturating_add(divider);
            (*tab, start, end)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickTarget {
    QueueItem(usize),
    LibraryPlaylist(usize),
    LibraryTrack(usize),
    SearchResult(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollTarget {
    Queue,
    LibraryPlaylists,
    LibraryTracks,
    Search,
    Lyrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollHit {
    pub target: ScrollTarget,
    pub max_scroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerClickTarget {
    Tab(Tab),
    Shuffle,
    Repeat,
    Seek(f64),
}

#[derive(Debug, Clone, Copy)]
struct AppLayout {
    player: Rect,
    tabs: Rect,
    tab_content: Rect,
}

fn app_layout(area: Rect, state: &AppState) -> AppLayout {
    let box_w = if state.ui_width == 0 {
        area.width
    } else {
        area.width.min(state.ui_width)
    };
    let h_pad = (area.width.saturating_sub(box_w)) / 2;
    let inner = Rect::new(area.x + h_pad, area.y, box_w, area.height);
    let content_area = inner.inner(Margin::new(1, 1));
    let chunks = Layout::vertical([
        Constraint::Length(player_height(state)),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(content_area);

    AppLayout {
        player: chunks[0],
        tabs: chunks[1],
        tab_content: chunks[3],
    }
}

fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn item_at(area: Rect, scroll: usize, len: usize, column: u16, row: u16) -> Option<usize> {
    if !contains(area, column, row) {
        return None;
    }
    let index = scroll + usize::from(row - area.y);
    (index < len).then_some(index)
}

fn filtered_track_len(tracks: &[crate::backend::PlaylistTrack], query: &str) -> usize {
    if query.is_empty() {
        return tracks.len();
    }
    let query = query.to_lowercase();
    tracks
        .iter()
        .filter(|track| {
            track.name.to_lowercase().contains(&query)
                || track.artist.to_lowercase().contains(&query)
        })
        .count()
}

fn list_area(content: Rect, header_rows: u16) -> Rect {
    Rect::new(
        content.x,
        content.y.saturating_add(header_rows),
        content.width,
        content.height.saturating_sub(header_rows),
    )
}

fn max_scroll(len: usize, area: Rect) -> usize {
    len.saturating_sub(usize::from(area.height))
}

fn player_text_area(player: Rect, state: &AppState) -> Rect {
    if state.show_artwork && player.width >= 30 {
        Layout::horizontal([
            Constraint::Length(14),
            Constraint::Length(2),
            Constraint::Min(20),
        ])
        .split(player)[2]
    } else {
        player
    }
}

/// Resolve clicks on the tab bar and now-playing controls. Progress clicks are
/// reported as an absolute position in seconds, measured only over the bar
/// between the elapsed and total-time labels.
pub fn player_click_target(
    area: Rect,
    state: &AppState,
    column: u16,
    row: u16,
) -> Option<PlayerClickTarget> {
    if state.overlays.error_message.is_some()
        || state.overlays.show_help
        || state.overlays.playlist_picker_visible
        || state.theme.picker_visible
    {
        return None;
    }

    let layout = app_layout(area, state);
    if contains(layout.tabs, column, row) {
        return tab_ranges(layout.tabs)
            .into_iter()
            .find(|(_, start, end)| column >= *start && column < *end)
            .map(|(tab, _, _)| PlayerClickTarget::Tab(tab));
    }

    let track = state.player.track.as_ref()?;
    let text = player_text_area(layout.player, state);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(text);

    if contains(rows[4], column, row) {
        let elapsed_width = format!("{} ", format_time(track.position)).chars().count() as u16;
        let total_width = format!(" {}", format_time(track.duration)).chars().count() as u16;
        let bar_width = rows[4]
            .width
            .saturating_sub(elapsed_width.saturating_add(total_width));
        let bar_x = rows[4].x.saturating_add(elapsed_width);
        if bar_width > 0 && column >= bar_x && column < bar_x.saturating_add(bar_width) {
            let fraction =
                f64::from(column - bar_x) / f64::from(bar_width.saturating_sub(1).max(1));
            return Some(PlayerClickTarget::Seek(
                (track.duration * fraction).clamp(0.0, track.duration),
            ));
        }
    }

    if contains(rows[6], column, row) {
        let shuffle = if state.player.shuffle_enabled {
            "⤮ on "
        } else {
            "⤮ off"
        };
        let repeat = format!("⟳ {}", state.player.repeat_mode.label());
        let volume = format!("Vol: {}%", state.player.volume);
        let controls_width =
            shuffle.chars().count() + 2 + repeat.chars().count() + 2 + volume.chars().count();
        let start = rows[6].x + rows[6].width.saturating_sub(controls_width as u16) / 2;
        let shuffle_end = start.saturating_add(shuffle.chars().count() as u16);
        let repeat_start = shuffle_end.saturating_add(2);
        let repeat_end = repeat_start.saturating_add(repeat.chars().count() as u16);
        if column >= start && column < shuffle_end {
            return Some(PlayerClickTarget::Shuffle);
        }
        if column >= repeat_start && column < repeat_end {
            return Some(PlayerClickTarget::Repeat);
        }
    }

    None
}

/// Resolve the scrollable region under a terminal cell and its maximum valid
/// offset. Wheel events over headers, artwork, tabs, or overlays are ignored.
pub fn scroll_target(area: Rect, state: &AppState, column: u16, row: u16) -> Option<ScrollHit> {
    if state.overlays.error_message.is_some()
        || state.overlays.show_help
        || state.overlays.playlist_picker_visible
        || state.theme.picker_visible
    {
        return None;
    }

    let content = app_layout(area, state).tab_content;
    let (target, list, len) = match state.active_tab {
        Tab::Queue => {
            let header = u16::from(!state.filter.query.is_empty() || state.filter.active);
            (
                ScrollTarget::Queue,
                list_area(content, header),
                filtered_track_len(&state.queue.tracks, &state.filter.query),
            )
        }
        Tab::Library => match &state.library.sub_view {
            LibrarySubView::Playlists => {
                let header = u16::from(!state.filter.query.is_empty() || state.filter.active);
                let len = if state.filter.query.is_empty() {
                    state.library.playlists.len()
                } else {
                    let query = state.filter.query.to_lowercase();
                    state
                        .library
                        .playlists
                        .iter()
                        .filter(|name| name.to_lowercase().contains(&query))
                        .count()
                };
                (
                    ScrollTarget::LibraryPlaylists,
                    list_area(content, header),
                    len,
                )
            }
            LibrarySubView::Tracks(_) => {
                let header = if state.filter.query.is_empty() && !state.filter.active {
                    1
                } else {
                    2
                };
                (
                    ScrollTarget::LibraryTracks,
                    list_area(content, header),
                    filtered_track_len(&state.library.tracks, &state.filter.query),
                )
            }
        },
        Tab::Search => (
            ScrollTarget::Search,
            list_area(content, 2),
            state.search.results.len(),
        ),
        Tab::Lyrics => (ScrollTarget::Lyrics, content, state.lyrics.lines.len()),
    };

    contains(list, column, row).then_some(ScrollHit {
        target,
        max_scroll: max_scroll(len, list),
    })
}

/// Resolve a terminal cell to an interactive list item. The returned indices
/// are display indices, matching each list's selection state even when a
/// queue/library filter maps them to different backend indices.
pub fn click_target(area: Rect, state: &AppState, column: u16, row: u16) -> Option<ClickTarget> {
    if state.overlays.error_message.is_some()
        || state.overlays.show_help
        || state.overlays.playlist_picker_visible
        || state.theme.picker_visible
    {
        return None;
    }

    let content = app_layout(area, state).tab_content;
    match state.active_tab {
        Tab::Queue => {
            let filtered_len = if state.filter.query.is_empty() {
                state.queue.tracks.len()
            } else {
                let query = state.filter.query.to_lowercase();
                state
                    .queue
                    .tracks
                    .iter()
                    .filter(|track| {
                        track.name.to_lowercase().contains(&query)
                            || track.artist.to_lowercase().contains(&query)
                    })
                    .count()
            };
            let has_filter = !state.filter.query.is_empty() || state.filter.active;
            let list = if has_filter {
                Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(content)[1]
            } else {
                content
            };
            item_at(list, state.queue.scroll, filtered_len, column, row).map(ClickTarget::QueueItem)
        }
        Tab::Library => match &state.library.sub_view {
            LibrarySubView::Playlists => {
                let filtered_len = if state.filter.query.is_empty() {
                    state.library.playlists.len()
                } else {
                    let query = state.filter.query.to_lowercase();
                    state
                        .library
                        .playlists
                        .iter()
                        .filter(|name| name.to_lowercase().contains(&query))
                        .count()
                };
                let has_filter = !state.filter.query.is_empty() || state.filter.active;
                let list = if has_filter {
                    Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(content)[1]
                } else {
                    content
                };
                item_at(list, state.library.scroll, filtered_len, column, row)
                    .map(ClickTarget::LibraryPlaylist)
            }
            LibrarySubView::Tracks(_) => {
                let filtered_len = if state.filter.query.is_empty() {
                    state.library.tracks.len()
                } else {
                    let query = state.filter.query.to_lowercase();
                    state
                        .library
                        .tracks
                        .iter()
                        .filter(|track| {
                            track.name.to_lowercase().contains(&query)
                                || track.artist.to_lowercase().contains(&query)
                        })
                        .count()
                };
                let has_filter = !state.filter.query.is_empty() || state.filter.active;
                let header_rows = if has_filter { 2 } else { 1 };
                let list = Rect::new(
                    content.x,
                    content.y.saturating_add(header_rows),
                    content.width,
                    content.height.saturating_sub(header_rows),
                );
                item_at(list, state.library.tracks_scroll, filtered_len, column, row)
                    .map(ClickTarget::LibraryTrack)
            }
        },
        Tab::Search => {
            let list = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(content)[2];
            item_at(
                list,
                state.search.scroll,
                state.search.results.len(),
                column,
                row,
            )
            .map(ClickTarget::SearchResult)
        }
        Tab::Lyrics => None,
    }
}

fn do_draw(
    f: &mut Frame,
    state: &AppState,
    theme: &Theme,
    artwork: &mut Option<Artwork>,
    effective_position: Option<f64>,
) -> Option<KittyPlacement> {
    let area = f.area();

    // Paint the complete frame with the theme background first. Without this,
    // ratatui leaves the terminal profile's background visible and shared
    // themes collapse to foreground colors only.
    f.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );

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
        .style(Style::default().bg(theme.background))
        .border_style(Style::default().fg(theme.border).bg(theme.background))
        .title(Span::styled(
            "  muse ♫ ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));

    f.render_widget(block, inner);

    let content_area = inner.inner(Margin::new(1, 1));

    // Split: player section | tab bar | gap | tab content | help line
    let chunks = Layout::vertical([
        Constraint::Length(player_height(state)),
        Constraint::Length(1), // tab bar
        Constraint::Length(1), // gap below tab bar
        Constraint::Min(3),    // tab content
        Constraint::Length(1), // help line
    ])
    .split(content_area);

    let kitty_placement =
        draw_player_section(f, chunks[0], state, theme, artwork, effective_position);
    draw_tab_bar(f, chunks[1], state, theme);
    // chunks[2] is the gap (empty)
    draw_tab_content(f, chunks[3], state, theme, effective_position);
    draw_help_line(f, chunks[4], state, theme);

    kitty_placement
}

fn selected_style(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.selection)
        .bg(theme.cursor_bg)
        .add_modifier(Modifier::BOLD)
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
    artwork: &mut Option<Artwork>,
    effective_position: Option<f64>,
) -> Option<KittyPlacement> {
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
        return None;
    }

    let Some(track) = &state.player.track else {
        let p = Paragraph::new(Span::styled(
            "No track playing",
            Style::default().fg(theme.text_dim),
        ))
        .alignment(Alignment::Center);
        f.render_widget(p, area);
        return None;
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
    // styling in this area that interferes with graphics protocols.
    //
    // For the hand-rolled Kitty path we don't draw a ratatui widget at all:
    // we mark those cells as skipped (so ratatui's diff doesn't overwrite the
    // image with blanks) and return a KittyPlacement for the main loop to emit
    // as an escape sequence after the frame flushes.
    let mut kitty_placement = None;
    if let Some(art_rect) = art_area {
        match artwork.as_mut() {
            Some(Artwork::Ratatui(proto)) => {
                f.render_widget(Clear, art_rect);
                let image = StatefulImage::default();
                f.render_stateful_widget(image, art_rect, proto);
            }
            Some(Artwork::Kitty(_)) => {
                f.render_widget(Clear, art_rect);
                mark_skip(f, art_rect);
                kitty_placement = Some(KittyPlacement { rect: art_rect });
            }
            None => {}
        }
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
        Paragraph::new(Span::styled(subtitle, Style::default().fg(theme.time_text)))
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
    let bar_width = (rows[4].width as usize).saturating_sub(elapsed_str.len() + total_str.len());
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

    kitty_placement
}

/// Mark every cell in `rect` as skipped so ratatui's buffer diff leaves them
/// untouched. This protects a hand-rolled Kitty image drawn into that region
/// from being overwritten with blanks on subsequent frames.
fn mark_skip(f: &mut Frame, rect: Rect) {
    let buf = f.buffer_mut();
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_skip(true);
            }
        }
    }
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
        .padding(TAB_PADDING_LEFT, TAB_PADDING_RIGHT)
        .divider(Span::styled(
            TAB_DIVIDER,
            Style::default().fg(theme.text_muted),
        ))
        .highlight_style(
            Style::default()
                .fg(theme.selection)
                .add_modifier(Modifier::BOLD),
        )
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
        .style(Style::default().bg(theme.cursor_bg))
        .title(Span::styled(
            " Error ",
            Style::default()
                .fg(theme.error)
                .add_modifier(Modifier::BOLD),
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
        Style::default()
            .fg(theme.text_dim)
            .add_modifier(Modifier::DIM),
    )));

    f.render_widget(Paragraph::new(body).alignment(Alignment::Center), inner);
}

fn draw_filter_bar(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let line = Line::from(vec![
        Span::styled(
            "/ ",
            Style::default().fg(theme.key).add_modifier(Modifier::BOLD),
        ),
        if state.filter.active {
            Span::styled(
                format!("{}▏", state.filter.query),
                Style::default().fg(theme.accent),
            )
        } else {
            Span::styled(
                state.filter.query.clone(),
                Style::default().fg(theme.accent),
            )
        },
        Span::styled(
            "  Esc to clear",
            Style::default()
                .fg(theme.text_dim)
                .add_modifier(Modifier::DIM),
        ),
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
        state
            .queue
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.name.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..state.queue.tracks.len()).collect()
    };

    if filtered.is_empty() && has_filter {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No matches",
                Style::default().fg(theme.text_dim),
            ))
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
                selected_style(theme)
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
                let rows =
                    Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
                draw_filter_bar(f, rows[0], state, theme);
                let q = state.filter.query.to_lowercase();
                let indices: Vec<usize> = state
                    .library
                    .playlists
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.to_lowercase().contains(&q))
                    .map(|(i, _)| i)
                    .collect();
                (rows[1], indices)
            } else {
                (area, (0..state.library.playlists.len()).collect())
            };

            if filtered.is_empty() && has_filter {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        "No matches",
                        Style::default().fg(theme.text_dim),
                    ))
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
                        selected_style(theme)
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
                    state
                        .library
                        .tracks
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| {
                            t.name.to_lowercase().contains(&q)
                                || t.artist.to_lowercase().contains(&q)
                        })
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    (0..state.library.tracks.len()).collect()
                };

                if filtered.is_empty() && has_filter {
                    f.render_widget(
                        Paragraph::new(Span::styled(
                            "No matches",
                            Style::default().fg(theme.text_dim),
                        ))
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
                            selected_style(theme)
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

                let mut list_state = ListState::default().with_offset(state.library.tracks_scroll);
                f.render_stateful_widget(List::new(items), list_area, &mut list_state);
            }
        }
    }
}

fn draw_search(f: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(area);

    // Search input — accent color + cursor when editing, dimmed when browsing
    if state.search.editing {
        let search_line = Line::from(vec![
            Span::styled(
                "/ ",
                Style::default().fg(theme.key).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}▏", state.search.query),
                Style::default().fg(theme.accent),
            ),
            Span::styled(
                "  Enter to browse · Esc to stop",
                Style::default()
                    .fg(theme.text_dim)
                    .add_modifier(Modifier::DIM),
            ),
        ]);
        f.render_widget(Paragraph::new(search_line), rows[0]);
    } else {
        let search_line = Line::from(vec![
            Span::styled(
                format!("/ {}", state.search.query),
                Style::default().fg(theme.text_dim),
            ),
            Span::styled(
                "  / to edit",
                Style::default()
                    .fg(theme.text_dim)
                    .add_modifier(Modifier::DIM),
            ),
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
                selected_style(theme)
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
    let max_name_len = state
        .theme
        .all
        .iter()
        .map(|(n, _)| n.len())
        .max()
        .unwrap_or(10);
    let popup_w = (max_name_len as u16 + 8).min(area.width.saturating_sub(4)); // "▸ " + name + " ✓" + padding
    let popup_h = (state.theme.all.len() as u16 + 2).min(area.height.saturating_sub(2)); // +2 for border

    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    // Clear the area behind the popup
    f.render_widget(Clear, popup);

    let block = Block::default()
        .style(Style::default().bg(theme.background))
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
            let check = if *name == state.theme.name {
                " ✓"
            } else {
                ""
            };
            let style = if i == state.theme.selected {
                selected_style(theme)
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
                Style::default().fg(theme.key).add_modifier(Modifier::BOLD),
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
    let bindings_area = Rect::new(
        centered.x,
        centered.y + 2,
        centered.width,
        centered.height.saturating_sub(2),
    );
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
                selected_style(theme)
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
                Style::default()
                    .fg(theme.text_dim)
                    .add_modifier(Modifier::DIM),
            )),
            area,
        );
    } else {
        let padding = area.width as usize
            - left.len().min(area.width as usize)
            - right.len().min(area.width as usize);
        let line = Line::from(vec![
            Span::styled(
                left,
                Style::default()
                    .fg(theme.text_dim)
                    .add_modifier(Modifier::DIM),
            ),
            Span::raw(" ".repeat(padding.max(1))),
            Span::styled(
                right.as_str(),
                Style::default()
                    .fg(theme.text_dim)
                    .add_modifier(Modifier::DIM),
            ),
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

#[cfg(test)]
mod tests {
    use super::{
        app_layout, click_target, player_click_target, scroll_target, tab_ranges, ClickTarget,
        PlayerClickTarget, ScrollHit, ScrollTarget,
    };
    use crate::backend::{PlaylistTrack, SearchResult};
    use crate::state::{AppState, LibrarySubView, Tab};
    use ratatui::layout::Rect;

    fn track(name: &str) -> PlaylistTrack {
        PlaylistTrack {
            name: name.to_string(),
            artist: "artist".to_string(),
            album: "album".to_string(),
            duration: 180.0,
        }
    }

    #[test]
    fn every_rendered_tab_label_is_clickable() {
        let area = Rect::new(0, 0, 100, 40);
        let state = AppState::default();
        let tabs = app_layout(area, &state).tabs;
        let backend = ratatui::backend::TestBackend::new(area.width, area.height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::draw_tab_bar(frame, tabs, &state, &crate::theme::default_theme()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered: String = (tabs.left()..tabs.right())
            .map(|x| buffer[(x, tabs.y)].symbol())
            .collect();

        for tab in Tab::ALL {
            let label_start = rendered
                .find(tab.label())
                .unwrap_or_else(|| panic!("{} was not rendered", tab.label()))
                as u16;
            let column = tabs.x + label_start;
            assert_eq!(
                player_click_target(area, &state, column, tabs.y),
                Some(PlayerClickTarget::Tab(*tab)),
                "{} should be clickable",
                tab.label()
            );
        }

        let divider = tab_ranges(tabs)[0].2;
        assert_eq!(player_click_target(area, &state, divider, tabs.y), None);
    }

    #[test]
    fn player_controls_and_progress_are_clickable() {
        let area = Rect::new(0, 0, 100, 40);
        let mut state = AppState::default();
        state.player.track = Some(crate::backend::Track {
            name: "track".to_string(),
            artist: "artist".to_string(),
            album: "album".to_string(),
            duration: 240.0,
            position: 60.0,
        });
        let player = app_layout(area, &state).player;
        let text = super::player_text_area(player, &state);
        let progress_row = text.y + 4;
        let bar_start = text.x + 5; // "1:00 "
        let controls_row = text.y + 6;

        assert_eq!(
            player_click_target(area, &state, bar_start, progress_row),
            Some(PlayerClickTarget::Seek(0.0))
        );
        assert!(matches!(
            player_click_target(area, &state, text.right() - 6, progress_row),
            Some(PlayerClickTarget::Seek(position)) if position > 230.0
        ));
        let controls_start = text.x + text.width.saturating_sub(22) / 2;
        assert_eq!(
            player_click_target(area, &state, controls_start, controls_row),
            Some(PlayerClickTarget::Shuffle)
        );
        assert_eq!(
            player_click_target(area, &state, controls_start + 7, controls_row),
            Some(PlayerClickTarget::Repeat)
        );
    }

    #[test]
    fn queue_click_accounts_for_filter_row_and_scroll() {
        let area = Rect::new(0, 0, 100, 40);
        let mut state = AppState::default();
        state.active_tab = Tab::Queue;
        state.filter.active = true;
        state.queue.tracks = (0..12).map(|i| track(&format!("track {i}"))).collect();
        state.queue.scroll = 4;
        let list = app_layout(area, &state).tab_content;

        assert_eq!(
            click_target(area, &state, list.x, list.y + 1),
            Some(ClickTarget::QueueItem(4))
        );
        assert_eq!(click_target(area, &state, list.x, list.y), None);
    }

    #[test]
    fn library_track_click_skips_header_and_filter_rows() {
        let area = Rect::new(0, 0, 100, 40);
        let mut state = AppState::default();
        state.active_tab = Tab::Library;
        state.filter.active = true;
        state.library.sub_view = LibrarySubView::Tracks("playlist".to_string());
        state.library.tracks = (0..10).map(|i| track(&format!("track {i}"))).collect();
        state.library.tracks_scroll = 3;
        let list = app_layout(area, &state).tab_content;

        assert_eq!(
            click_target(area, &state, list.x, list.y + 2),
            Some(ClickTarget::LibraryTrack(3))
        );
        assert_eq!(click_target(area, &state, list.x, list.y + 1), None);
    }

    #[test]
    fn search_click_skips_input_and_gap() {
        let area = Rect::new(0, 0, 100, 40);
        let mut state = AppState::default();
        state.active_tab = Tab::Search;
        state.search.results = (0..8)
            .map(|i| SearchResult {
                name: format!("result {i}"),
                artist: "artist".to_string(),
                album: "album".to_string(),
            })
            .collect();
        state.search.scroll = 2;
        let list = app_layout(area, &state).tab_content;

        assert_eq!(
            click_target(area, &state, list.x, list.y + 2),
            Some(ClickTarget::SearchResult(2))
        );
        assert_eq!(click_target(area, &state, list.x, list.y + 1), None);
    }

    #[test]
    fn wheel_target_is_bounded_by_visible_filtered_rows() {
        let area = Rect::new(0, 0, 100, 25);
        let mut state = AppState::default();
        state.active_tab = Tab::Queue;
        state.queue.tracks = (0..30).map(|i| track(&format!("track {i}"))).collect();
        state.filter.query = "track 1".to_string(); // 11 matches
        let content = app_layout(area, &state).tab_content;
        let list_height = content.height.saturating_sub(1);

        assert_eq!(
            scroll_target(area, &state, content.x, content.y + 1),
            Some(ScrollHit {
                target: ScrollTarget::Queue,
                max_scroll: 11usize.saturating_sub(usize::from(list_height)),
            })
        );
        assert_eq!(scroll_target(area, &state, content.x, content.y), None);
    }

    #[test]
    fn lyrics_content_is_scrollable() {
        let area = Rect::new(0, 0, 100, 25);
        let mut state = AppState::default();
        state.active_tab = Tab::Lyrics;
        state.lyrics.lines = (0..30)
            .map(|i| crate::backend::LyricsLine {
                text: format!("line {i}"),
                time: None,
            })
            .collect();
        let content = app_layout(area, &state).tab_content;

        assert_eq!(
            scroll_target(area, &state, content.x, content.y),
            Some(ScrollHit {
                target: ScrollTarget::Lyrics,
                max_scroll: 30usize.saturating_sub(usize::from(content.height)),
            })
        );
    }

    #[test]
    fn overlays_block_underlying_clicks() {
        let area = Rect::new(0, 0, 100, 40);
        let mut state = AppState::default();
        state.queue.tracks.push(track("track"));
        state.overlays.show_help = true;
        let list = app_layout(area, &state).tab_content;

        assert_eq!(click_target(area, &state, list.x, list.y), None);
    }
}
