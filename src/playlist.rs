//! Apple Music playlist management — queue state persistence, playlist-aware
//! track navigation, and auto-advance logic.
//!
//! This module is specific to Apple Music's AppleScript-based playlist model.
//! Spotify (and other future backends) handle playlists and queuing natively
//! via their own APIs, so this module will not be needed for those backends.

use std::env;
use std::fs;
use std::path::PathBuf;

use crate::bridge;

// --- Queue state persistence ---

fn queue_state_path() -> PathBuf {
    let mut path = PathBuf::from(env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    path.push(".local/share/muse");
    path
}

/// Persist current queue position so CLI commands can use playlist-aware next/prev.
pub fn save_queue_state(playlist_name: &str, selected: usize, total: usize) {
    let dir = queue_state_path();
    let _ = fs::create_dir_all(&dir);
    let content = format!("{}\n{}\n{}", playlist_name, selected, total);
    let _ = fs::write(dir.join("queue_state"), content);
}

/// Clear persisted queue state.
pub fn clear_queue_state() {
    let _ = fs::remove_file(queue_state_path().join("queue_state"));
}

/// Load persisted queue state: (playlist_name, selected_index, total_tracks).
pub fn load_queue_state() -> Option<(String, usize, usize)> {
    let content = fs::read_to_string(queue_state_path().join("queue_state")).ok()?;
    let mut lines = content.lines();
    let name = lines.next()?.to_string();
    let selected: usize = lines.next()?.parse().ok()?;
    let total: usize = lines.next()?.parse().ok()?;
    if name.is_empty() {
        return None;
    }
    Some((name, selected, total))
}

// --- CLI playlist-aware navigation ---

/// Handle `muse next` — advance within the current playlist if queue state exists,
/// otherwise fall back to Music.app's global next.
pub fn cli_next() {
    if let Some((playlist, selected, total)) = load_queue_state() {
        if selected + 1 < total {
            let next_idx = selected + 1;
            bridge::play_track_in_playlist(&playlist, next_idx as i32);
            save_queue_state(&playlist, next_idx, total);
        }
    } else {
        bridge::next_track();
    }
}

/// Handle `muse prev` — go back within the current playlist if queue state exists,
/// otherwise fall back to Music.app's global previous.
pub fn cli_prev() {
    if let Some((playlist, selected, total)) = load_queue_state() {
        if selected > 0 {
            let prev_idx = selected - 1;
            bridge::play_track_in_playlist(&playlist, prev_idx as i32);
            save_queue_state(&playlist, prev_idx, total);
        }
    } else {
        bridge::previous_track();
    }
}

// --- Queue selection sync ---

/// When a new track notification arrives, find the matching track in the queue
/// and update the selection. Returns the new index if a match was found.
pub fn sync_queue_selection(
    queue_tracks: &[bridge::PlaylistTrack],
    queue_playlist_name: &str,
    track_name: &str,
    track_artist: &str,
) -> Option<usize> {
    let pos = queue_tracks
        .iter()
        .position(|t| t.name == track_name && t.artist == track_artist)?;
    save_queue_state(queue_playlist_name, pos, queue_tracks.len());
    Some(pos)
}
