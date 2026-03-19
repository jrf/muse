//! Backend trait and shared types for music service integration.
//!
//! All music backends (Apple Music, Spotify, etc.) implement the `MusicBackend`
//! trait, which provides a uniform interface for playback control, library
//! browsing, search, and notifications.

use std::sync::mpsc;

// MARK: - Shared types

#[derive(Debug, Clone)]
pub struct Track {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub position: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn label(&self) -> &'static str {
        match self {
            RepeatMode::Off => "off",
            RepeatMode::All => "all",
            RepeatMode::One => "one",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FullState {
    pub music_running: bool,
    pub player_state: PlayerState,
    pub volume: i32,
    pub shuffle_enabled: bool,
    pub repeat_mode: RepeatMode,
    pub track: Option<Track>,
    pub track_favorited: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PlaylistTrack {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SearchResult {
    pub name: String,
    pub artist: String,
    pub album: String,
}

#[derive(Debug, Clone)]
pub struct LyricsLine {
    pub text: String,
    pub time: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct LyricsResult {
    pub lines: Vec<LyricsLine>,
    pub synced: bool,
}

#[derive(Debug, Clone)]
pub struct NotificationInfo {
    pub player_state: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub total_time_ms: f64,
}

// MARK: - Backend trait

/// Trait for music service backends (Apple Music, Spotify, etc.)
///
/// All methods are synchronous. Backends that use async APIs (e.g. HTTP)
/// should block internally. The main loop calls backend methods from
/// background threads where appropriate.
pub trait MusicBackend: Send + Sync {
    /// Human-readable name of this backend (e.g. "Apple Music", "Spotify").
    #[allow(dead_code)]
    fn name(&self) -> &str;

    /// Ensure the music service is running/connected.
    fn ensure_running(&self);

    /// Fetch full player state (track, playback status, volume, etc.).
    fn fetch_state(&self) -> FullState;

    // -- Playback controls --

    fn play_pause(&self);
    fn next_track(&self);
    fn previous_track(&self);
    fn set_volume(&self, vol: i32);
    fn toggle_shuffle(&self);
    fn cycle_repeat(&self);
    fn toggle_favorite(&self);

    // -- Playlists --

    fn get_playlists(&self) -> Vec<String>;
    fn get_playlist_tracks(&self, name: &str) -> Vec<PlaylistTrack>;
    fn play_track_in_playlist(&self, playlist: &str, index: usize);

    // -- Search --

    fn search(&self, query: &str) -> Vec<SearchResult>;
    fn play_track(&self, name: &str, artist: &str);

    // -- Lyrics --

    fn get_lyrics(&self, track_name: &str, artist: &str) -> Option<LyricsResult>;

    // -- Artwork --

    /// Returns raw image data (JPEG/PNG) for the current track's artwork.
    fn get_artwork_data(&self) -> Option<Vec<u8>>;

    // -- External links --

    /// Open the artist in the music service's native app/website.
    fn reveal_artist(&self, artist: &str);

    /// Open the album in the music service's native app/website.
    fn reveal_album(&self, album: &str, artist: &str);

    /// Add the current track to a named playlist.
    fn add_to_playlist(&self, playlist_name: &str);

    /// Remove a track from a playlist by index.
    fn remove_from_playlist(&self, playlist_name: &str, index: usize);

    // -- Notifications --

    /// Set up notification delivery. The backend should send `NotificationInfo`
    /// through the provided channel whenever playback state changes.
    ///
    /// For Apple Music: registers a C callback via NSDistributedNotificationCenter.
    /// For Spotify: spawns a polling thread that checks the API periodically.
    fn setup_notifications(&self, tx: mpsc::Sender<NotificationInfo>);

    /// Called periodically from the main thread (~100ms).
    ///
    /// Apple Music uses this to pump the RunLoop for notification delivery.
    /// Other backends can use this for periodic main-thread work or no-op.
    fn tick(&self);

    /// Whether the frontend should auto-advance our queue when a track ends.
    /// Returns false for backends like Spotify that manage their own queue.
    fn needs_queue_advance(&self) -> bool;
}
