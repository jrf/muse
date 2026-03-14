//! Application state — mirrors the Swift AppState but in idiomatic Rust.

use ratatui_image::protocol::StatefulProtocol;

use crate::bridge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Queue,
    Library,
    Search,
    Lyrics,
    Themes,
}

impl Tab {
    pub const ALL: &[Tab] = &[Tab::Queue, Tab::Library, Tab::Search, Tab::Lyrics, Tab::Themes];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Queue => "Queue",
            Tab::Library => "Library",
            Tab::Search => "Search",
            Tab::Lyrics => "Lyrics",
            Tab::Themes => "Themes",
        }
    }

    pub fn next(&self) -> Tab {
        match self {
            Tab::Queue => Tab::Library,
            Tab::Library => Tab::Search,
            Tab::Search => Tab::Lyrics,
            Tab::Lyrics => Tab::Themes,
            Tab::Themes => Tab::Queue,
        }
    }

    pub fn prev(&self) -> Tab {
        match self {
            Tab::Queue => Tab::Themes,
            Tab::Library => Tab::Queue,
            Tab::Search => Tab::Library,
            Tab::Lyrics => Tab::Search,
            Tab::Themes => Tab::Lyrics,
        }
    }
}

#[derive(Debug, Clone)]
pub enum LibrarySubView {
    Playlists,
    Tracks(String),
}

pub struct AppState {
    // Player
    pub track: Option<bridge::Track>,
    pub artwork: Option<StatefulProtocol>,
    pub artwork_key: String,
    pub player_state: bridge::PlayerState,
    pub volume: i32,
    pub shuffle_enabled: bool,
    pub repeat_mode: bridge::RepeatMode,
    pub music_running: bool,

    // Tabs
    pub active_tab: Tab,

    // Queue
    pub queue_tracks: Vec<bridge::PlaylistTrack>,
    pub queue_selected: usize,
    pub queue_scroll: usize,
    pub queue_playlist_name: String,

    // Library
    pub playlists: Vec<String>,
    pub library_sub_view: LibrarySubView,
    pub library_selected: usize,
    pub library_scroll: usize,
    pub playlist_tracks: Vec<bridge::PlaylistTrack>,
    pub playlist_tracks_selected: usize,
    pub playlist_tracks_scroll: usize,

    // Search
    pub search_query: String,
    pub search_results: Vec<bridge::SearchResult>,
    pub search_selected: usize,
    pub search_scroll: usize,

    // Lyrics
    pub lyrics_lines: Vec<bridge::LyricsLine>,
    pub lyrics_synced: bool,
    pub lyrics_scroll: usize,
    pub lyrics_manual_scroll: bool,
    pub lyrics_track_key: String,

    // Themes
    pub theme_name: String,
    pub theme_selected: usize,
    pub theme_scroll: usize,

    // Help overlay
    pub show_help: bool,

    // Favorite
    pub current_track_favorited: bool,

    // Playlist picker overlay
    pub show_playlist_picker: bool,
    pub playlist_picker_selected: usize,
    pub playlist_picker_scroll: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            track: None,
            artwork: None,
            artwork_key: String::new(),
            player_state: bridge::PlayerState::Stopped,
            volume: 50,
            shuffle_enabled: false,
            repeat_mode: bridge::RepeatMode::Off,
            music_running: true,
            active_tab: Tab::Queue,
            queue_tracks: Vec::new(),
            queue_selected: 0,
            queue_scroll: 0,
            queue_playlist_name: String::new(),
            playlists: Vec::new(),
            library_sub_view: LibrarySubView::Playlists,
            library_selected: 0,
            library_scroll: 0,
            playlist_tracks: Vec::new(),
            playlist_tracks_selected: 0,
            playlist_tracks_scroll: 0,
            search_query: String::new(),
            search_results: Vec::new(),
            search_selected: 0,
            search_scroll: 0,
            lyrics_lines: Vec::new(),
            lyrics_synced: false,
            lyrics_scroll: 0,
            lyrics_manual_scroll: false,
            lyrics_track_key: String::new(),
            theme_name: "synthwave".to_string(),
            theme_selected: 0,
            theme_scroll: 0,
            show_help: false,
            current_track_favorited: false,
            show_playlist_picker: false,
            playlist_picker_selected: 0,
            playlist_picker_scroll: 0,
        }
    }
}
