//! Application state — mirrors the Swift AppState but in idiomatic Rust.

use ratatui_image::protocol::StatefulProtocol;

use crate::backend;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Queue,
    Library,
    Search,
    Lyrics,
}

impl Tab {
    pub const ALL: &[Tab] = &[Tab::Queue, Tab::Library, Tab::Search, Tab::Lyrics];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Queue => "Queue",
            Tab::Library => "Library",
            Tab::Search => "Search",
            Tab::Lyrics => "Lyrics",
        }
    }

    pub fn next(&self) -> Tab {
        match self {
            Tab::Queue => Tab::Library,
            Tab::Library => Tab::Search,
            Tab::Search => Tab::Lyrics,
            Tab::Lyrics => Tab::Queue,
        }
    }

    pub fn prev(&self) -> Tab {
        match self {
            Tab::Queue => Tab::Lyrics,
            Tab::Library => Tab::Queue,
            Tab::Search => Tab::Library,
            Tab::Lyrics => Tab::Search,
        }
    }

    pub fn from_name(s: &str) -> Option<Tab> {
        match s {
            "queue" => Some(Tab::Queue),
            "library" => Some(Tab::Library),
            "search" => Some(Tab::Search),
            "lyrics" => Some(Tab::Lyrics),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum LibrarySubView {
    Playlists,
    Tracks(String),
}

pub struct AppState {
    // Config
    pub ui_width: u16,
    pub show_artwork: bool,

    // Player
    pub track: Option<backend::Track>,
    pub artwork: Option<StatefulProtocol>,
    pub artwork_key: String,
    pub player_state: backend::PlayerState,
    pub volume: i32,
    pub shuffle_enabled: bool,
    pub repeat_mode: backend::RepeatMode,
    pub music_running: bool,

    // Tabs
    pub active_tab: Tab,

    // Queue
    pub queue_tracks: Vec<backend::PlaylistTrack>,
    pub queue_selected: usize,
    pub queue_scroll: usize,
    pub queue_playlist_name: String,

    // Library
    pub playlists: Vec<String>,
    pub library_sub_view: LibrarySubView,
    pub library_selected: usize,
    pub library_scroll: usize,
    pub playlist_tracks: Vec<backend::PlaylistTrack>,
    pub playlist_tracks_selected: usize,
    pub playlist_tracks_scroll: usize,

    // Search
    pub search_query: String,
    pub search_results: Vec<backend::SearchResult>,
    pub search_selected: usize,
    pub search_scroll: usize,

    // Lyrics
    pub lyrics_lines: Vec<backend::LyricsLine>,
    pub lyrics_synced: bool,
    pub lyrics_scroll: usize,
    pub lyrics_manual_scroll: bool,
    pub lyrics_track_key: String,

    // Themes
    pub themes: Vec<(String, Theme)>,
    pub theme_name: String,
    pub theme_selected: usize,
    pub theme_scroll: usize,
    pub show_theme_picker: bool,

    // Help overlay
    pub show_help: bool,

    // Favorite
    pub current_track_favorited: bool,

    // Playlist picker overlay
    pub show_playlist_picker: bool,
    pub playlist_picker_selected: usize,
    pub playlist_picker_scroll: usize,

    // Last.fm
    pub lastfm_status: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            ui_width: 120,
            show_artwork: true,
            track: None,
            artwork: None,
            artwork_key: String::new(),
            player_state: backend::PlayerState::Stopped,
            volume: 50,
            shuffle_enabled: false,
            repeat_mode: backend::RepeatMode::Off,
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
            themes: Vec::new(),
            theme_name: "synthwave".to_string(),
            theme_selected: 0,
            theme_scroll: 0,
            show_theme_picker: false,
            show_help: false,
            current_track_favorited: false,
            show_playlist_picker: false,
            playlist_picker_selected: 0,
            playlist_picker_scroll: 0,
            lastfm_status: String::new(),
        }
    }
}
