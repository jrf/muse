//! Event handlers extracted from main.rs.
//!
//! This module owns:
//! - The `AppEvent` enum (messages flowing through the main loop's mpsc channel).
//! - Keyboard handlers for each tab and overlay.
//! - State-application handlers (`apply_fresh_state`, `handle_notification`).
//! - Small navigation/filter helpers used by the handlers.
//! - The `fire_and_refresh` and `perform_search` thread spawners.
//!
//! `run_app` in main.rs is the only caller of `handle_key`, `apply_fresh_state`,
//! and `handle_notification`. Everything else here is internal.

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::ImageReader;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

use crate::backend::{self, MusicBackend};
use crate::playlist;
use crate::state::{AppState, LibrarySubView, Tab};
use crate::theme::{self, Theme};

const PAGE_SIZE: usize = 20;

/// Events sent to the main loop.
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    MusicNotification(backend::NotificationInfo),
    StateRefreshed(backend::FullState),
    PlaylistsLoaded(Vec<String>),
    PlaylistTracksLoaded(String, Vec<backend::PlaylistTrack>),
    SearchResults(String, Vec<backend::SearchResult>),
    LyricsLoaded(String, Option<backend::LyricsResult>),
    ArtworkLoaded(String, StatefulProtocol),
    LastfmScrobbleResult(Result<(), String>),
}

// MARK: - Navigation helpers

/// Map vim-style key combos to equivalent navigation keys.
/// When `vim_letters` is true, also map j/k/g/G to arrow keys.
/// Pass false when the user is typing into a text field.
pub fn normalize_nav_key(key: &KeyEvent, vim_letters: bool) -> KeyCode {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('f') => KeyCode::PageDown,
            KeyCode::Char('b') => KeyCode::PageUp,
            _ => key.code,
        }
    } else if vim_letters {
        match key.code {
            KeyCode::Char('j') => KeyCode::Down,
            KeyCode::Char('k') => KeyCode::Up,
            KeyCode::Char('g') => KeyCode::Home,
            KeyCode::Char('G') => KeyCode::End,
            _ => key.code,
        }
    } else {
        key.code
    }
}

/// Navigate a list selection with scroll tracking. Returns (new_selected, new_scroll).
pub fn list_nav(code: KeyCode, selected: usize, scroll: usize, len: usize) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    let visible = PAGE_SIZE;
    let new_sel = match code {
        KeyCode::Up => selected.saturating_sub(1),
        KeyCode::Down => (selected + 1).min(last),
        KeyCode::Home => 0,
        KeyCode::End => last,
        KeyCode::PageUp => selected.saturating_sub(visible),
        KeyCode::PageDown => (selected + visible).min(last),
        _ => return None,
    };
    let new_scroll = if new_sel < scroll {
        new_sel
    } else if new_sel >= scroll + visible {
        new_sel - (visible - 1)
    } else {
        scroll
    };
    Some((new_sel, new_scroll))
}

/// Return indices of items whose text matches the filter query (case-insensitive substring).
pub fn filter_track_indices(tracks: &[backend::PlaylistTrack], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..tracks.len()).collect();
    }
    let q = query.to_lowercase();
    tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            t.name.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}

pub fn filter_string_indices(items: &[String], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..items.len()).collect();
    }
    let q = query.to_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, s)| s.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

fn clear_filter(state: &mut AppState) {
    state.filter.query.clear();
    state.filter.active = false;
}

// MARK: - State application

pub fn apply_fresh_state(
    state: &mut AppState,
    artwork: &mut Option<StatefulProtocol>,
    fresh: &backend::FullState,
    picker: &Option<Arc<Picker>>,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    let old_art_key = state.player.artwork_key.clone();

    // Don't flip to "not running" if we were previously running — could be a
    // transient error during track transitions.  Only mark not running if we
    // also had no track before (i.e. we never connected).
    if fresh.music_running || !state.player.music_running {
        state.player.music_running = fresh.music_running;
    }

    if fresh.music_running {
        state.player.volume = fresh.volume;
        state.player.shuffle_enabled = fresh.shuffle_enabled;
        state.player.repeat_mode = fresh.repeat_mode;
        state.player.current_track_favorited = fresh.track_favorited;
        // Only update track/player_state with concrete data.
        // During transitions, keep showing the previous track.
        if let Some(ref track) = fresh.track {
            // If track just finished (same track, near end, no longer playing),
            // snap position to duration so the progress bar shows completion.
            let was_playing = state.player.playback == backend::PlayerState::Playing;
            let is_no_longer_playing = fresh.player_state != backend::PlayerState::Playing;
            let is_same_track = state
                .player
                .track
                .as_ref()
                .map_or(false, |t| t.name == track.name && t.artist == track.artist);
            let near_end = track.duration > 0.0 && (track.duration - track.position) < 5.0;

            let mut updated_track = track.clone();
            if was_playing && is_no_longer_playing && is_same_track && near_end {
                updated_track.position = updated_track.duration;
            }

            state.player.track = Some(updated_track);
            state.player.playback = fresh.player_state;
        } else if state.player.track.is_none() {
            // No previous track either — show whatever state we got
            state.player.playback = fresh.player_state;
        }
        // If fresh has no track but we had one, keep the old track visible
        // and don't update player_state (it may transiently say "stopped")
    }

    // Fetch artwork when track changes
    let new_art_key = fresh
        .track
        .as_ref()
        .map(|t| format!("{}\t{}", t.artist, t.album))
        .unwrap_or_default();
    if new_art_key != old_art_key && !new_art_key.is_empty() {
        state.player.artwork_key = new_art_key.clone();
        // Keep old artwork visible until new one arrives — don't set to None
        if let Some(ref picker) = picker {
            let tx2 = tx.clone();
            let picker = picker.clone();
            let b = backend.clone();
            std::thread::spawn(move || {
                if let Some(proto) = fetch_artwork(&picker, &*b) {
                    let _ = tx2.send(AppEvent::ArtworkLoaded(new_art_key, proto));
                }
            });
        }
    } else if new_art_key.is_empty() {
        state.player.artwork_key.clear();
        *artwork = None;
    }
}

pub fn handle_notification(
    state: &mut AppState,
    info: &backend::NotificationInfo,
    picker: &Option<Arc<Picker>>,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    match info.player_state.as_str() {
        "Playing" => state.player.playback = backend::PlayerState::Playing,
        "Paused" | "Stopped" => {
            let was_playing = state.player.playback == backend::PlayerState::Playing;
            let is_same_track = !info.name.is_empty()
                && state
                    .player
                    .track
                    .as_ref()
                    .map_or(false, |t| t.name == info.name && t.artist == info.artist);
            let near_end = state
                .player
                .track
                .as_ref()
                .map_or(false, |t| t.duration > 0.0 && (t.duration - t.position) < 5.0);

            // Snap position to duration so progress bar shows completion.
            if is_same_track && near_end {
                if let Some(ref mut t) = state.player.track {
                    t.position = t.duration;
                }
            }

            if info.player_state == "Stopped" && !info.name.is_empty() {
                // Don't immediately mark stopped during transitions
            } else if info.player_state == "Stopped" {
                state.player.playback = backend::PlayerState::Stopped;
            } else {
                state.player.playback = backend::PlayerState::Paused;
            }

            // Auto-advance for Apple Music when a track finishes naturally.
            // Guard: was_playing prevents re-entry (state is already
            // updated above so a second notification won't fire again).
            // Spotify manages its own queue natively — no intervention needed.
            if was_playing && is_same_track && near_end && backend.needs_queue_advance() {
                let playing_idx = state.queue.playing.unwrap_or(state.queue.selected);
                if !state.queue.tracks.is_empty()
                    && playing_idx + 1 < state.queue.tracks.len()
                {
                    let next_idx = playing_idx + 1;
                    state.queue.playing = Some(next_idx);
                    playlist::save_queue_state(&state.queue.playlist_name, next_idx, state.queue.tracks.len());
                    if state.queue.selected == playing_idx {
                        state.queue.selected = next_idx;
                    }
                    if next_idx >= state.queue.scroll + PAGE_SIZE {
                        state.queue.scroll = next_idx.saturating_sub(3);
                    }
                    let playlist = state.queue.playlist_name.clone();
                    fire_and_refresh(backend, tx, move |b| {
                        b.play_track_in_playlist(&playlist, next_idx)
                    });
                } else {
                    fire_and_refresh(backend, tx, |b| b.next_track());
                }
            }
        }
        _ => {}
    }

    if !info.name.is_empty() {
        let is_new = state
            .player
            .track
            .as_ref()
            .map_or(true, |t| t.name != info.name || t.artist != info.artist);

        state.player.track = Some(backend::Track {
            name: info.name.clone(),
            artist: info.artist.clone(),
            album: info.album.clone(),
            duration: info.total_time_ms / 1000.0,
            position: if is_new {
                0.0
            } else {
                state.player.track.as_ref().map_or(0.0, |t| t.position)
            },
        });

        // Sync queue_playing if the new track matches a queue entry
        // (handles CLI next/prev while TUI is running).
        if is_new && !state.queue.tracks.is_empty() {
            if let Some(pos) = playlist::sync_queue_selection(
                &state.queue.tracks,
                &state.queue.playlist_name,
                &info.name,
                &info.artist,
            ) {
                state.queue.playing = Some(pos);
            }
        }

        if is_new {
            let new_key = format!("{}\t{}", info.artist, info.album);
            if new_key != state.player.artwork_key {
                state.player.artwork_key = new_key.clone();
                if let Some(ref picker) = picker {
                    let tx2 = tx.clone();
                    let picker = picker.clone();
                    let b = backend.clone();
                    std::thread::spawn(move || {
                        if let Some(proto) = fetch_artwork(&picker, &*b) {
                            let _ = tx2.send(AppEvent::ArtworkLoaded(new_key, proto));
                        }
                    });
                }
            }
        }
    }

    state.player.music_running = true;
}

// MARK: - Key handling

/// Returns true if the app should quit.
pub fn handle_key(
    key: KeyEvent,
    state: &mut AppState,
    theme: &mut Theme,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) -> bool {
    // Error overlay — any key dismisses
    if state.overlays.error_message.is_some() {
        state.overlays.error_message = None;
        return false;
    }

    // Help overlay
    if key.code == KeyCode::Char('?') {
        state.overlays.show_help = !state.overlays.show_help;
        return false;
    }
    if state.overlays.show_help {
        state.overlays.show_help = false;
        return false;
    }

    // Theme picker overlay
    if state.theme.picker_visible {
        handle_theme_picker_key(key, state, theme);
        return false;
    }

    // Playlist picker
    if state.overlays.playlist_picker_visible {
        handle_playlist_picker_key(key, state, tx, backend);
        return false;
    }

    let in_search = state.active_tab == Tab::Search && state.search.editing;
    let in_filter = state.filter.active;
    let text_input = in_search || in_filter;

    // Global keys
    match key.code {
        KeyCode::Char('q') if !text_input => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Tab => {
            clear_filter(state);
            state.active_tab = state.active_tab.next();
            return false;
        }
        KeyCode::BackTab => {
            clear_filter(state);
            state.active_tab = state.active_tab.prev();
            return false;
        }
        KeyCode::Char('t') if !text_input => {
            state.theme.picker_visible = !state.theme.picker_visible;
            if state.theme.picker_visible {
                if let Some((idx, _)) = theme::find_theme(&state.theme.name, &state.theme.all) {
                    state.theme.selected = idx;
                } else {
                    state.theme.selected = 0;
                }
                state.theme.scroll = 0;
            } else {
                restore_saved_theme(state, theme);
            }
            return false;
        }
        KeyCode::Char('l') if !text_input => {
            clear_filter(state);
            state.active_tab = Tab::Library;
            return false;
        }
        KeyCode::Char('L') if !text_input => {
            state.active_tab = Tab::Lyrics;
            return false;
        }
        KeyCode::Char('/') if state.active_tab != Tab::Queue && state.active_tab != Tab::Library => {
            state.active_tab = Tab::Search;
            state.search.editing = true;
            return false;
        }
        KeyCode::Char(' ') if !text_input => {
            state.player.playback = if state.player.playback == backend::PlayerState::Playing {
                backend::PlayerState::Paused
            } else {
                backend::PlayerState::Playing
            };
            fire_and_refresh(backend, tx, |b| b.play_pause());
            return false;
        }
        KeyCode::Char('n') if !text_input => {
            fire_and_refresh(backend, tx, |b| b.next_track());
            return false;
        }
        KeyCode::Char('p') if !text_input => {
            fire_and_refresh(backend, tx, |b| b.previous_track());
            return false;
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            state.player.volume = (state.player.volume + 5).min(100);
            let vol = state.player.volume;
            let b = backend.clone();
            std::thread::spawn(move || b.set_volume(vol));
            return false;
        }
        KeyCode::Char('-') if !text_input => {
            state.player.volume = (state.player.volume - 5).max(0);
            let vol = state.player.volume;
            let b = backend.clone();
            std::thread::spawn(move || b.set_volume(vol));
            return false;
        }
        KeyCode::Char('s') if !text_input => {
            state.player.shuffle_enabled = !state.player.shuffle_enabled;
            fire_and_refresh(backend, tx, |b| b.toggle_shuffle());
            return false;
        }
        KeyCode::Char('r') if !text_input => {
            state.player.repeat_mode = match state.player.repeat_mode {
                backend::RepeatMode::Off => backend::RepeatMode::All,
                backend::RepeatMode::All => backend::RepeatMode::One,
                backend::RepeatMode::One => backend::RepeatMode::Off,
            };
            fire_and_refresh(backend, tx, |b| b.cycle_repeat());
            return false;
        }
        KeyCode::Char('C') if !text_input => {
            state.queue.tracks.clear();
            state.queue.selected = 0;
            state.queue.scroll = 0;
            state.queue.playing = None;
            state.queue.playlist_name.clear();
            playlist::clear_queue_state();
            return false;
        }
        KeyCode::Char('f') if !text_input && !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.player.current_track_favorited = !state.player.current_track_favorited;
            fire_and_refresh(backend, tx, |b| b.toggle_favorite());
            return false;
        }
        KeyCode::Char('P') if !text_input => {
            state.overlays.playlist_picker_visible = !state.overlays.playlist_picker_visible;
            state.overlays.playlist_picker_selected = 0;
            state.overlays.playlist_picker_scroll = 0;
            return false;
        }
        KeyCode::Char('a') if !text_input => {
            if let Some(artist) = state.player.track.as_ref().map(|t| t.artist.clone()) {
                if !artist.is_empty() {
                    state.active_tab = Tab::Search;
                    state.search.editing = false;
                    state.search.query = artist;
                    state.search.selected = 0;
                    state.search.scroll = 0;
                    perform_search(state, tx, backend);
                }
            }
            return false;
        }
        KeyCode::Char('A') => {
            if let Some(album) = state.player.track.as_ref().map(|t| t.album.clone()) {
                if !album.is_empty() {
                    state.active_tab = Tab::Search;
                    state.search.editing = false;
                    state.search.query = album;
                    state.search.selected = 0;
                    state.search.scroll = 0;
                    perform_search(state, tx, backend);
                }
            }
            return false;
        }
        KeyCode::Char('o') if !text_input => {
            if let Some(artist) = state.player.track.as_ref().map(|t| t.artist.clone()) {
                if !artist.is_empty() {
                    let b = backend.clone();
                    std::thread::spawn(move || b.reveal_artist(&artist));
                }
            }
            return false;
        }
        KeyCode::Char('O') if !text_input => {
            if let Some(track) = state.player.track.clone() {
                if !track.album.is_empty() {
                    let b = backend.clone();
                    std::thread::spawn(move || {
                        b.reveal_album(&track.album, &track.artist)
                    });
                }
            }
            return false;
        }
        _ => {}
    }

    // Tab-specific keys
    match state.active_tab {
        Tab::Queue => handle_queue_key(key, state, tx, backend),
        Tab::Library => handle_library_key(key, state, tx, backend),
        Tab::Search => handle_search_key(key, state, tx, backend),
        Tab::Lyrics => handle_lyrics_key(key, state),
    }

    false
}

fn handle_queue_key(
    key: KeyEvent,
    state: &mut AppState,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    if state.filter.active {
        match key.code {
            KeyCode::Esc => {
                clear_filter(state);
                return;
            }
            KeyCode::Backspace => {
                state.filter.query.pop();
                if state.filter.query.is_empty() {
                    state.filter.active = false;
                }
                state.queue.selected = 0;
                state.queue.scroll = 0;
                return;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.filter.query.push(ch);
                state.queue.selected = 0;
                state.queue.scroll = 0;
                return;
            }
            _ => {}
        }
    }

    let filtered = filter_track_indices(&state.queue.tracks, &state.filter.query);
    let has_filter = !state.filter.query.is_empty();

    let nav_len = if has_filter { filtered.len() } else { state.queue.tracks.len() };
    let vim_nav = !state.filter.active;
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, vim_nav), state.queue.selected, state.queue.scroll, nav_len) {
        state.queue.selected = sel;
        state.queue.scroll = scr;
        return;
    }

    match key.code {
        KeyCode::Char('/') => {
            state.filter.active = true;
            state.filter.query.clear();
            state.queue.selected = 0;
            state.queue.scroll = 0;
        }
        KeyCode::Enter => {
            let real_idx = if has_filter {
                filtered.get(state.queue.selected).copied()
            } else if state.queue.selected < state.queue.tracks.len() {
                Some(state.queue.selected)
            } else {
                None
            };
            if let Some(idx) = real_idx {
                state.queue.playing = Some(idx);
                playlist::save_queue_state(&state.queue.playlist_name, idx, state.queue.tracks.len());
                let playlist = state.queue.playlist_name.clone();
                fire_and_refresh(backend, tx, move |b| b.play_track_in_playlist(&playlist, idx));
                state.filter.active = false;
            }
        }
        KeyCode::Char('d') | KeyCode::Char('x') if !state.filter.active => {
            let real_idx = if has_filter {
                filtered.get(state.queue.selected).copied()
            } else if state.queue.selected < state.queue.tracks.len() {
                Some(state.queue.selected)
            } else {
                None
            };
            if let Some(removed) = real_idx {
                state.queue.tracks.remove(removed);
                if state.queue.tracks.is_empty() {
                    state.queue.selected = 0;
                    state.queue.scroll = 0;
                    state.queue.playing = None;
                    state.queue.playlist_name.clear();
                    playlist::clear_queue_state();
                    clear_filter(state);
                } else {
                    let new_filtered = filter_track_indices(&state.queue.tracks, &state.filter.query);
                    if state.queue.selected >= new_filtered.len() && !new_filtered.is_empty() {
                        state.queue.selected = new_filtered.len() - 1;
                    }
                    if let Some(ref mut pi) = state.queue.playing {
                        if removed < *pi {
                            *pi -= 1;
                        } else if removed == *pi {
                            state.queue.playing = None;
                        }
                    }
                    let persist_idx = state.queue.playing.unwrap_or(0);
                    playlist::save_queue_state(&state.queue.playlist_name, persist_idx, state.queue.tracks.len());
                }
            }
        }
        KeyCode::Esc if has_filter => {
            clear_filter(state);
        }
        _ => {}
    }
}

fn handle_library_key(
    key: KeyEvent,
    state: &mut AppState,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    if state.filter.active {
        match key.code {
            KeyCode::Esc => {
                clear_filter(state);
                return;
            }
            KeyCode::Backspace => {
                state.filter.query.pop();
                if state.filter.query.is_empty() {
                    state.filter.active = false;
                }
                match &state.library.sub_view {
                    LibrarySubView::Playlists => {
                        state.library.selected = 0;
                        state.library.scroll = 0;
                    }
                    LibrarySubView::Tracks(_) => {
                        state.library.tracks_selected = 0;
                        state.library.tracks_scroll = 0;
                    }
                }
                return;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.filter.query.push(ch);
                match &state.library.sub_view {
                    LibrarySubView::Playlists => {
                        state.library.selected = 0;
                        state.library.scroll = 0;
                    }
                    LibrarySubView::Tracks(_) => {
                        state.library.tracks_selected = 0;
                        state.library.tracks_scroll = 0;
                    }
                }
                return;
            }
            _ => {}
        }
    }

    let has_filter = !state.filter.query.is_empty();

    match state.library.sub_view.clone() {
        LibrarySubView::Playlists => {
            let filtered = filter_string_indices(&state.library.playlists, &state.filter.query);
            let nav_len = if has_filter { filtered.len() } else { state.library.playlists.len() };
            let vim_nav = !state.filter.active;
            if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, vim_nav), state.library.selected, state.library.scroll, nav_len) {
                state.library.selected = sel;
                state.library.scroll = scr;
                return;
            }
            match key.code {
                KeyCode::Char('/') => {
                    state.filter.active = true;
                    state.filter.query.clear();
                    state.library.selected = 0;
                    state.library.scroll = 0;
                }
                KeyCode::Enter => {
                    let real_idx = if has_filter {
                        filtered.get(state.library.selected).copied()
                    } else if state.library.selected < state.library.playlists.len() {
                        Some(state.library.selected)
                    } else {
                        None
                    };
                    if let Some(idx) = real_idx {
                        let name = state.library.playlists[idx].clone();
                        state.queue.tracks.clear();
                        state.queue.playlist_name = name.clone();
                        state.queue.selected = 0;
                        state.queue.scroll = 0;
                        state.queue.playing = Some(0);
                        playlist::save_queue_state(&name, 0, 0);
                        state.filter.active = false;
                        let play_name = name.clone();
                        fire_and_refresh(backend, tx, move |b| {
                            b.play_track_in_playlist(&play_name, 0)
                        });
                        let tx2 = tx.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || {
                            let tracks = b.get_playlist_tracks(&name);
                            let _ = tx2.send(AppEvent::PlaylistTracksLoaded(name, tracks));
                        });
                    }
                }
                KeyCode::Right => {
                    let real_idx = if has_filter {
                        filtered.get(state.library.selected).copied()
                    } else if state.library.selected < state.library.playlists.len() {
                        Some(state.library.selected)
                    } else {
                        None
                    };
                    if let Some(idx) = real_idx {
                        let name = state.library.playlists[idx].clone();
                        state.library.sub_view = LibrarySubView::Tracks(name.clone());
                        state.library.tracks.clear();
                        state.library.tracks_selected = 0;
                        state.library.tracks_scroll = 0;
                        state.filter.active = false;
                        let tx2 = tx.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || {
                            let tracks = b.get_playlist_tracks(&name);
                            let _ = tx2.send(AppEvent::PlaylistTracksLoaded(name, tracks));
                        });
                    }
                }
                KeyCode::Char('Q') => {
                    let real_idx = if has_filter {
                        filtered.get(state.library.selected).copied()
                    } else if state.library.selected < state.library.playlists.len() {
                        Some(state.library.selected)
                    } else {
                        None
                    };
                    if let Some(idx) = real_idx {
                        let name = state.library.playlists[idx].clone();
                        state.queue.tracks.clear();
                        state.queue.playlist_name = name.clone();
                        state.queue.selected = 0;
                        state.queue.scroll = 0;
                        state.queue.playing = None;
                        playlist::save_queue_state(&name, 0, 0);
                        state.filter.active = false;
                        state.active_tab = Tab::Queue;
                        let tx2 = tx.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || {
                            let tracks = b.get_playlist_tracks(&name);
                            let _ = tx2.send(AppEvent::PlaylistTracksLoaded(name, tracks));
                        });
                    }
                }
                KeyCode::Esc if has_filter => {
                    clear_filter(state);
                }
                _ => {}
            }
        }
        LibrarySubView::Tracks(ref playlist_name) => {
            let filtered = filter_track_indices(&state.library.tracks, &state.filter.query);
            let nav_len = if has_filter { filtered.len() } else { state.library.tracks.len() };
            let vim_nav = !state.filter.active;
            if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, vim_nav), state.library.tracks_selected, state.library.tracks_scroll, nav_len) {
                state.library.tracks_selected = sel;
                state.library.tracks_scroll = scr;
                return;
            }
            match key.code {
                KeyCode::Char('/') => {
                    state.filter.active = true;
                    state.filter.query.clear();
                    state.library.tracks_selected = 0;
                    state.library.tracks_scroll = 0;
                }
                KeyCode::Backspace if !state.filter.active && !has_filter => {
                    clear_filter(state);
                    state.library.sub_view = LibrarySubView::Playlists;
                }
                KeyCode::Enter => {
                    let real_idx = if has_filter {
                        filtered.get(state.library.tracks_selected).copied()
                    } else if state.library.tracks_selected < state.library.tracks.len() {
                        Some(state.library.tracks_selected)
                    } else {
                        None
                    };
                    if let Some(idx) = real_idx {
                        state.queue.tracks = state.library.tracks.clone();
                        state.queue.playlist_name = playlist_name.clone();
                        state.queue.selected = idx;
                        state.queue.playing = Some(idx);
                        if idx < state.queue.scroll || idx >= state.queue.scroll + PAGE_SIZE {
                            state.queue.scroll = idx.saturating_sub(3);
                        }
                        playlist::save_queue_state(playlist_name, idx, state.library.tracks.len());
                        let name = playlist_name.clone();
                        fire_and_refresh(backend, tx, move |b| {
                            b.play_track_in_playlist(&name, idx)
                        });
                        state.filter.active = false;
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('x') if !state.filter.active => {
                    let real_idx = if has_filter {
                        filtered.get(state.library.tracks_selected).copied()
                    } else if state.library.tracks_selected < state.library.tracks.len() {
                        Some(state.library.tracks_selected)
                    } else {
                        None
                    };
                    if let Some(idx) = real_idx {
                        let name = playlist_name.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || b.remove_from_playlist(&name, idx));
                        state.library.tracks.remove(idx);
                        let new_filtered = filter_track_indices(&state.library.tracks, &state.filter.query);
                        if state.library.tracks_selected >= new_filtered.len()
                            && !new_filtered.is_empty()
                        {
                            state.library.tracks_selected = new_filtered.len() - 1;
                        }
                    }
                }
                KeyCode::Esc if has_filter => {
                    clear_filter(state);
                }
                _ => {}
            }
        }
    }
}

fn handle_search_key(
    key: KeyEvent,
    state: &mut AppState,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    if state.search.editing {
        match key.code {
            KeyCode::Esc => {
                state.search.editing = false;
            }
            KeyCode::Enter => {
                state.search.editing = false;
            }
            KeyCode::Backspace => {
                if !state.search.query.is_empty() {
                    state.search.query.pop();
                    perform_search(state, tx, backend);
                } else {
                    state.search.results.clear();
                    state.search.selected = 0;
                    state.search.scroll = 0;
                }
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    state.search.query.push(ch);
                    state.search.selected = 0;
                    state.search.scroll = 0;
                    perform_search(state, tx, backend);
                }
            }
            _ => {}
        }
    } else {
        if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.search.selected, state.search.scroll, state.search.results.len()) {
            state.search.selected = sel;
            state.search.scroll = scr;
            return;
        }
        match key.code {
            KeyCode::Char('/') | KeyCode::Char('i') => {
                state.search.editing = true;
            }
            KeyCode::Enter => {
                if !state.search.results.is_empty()
                    && state.search.selected < state.search.results.len()
                {
                    let result = state.search.results[state.search.selected].clone();
                    fire_and_refresh(backend, tx, move |b| b.play_track(&result.name, &result.artist));
                }
            }
            KeyCode::Backspace => {
                state.search.query.clear();
                state.search.results.clear();
                state.search.selected = 0;
                state.search.scroll = 0;
                state.search.editing = true;
            }
            _ => {}
        }
    }
}

fn handle_lyrics_key(key: KeyEvent, state: &mut AppState) {
    match normalize_nav_key(&key, true) {
        KeyCode::Up => {
            if state.lyrics.scroll > 0 {
                state.lyrics.scroll -= 1;
                state.lyrics.manual_scroll = true;
            }
        }
        KeyCode::Down => {
            state.lyrics.scroll += 1;
            state.lyrics.manual_scroll = true;
        }
        KeyCode::Home => {
            state.lyrics.scroll = 0;
            state.lyrics.manual_scroll = true;
        }
        KeyCode::End => {
            state.lyrics.scroll = usize::MAX / 2;
            state.lyrics.manual_scroll = true;
        }
        KeyCode::PageUp => {
            state.lyrics.scroll = state.lyrics.scroll.saturating_sub(PAGE_SIZE);
            state.lyrics.manual_scroll = true;
        }
        KeyCode::PageDown => {
            state.lyrics.scroll += PAGE_SIZE;
            state.lyrics.manual_scroll = true;
        }
        KeyCode::Char('0') => {
            state.lyrics.manual_scroll = false;
        }
        _ => {}
    }
}

fn handle_theme_picker_key(key: KeyEvent, state: &mut AppState, theme: &mut Theme) {
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.theme.selected, state.theme.scroll, state.theme.all.len()) {
        state.theme.selected = sel;
        state.theme.scroll = scr;
        preview_theme(state, theme);
        return;
    }
    match key.code {
        KeyCode::Enter => {
            if state.theme.selected < state.theme.all.len() {
                let (ref name, t) = state.theme.all[state.theme.selected];
                state.theme.name = name.clone();
                *theme = t;
                state.theme.picker_visible = false;
            }
        }
        KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('q') => {
            restore_saved_theme(state, theme);
            state.theme.picker_visible = false;
        }
        _ => {}
    }
}

fn handle_playlist_picker_key(
    key: KeyEvent,
    state: &mut AppState,
    _tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.overlays.playlist_picker_selected, state.overlays.playlist_picker_scroll, state.library.playlists.len()) {
        state.overlays.playlist_picker_selected = sel;
        state.overlays.playlist_picker_scroll = scr;
        return;
    }
    match key.code {
        KeyCode::Enter => {
            if !state.library.playlists.is_empty()
                && state.overlays.playlist_picker_selected < state.library.playlists.len()
            {
                let name = state.library.playlists[state.overlays.playlist_picker_selected].clone();
                state.overlays.playlist_picker_visible = false;
                let b = backend.clone();
                std::thread::spawn(move || b.add_to_playlist(&name));
            }
        }
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('P') => {
            state.overlays.playlist_picker_visible = false;
        }
        _ => {}
    }
}

// MARK: - Theme helpers

fn preview_theme(state: &AppState, theme: &mut Theme) {
    if state.theme.selected < state.theme.all.len() {
        *theme = state.theme.all[state.theme.selected].1;
    }
}

fn restore_saved_theme(state: &AppState, theme: &mut Theme) {
    if let Some((_, t)) = theme::find_theme(&state.theme.name, &state.theme.all) {
        *theme = t;
    }
}

// MARK: - Background-thread helpers

fn perform_search(
    state: &AppState,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    let query = state.search.query.clone();
    if query.len() < 2 {
        return;
    }
    let tx2 = tx.clone();
    let b = backend.clone();
    std::thread::spawn(move || {
        let results = b.search(&query);
        let _ = tx2.send(AppEvent::SearchResults(query, results));
    });
}

pub fn fetch_artwork(picker: &Picker, backend: &dyn MusicBackend) -> Option<StatefulProtocol> {
    for _ in 0..3 {
        if let Some(data) = backend.get_artwork_data() {
            if let Ok(reader) = ImageReader::new(std::io::Cursor::new(data)).with_guessed_format() {
                if let Ok(img) = reader.decode() {
                    return Some(picker.new_resize_protocol(img));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    None
}

pub fn fire_and_refresh<F: FnOnce(&dyn MusicBackend) + Send + 'static>(
    backend: &Arc<dyn MusicBackend>,
    tx: &mpsc::Sender<AppEvent>,
    action: F,
) {
    let tx2 = tx.clone();
    let b = backend.clone();
    std::thread::spawn(move || {
        action(&*b);
        for delay_ms in [500u64, 800, 800] {
            std::thread::sleep(Duration::from_millis(delay_ms));
            let fresh = b.fetch_state();
            if tx2.send(AppEvent::StateRefreshed(fresh)).is_err() {
                break;
            }
        }
    });
}
