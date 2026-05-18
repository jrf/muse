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
    state.filter_query.clear();
    state.filter_active = false;
}

// MARK: - State application

pub fn apply_fresh_state(
    state: &mut AppState,
    fresh: &backend::FullState,
    picker: &Option<Arc<Picker>>,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    let old_art_key = state.artwork_key.clone();

    // Don't flip to "not running" if we were previously running — could be a
    // transient error during track transitions.  Only mark not running if we
    // also had no track before (i.e. we never connected).
    if fresh.music_running || !state.music_running {
        state.music_running = fresh.music_running;
    }

    if fresh.music_running {
        state.volume = fresh.volume;
        state.shuffle_enabled = fresh.shuffle_enabled;
        state.repeat_mode = fresh.repeat_mode;
        state.current_track_favorited = fresh.track_favorited;
        // Only update track/player_state with concrete data.
        // During transitions, keep showing the previous track.
        if let Some(ref track) = fresh.track {
            // If track just finished (same track, near end, no longer playing),
            // snap position to duration so the progress bar shows completion.
            let was_playing = state.player_state == backend::PlayerState::Playing;
            let is_no_longer_playing = fresh.player_state != backend::PlayerState::Playing;
            let is_same_track = state
                .track
                .as_ref()
                .map_or(false, |t| t.name == track.name && t.artist == track.artist);
            let near_end = track.duration > 0.0 && (track.duration - track.position) < 5.0;

            let mut updated_track = track.clone();
            if was_playing && is_no_longer_playing && is_same_track && near_end {
                updated_track.position = updated_track.duration;
            }

            state.track = Some(updated_track);
            state.player_state = fresh.player_state;
        } else if state.track.is_none() {
            // No previous track either — show whatever state we got
            state.player_state = fresh.player_state;
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
        state.artwork_key = new_art_key.clone();
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
        state.artwork_key.clear();
        state.artwork = None;
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
        "Playing" => state.player_state = backend::PlayerState::Playing,
        "Paused" | "Stopped" => {
            let was_playing = state.player_state == backend::PlayerState::Playing;
            let is_same_track = !info.name.is_empty()
                && state
                    .track
                    .as_ref()
                    .map_or(false, |t| t.name == info.name && t.artist == info.artist);
            let near_end = state
                .track
                .as_ref()
                .map_or(false, |t| t.duration > 0.0 && (t.duration - t.position) < 5.0);

            // Snap position to duration so progress bar shows completion.
            if is_same_track && near_end {
                if let Some(ref mut t) = state.track {
                    t.position = t.duration;
                }
            }

            if info.player_state == "Stopped" && !info.name.is_empty() {
                // Don't immediately mark stopped during transitions
            } else if info.player_state == "Stopped" {
                state.player_state = backend::PlayerState::Stopped;
            } else {
                state.player_state = backend::PlayerState::Paused;
            }

            // Auto-advance for Apple Music when a track finishes naturally.
            // Guard: was_playing prevents re-entry (state is already
            // updated above so a second notification won't fire again).
            // Spotify manages its own queue natively — no intervention needed.
            if was_playing && is_same_track && near_end && backend.needs_queue_advance() {
                let playing_idx = state.queue_playing.unwrap_or(state.queue_selected);
                if !state.queue_tracks.is_empty()
                    && playing_idx + 1 < state.queue_tracks.len()
                {
                    let next_idx = playing_idx + 1;
                    state.queue_playing = Some(next_idx);
                    playlist::save_queue_state(&state.queue_playlist_name, next_idx, state.queue_tracks.len());
                    if state.queue_selected == playing_idx {
                        state.queue_selected = next_idx;
                    }
                    if next_idx >= state.queue_scroll + PAGE_SIZE {
                        state.queue_scroll = next_idx.saturating_sub(3);
                    }
                    let playlist = state.queue_playlist_name.clone();
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
            .track
            .as_ref()
            .map_or(true, |t| t.name != info.name || t.artist != info.artist);

        state.track = Some(backend::Track {
            name: info.name.clone(),
            artist: info.artist.clone(),
            album: info.album.clone(),
            duration: info.total_time_ms / 1000.0,
            position: if is_new {
                0.0
            } else {
                state.track.as_ref().map_or(0.0, |t| t.position)
            },
        });

        // Sync queue_playing if the new track matches a queue entry
        // (handles CLI next/prev while TUI is running).
        if is_new && !state.queue_tracks.is_empty() {
            if let Some(pos) = playlist::sync_queue_selection(
                &state.queue_tracks,
                &state.queue_playlist_name,
                &info.name,
                &info.artist,
            ) {
                state.queue_playing = Some(pos);
            }
        }

        if is_new {
            let new_key = format!("{}\t{}", info.artist, info.album);
            if new_key != state.artwork_key {
                state.artwork_key = new_key.clone();
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

    state.music_running = true;
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
    if state.error_message.is_some() {
        state.error_message = None;
        return false;
    }

    // Help overlay
    if key.code == KeyCode::Char('?') {
        state.show_help = !state.show_help;
        return false;
    }
    if state.show_help {
        state.show_help = false;
        return false;
    }

    // Theme picker overlay
    if state.show_theme_picker {
        handle_theme_picker_key(key, state, theme);
        return false;
    }

    // Playlist picker
    if state.show_playlist_picker {
        handle_playlist_picker_key(key, state, tx, backend);
        return false;
    }

    let in_search = state.active_tab == Tab::Search && state.search_editing;
    let in_filter = state.filter_active;
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
            state.show_theme_picker = !state.show_theme_picker;
            if state.show_theme_picker {
                if let Some((idx, _)) = theme::find_theme(&state.theme_name, &state.themes) {
                    state.theme_selected = idx;
                } else {
                    state.theme_selected = 0;
                }
                state.theme_scroll = 0;
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
            state.search_editing = true;
            return false;
        }
        KeyCode::Char(' ') if !text_input => {
            state.player_state = if state.player_state == backend::PlayerState::Playing {
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
            state.volume = (state.volume + 5).min(100);
            let vol = state.volume;
            let b = backend.clone();
            std::thread::spawn(move || b.set_volume(vol));
            return false;
        }
        KeyCode::Char('-') if !text_input => {
            state.volume = (state.volume - 5).max(0);
            let vol = state.volume;
            let b = backend.clone();
            std::thread::spawn(move || b.set_volume(vol));
            return false;
        }
        KeyCode::Char('s') if !text_input => {
            state.shuffle_enabled = !state.shuffle_enabled;
            fire_and_refresh(backend, tx, |b| b.toggle_shuffle());
            return false;
        }
        KeyCode::Char('r') if !text_input => {
            state.repeat_mode = match state.repeat_mode {
                backend::RepeatMode::Off => backend::RepeatMode::All,
                backend::RepeatMode::All => backend::RepeatMode::One,
                backend::RepeatMode::One => backend::RepeatMode::Off,
            };
            fire_and_refresh(backend, tx, |b| b.cycle_repeat());
            return false;
        }
        KeyCode::Char('C') if !text_input => {
            state.queue_tracks.clear();
            state.queue_selected = 0;
            state.queue_scroll = 0;
            state.queue_playing = None;
            state.queue_playlist_name.clear();
            playlist::clear_queue_state();
            return false;
        }
        KeyCode::Char('f') if !text_input && !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.current_track_favorited = !state.current_track_favorited;
            fire_and_refresh(backend, tx, |b| b.toggle_favorite());
            return false;
        }
        KeyCode::Char('P') if !text_input => {
            state.show_playlist_picker = !state.show_playlist_picker;
            state.playlist_picker_selected = 0;
            state.playlist_picker_scroll = 0;
            return false;
        }
        KeyCode::Char('a') if !text_input => {
            if let Some(artist) = state.track.as_ref().map(|t| t.artist.clone()) {
                if !artist.is_empty() {
                    state.active_tab = Tab::Search;
                    state.search_editing = false;
                    state.search_query = artist;
                    state.search_selected = 0;
                    state.search_scroll = 0;
                    perform_search(state, tx, backend);
                }
            }
            return false;
        }
        KeyCode::Char('A') => {
            if let Some(album) = state.track.as_ref().map(|t| t.album.clone()) {
                if !album.is_empty() {
                    state.active_tab = Tab::Search;
                    state.search_editing = false;
                    state.search_query = album;
                    state.search_selected = 0;
                    state.search_scroll = 0;
                    perform_search(state, tx, backend);
                }
            }
            return false;
        }
        KeyCode::Char('o') if !text_input => {
            if let Some(artist) = state.track.as_ref().map(|t| t.artist.clone()) {
                if !artist.is_empty() {
                    let b = backend.clone();
                    std::thread::spawn(move || b.reveal_artist(&artist));
                }
            }
            return false;
        }
        KeyCode::Char('O') if !text_input => {
            if let Some(track) = state.track.clone() {
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
    if state.filter_active {
        match key.code {
            KeyCode::Esc => {
                clear_filter(state);
                return;
            }
            KeyCode::Backspace => {
                state.filter_query.pop();
                if state.filter_query.is_empty() {
                    state.filter_active = false;
                }
                state.queue_selected = 0;
                state.queue_scroll = 0;
                return;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.filter_query.push(ch);
                state.queue_selected = 0;
                state.queue_scroll = 0;
                return;
            }
            _ => {}
        }
    }

    let filtered = filter_track_indices(&state.queue_tracks, &state.filter_query);
    let has_filter = !state.filter_query.is_empty();

    let nav_len = if has_filter { filtered.len() } else { state.queue_tracks.len() };
    let vim_nav = !state.filter_active;
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, vim_nav), state.queue_selected, state.queue_scroll, nav_len) {
        state.queue_selected = sel;
        state.queue_scroll = scr;
        return;
    }

    match key.code {
        KeyCode::Char('/') => {
            state.filter_active = true;
            state.filter_query.clear();
            state.queue_selected = 0;
            state.queue_scroll = 0;
        }
        KeyCode::Enter => {
            let real_idx = if has_filter {
                filtered.get(state.queue_selected).copied()
            } else if state.queue_selected < state.queue_tracks.len() {
                Some(state.queue_selected)
            } else {
                None
            };
            if let Some(idx) = real_idx {
                state.queue_playing = Some(idx);
                playlist::save_queue_state(&state.queue_playlist_name, idx, state.queue_tracks.len());
                let playlist = state.queue_playlist_name.clone();
                fire_and_refresh(backend, tx, move |b| b.play_track_in_playlist(&playlist, idx));
                clear_filter(state);
            }
        }
        KeyCode::Char('d') | KeyCode::Char('x') if !state.filter_active => {
            let real_idx = if has_filter {
                filtered.get(state.queue_selected).copied()
            } else if state.queue_selected < state.queue_tracks.len() {
                Some(state.queue_selected)
            } else {
                None
            };
            if let Some(removed) = real_idx {
                state.queue_tracks.remove(removed);
                if state.queue_tracks.is_empty() {
                    state.queue_selected = 0;
                    state.queue_scroll = 0;
                    state.queue_playing = None;
                    state.queue_playlist_name.clear();
                    playlist::clear_queue_state();
                    clear_filter(state);
                } else {
                    let new_filtered = filter_track_indices(&state.queue_tracks, &state.filter_query);
                    if state.queue_selected >= new_filtered.len() && !new_filtered.is_empty() {
                        state.queue_selected = new_filtered.len() - 1;
                    }
                    if let Some(ref mut pi) = state.queue_playing {
                        if removed < *pi {
                            *pi -= 1;
                        } else if removed == *pi {
                            state.queue_playing = None;
                        }
                    }
                    let persist_idx = state.queue_playing.unwrap_or(0);
                    playlist::save_queue_state(&state.queue_playlist_name, persist_idx, state.queue_tracks.len());
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
    if state.filter_active {
        match key.code {
            KeyCode::Esc => {
                clear_filter(state);
                return;
            }
            KeyCode::Backspace => {
                state.filter_query.pop();
                if state.filter_query.is_empty() {
                    state.filter_active = false;
                }
                match &state.library_sub_view {
                    LibrarySubView::Playlists => {
                        state.library_selected = 0;
                        state.library_scroll = 0;
                    }
                    LibrarySubView::Tracks(_) => {
                        state.playlist_tracks_selected = 0;
                        state.playlist_tracks_scroll = 0;
                    }
                }
                return;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.filter_query.push(ch);
                match &state.library_sub_view {
                    LibrarySubView::Playlists => {
                        state.library_selected = 0;
                        state.library_scroll = 0;
                    }
                    LibrarySubView::Tracks(_) => {
                        state.playlist_tracks_selected = 0;
                        state.playlist_tracks_scroll = 0;
                    }
                }
                return;
            }
            _ => {}
        }
    }

    let has_filter = !state.filter_query.is_empty();

    match state.library_sub_view.clone() {
        LibrarySubView::Playlists => {
            let filtered = filter_string_indices(&state.playlists, &state.filter_query);
            let nav_len = if has_filter { filtered.len() } else { state.playlists.len() };
            let vim_nav = !state.filter_active;
            if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, vim_nav), state.library_selected, state.library_scroll, nav_len) {
                state.library_selected = sel;
                state.library_scroll = scr;
                return;
            }
            match key.code {
                KeyCode::Char('/') => {
                    state.filter_active = true;
                    state.filter_query.clear();
                    state.library_selected = 0;
                    state.library_scroll = 0;
                }
                KeyCode::Enter => {
                    let real_idx = if has_filter {
                        filtered.get(state.library_selected).copied()
                    } else if state.library_selected < state.playlists.len() {
                        Some(state.library_selected)
                    } else {
                        None
                    };
                    if let Some(idx) = real_idx {
                        let name = state.playlists[idx].clone();
                        state.library_sub_view = LibrarySubView::Tracks(name.clone());
                        state.playlist_tracks.clear();
                        state.playlist_tracks_selected = 0;
                        state.playlist_tracks_scroll = 0;
                        clear_filter(state);
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
            let filtered = filter_track_indices(&state.playlist_tracks, &state.filter_query);
            let nav_len = if has_filter { filtered.len() } else { state.playlist_tracks.len() };
            let vim_nav = !state.filter_active;
            if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, vim_nav), state.playlist_tracks_selected, state.playlist_tracks_scroll, nav_len) {
                state.playlist_tracks_selected = sel;
                state.playlist_tracks_scroll = scr;
                return;
            }
            match key.code {
                KeyCode::Char('/') => {
                    state.filter_active = true;
                    state.filter_query.clear();
                    state.playlist_tracks_selected = 0;
                    state.playlist_tracks_scroll = 0;
                }
                KeyCode::Backspace if !state.filter_active && !has_filter => {
                    clear_filter(state);
                    state.library_sub_view = LibrarySubView::Playlists;
                }
                KeyCode::Enter => {
                    let real_idx = if has_filter {
                        filtered.get(state.playlist_tracks_selected).copied()
                    } else if state.playlist_tracks_selected < state.playlist_tracks.len() {
                        Some(state.playlist_tracks_selected)
                    } else {
                        None
                    };
                    if let Some(idx) = real_idx {
                        state.queue_tracks = state.playlist_tracks.clone();
                        state.queue_playlist_name = playlist_name.clone();
                        state.queue_selected = idx;
                        state.queue_playing = Some(idx);
                        if idx < state.queue_scroll || idx >= state.queue_scroll + PAGE_SIZE {
                            state.queue_scroll = idx.saturating_sub(3);
                        }
                        playlist::save_queue_state(playlist_name, idx, state.playlist_tracks.len());
                        let name = playlist_name.clone();
                        fire_and_refresh(backend, tx, move |b| {
                            b.play_track_in_playlist(&name, idx)
                        });
                        clear_filter(state);
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('x') if !state.filter_active => {
                    let real_idx = if has_filter {
                        filtered.get(state.playlist_tracks_selected).copied()
                    } else if state.playlist_tracks_selected < state.playlist_tracks.len() {
                        Some(state.playlist_tracks_selected)
                    } else {
                        None
                    };
                    if let Some(idx) = real_idx {
                        let name = playlist_name.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || b.remove_from_playlist(&name, idx));
                        state.playlist_tracks.remove(idx);
                        let new_filtered = filter_track_indices(&state.playlist_tracks, &state.filter_query);
                        if state.playlist_tracks_selected >= new_filtered.len()
                            && !new_filtered.is_empty()
                        {
                            state.playlist_tracks_selected = new_filtered.len() - 1;
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
    if state.search_editing {
        match key.code {
            KeyCode::Esc => {
                state.search_editing = false;
            }
            KeyCode::Enter => {
                state.search_editing = false;
            }
            KeyCode::Backspace => {
                if !state.search_query.is_empty() {
                    state.search_query.pop();
                    perform_search(state, tx, backend);
                } else {
                    state.search_results.clear();
                    state.search_selected = 0;
                    state.search_scroll = 0;
                }
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    state.search_query.push(ch);
                    state.search_selected = 0;
                    state.search_scroll = 0;
                    perform_search(state, tx, backend);
                }
            }
            _ => {}
        }
    } else {
        if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.search_selected, state.search_scroll, state.search_results.len()) {
            state.search_selected = sel;
            state.search_scroll = scr;
            return;
        }
        match key.code {
            KeyCode::Char('/') | KeyCode::Char('i') => {
                state.search_editing = true;
            }
            KeyCode::Enter => {
                if !state.search_results.is_empty()
                    && state.search_selected < state.search_results.len()
                {
                    let result = state.search_results[state.search_selected].clone();
                    fire_and_refresh(backend, tx, move |b| b.play_track(&result.name, &result.artist));
                }
            }
            KeyCode::Backspace => {
                state.search_query.clear();
                state.search_results.clear();
                state.search_selected = 0;
                state.search_scroll = 0;
                state.search_editing = true;
            }
            _ => {}
        }
    }
}

fn handle_lyrics_key(key: KeyEvent, state: &mut AppState) {
    match normalize_nav_key(&key, true) {
        KeyCode::Up => {
            if state.lyrics_scroll > 0 {
                state.lyrics_scroll -= 1;
                state.lyrics_manual_scroll = true;
            }
        }
        KeyCode::Down => {
            state.lyrics_scroll += 1;
            state.lyrics_manual_scroll = true;
        }
        KeyCode::Home => {
            state.lyrics_scroll = 0;
            state.lyrics_manual_scroll = true;
        }
        KeyCode::End => {
            state.lyrics_scroll = usize::MAX / 2;
            state.lyrics_manual_scroll = true;
        }
        KeyCode::PageUp => {
            state.lyrics_scroll = state.lyrics_scroll.saturating_sub(PAGE_SIZE);
            state.lyrics_manual_scroll = true;
        }
        KeyCode::PageDown => {
            state.lyrics_scroll += PAGE_SIZE;
            state.lyrics_manual_scroll = true;
        }
        KeyCode::Char('0') => {
            state.lyrics_manual_scroll = false;
        }
        _ => {}
    }
}

fn handle_theme_picker_key(key: KeyEvent, state: &mut AppState, theme: &mut Theme) {
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.theme_selected, state.theme_scroll, state.themes.len()) {
        state.theme_selected = sel;
        state.theme_scroll = scr;
        preview_theme(state, theme);
        return;
    }
    match key.code {
        KeyCode::Enter => {
            if state.theme_selected < state.themes.len() {
                let (ref name, t) = state.themes[state.theme_selected];
                state.theme_name = name.clone();
                *theme = t;
                save_theme(&state.theme_name);
                state.show_theme_picker = false;
            }
        }
        KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('q') => {
            restore_saved_theme(state, theme);
            state.show_theme_picker = false;
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
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.playlist_picker_selected, state.playlist_picker_scroll, state.playlists.len()) {
        state.playlist_picker_selected = sel;
        state.playlist_picker_scroll = scr;
        return;
    }
    match key.code {
        KeyCode::Enter => {
            if !state.playlists.is_empty()
                && state.playlist_picker_selected < state.playlists.len()
            {
                let name = state.playlists[state.playlist_picker_selected].clone();
                state.show_playlist_picker = false;
                let b = backend.clone();
                std::thread::spawn(move || b.add_to_playlist(&name));
            }
        }
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('P') => {
            state.show_playlist_picker = false;
        }
        _ => {}
    }
}

// MARK: - Theme helpers

fn preview_theme(state: &AppState, theme: &mut Theme) {
    if state.theme_selected < state.themes.len() {
        *theme = state.themes[state.theme_selected].1;
    }
}

fn restore_saved_theme(state: &AppState, theme: &mut Theme) {
    if let Some((_, t)) = theme::find_theme(&state.theme_name, &state.themes) {
        *theme = t;
    }
}

// MARK: - Background-thread helpers

fn perform_search(
    state: &AppState,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    let query = state.search_query.clone();
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

// MARK: - Theme persistence (here because the theme picker handler triggers it)

fn save_theme(name: &str) {
    let dir = crate::config_dir();
    let _ = std::fs::create_dir_all(&dir);

    let path = crate::config_file();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml::Table = existing.parse().unwrap_or_default();
    doc.insert("theme".to_string(), toml::Value::String(name.to_string()));
    let _ = std::fs::write(path, toml::to_string_pretty(&doc).unwrap_or_default());
}
