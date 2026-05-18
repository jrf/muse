//! Application state.
//!
//! Decomposed into per-feature sub-structs so handlers can be reasoned about
//! one concern at a time and so the surface area of any individual mutation
//! is small. AppState owns one instance of each.

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

// MARK: - Sub-state structs

pub struct PlayerData {
    pub track: Option<backend::Track>,
    /// Identifier for the artwork currently held in run_app's local
    /// `current_artwork`. Kept in state so handlers can detect when the
    /// track changes and decide whether to refetch.
    pub artwork_key: String,
    pub playback: backend::PlayerState,
    pub volume: i32,
    pub shuffle_enabled: bool,
    pub repeat_mode: backend::RepeatMode,
    pub music_running: bool,
    pub current_track_favorited: bool,
}

impl Default for PlayerData {
    fn default() -> Self {
        Self {
            track: None,
            artwork_key: String::new(),
            playback: backend::PlayerState::Stopped,
            volume: 50,
            shuffle_enabled: false,
            repeat_mode: backend::RepeatMode::Off,
            music_running: true,
            current_track_favorited: false,
        }
    }
}

#[derive(Default)]
pub struct QueueData {
    pub tracks: Vec<backend::PlaylistTrack>,
    pub selected: usize,
    pub scroll: usize,
    /// Index of the currently-playing track (distinct from the user's cursor).
    pub playing: Option<usize>,
    pub playlist_name: String,
}

pub struct LibraryData {
    pub playlists: Vec<String>,
    pub sub_view: LibrarySubView,
    pub selected: usize,
    pub scroll: usize,
    pub tracks: Vec<backend::PlaylistTrack>,
    pub tracks_selected: usize,
    pub tracks_scroll: usize,
}

impl Default for LibraryData {
    fn default() -> Self {
        Self {
            playlists: Vec::new(),
            sub_view: LibrarySubView::Playlists,
            selected: 0,
            scroll: 0,
            tracks: Vec::new(),
            tracks_selected: 0,
            tracks_scroll: 0,
        }
    }
}

pub struct SearchData {
    pub query: String,
    pub results: Vec<backend::SearchResult>,
    pub selected: usize,
    pub scroll: usize,
    pub editing: bool,
}

impl Default for SearchData {
    fn default() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            scroll: 0,
            editing: true,
        }
    }
}

#[derive(Default)]
pub struct LyricsData {
    pub lines: Vec<backend::LyricsLine>,
    pub synced: bool,
    pub scroll: usize,
    pub manual_scroll: bool,
    pub track_key: String,
}

pub struct ThemeData {
    pub all: Vec<(String, Theme)>,
    pub name: String,
    pub selected: usize,
    pub scroll: usize,
    pub picker_visible: bool,
}

impl Default for ThemeData {
    fn default() -> Self {
        Self {
            all: Vec::new(),
            name: "synthwave".to_string(),
            selected: 0,
            scroll: 0,
            picker_visible: false,
        }
    }
}

#[derive(Default)]
pub struct OverlayData {
    pub show_help: bool,
    pub playlist_picker_visible: bool,
    pub playlist_picker_selected: usize,
    pub playlist_picker_scroll: usize,
    /// Transient error overlay; dismissed on any key.
    pub error_message: Option<String>,
}

#[derive(Default)]
pub struct FilterData {
    /// Substring filter (shared across Queue and Library tabs).
    pub query: String,
    /// True while the user is actively typing into the filter input.
    pub active: bool,
}

// MARK: - AppState

pub struct AppState {
    // Config (from ~/.config/muse/config.toml)
    pub ui_width: u16,
    pub show_artwork: bool,
    pub lyrics_enabled: bool,

    // Active UI
    pub active_tab: Tab,

    // Per-concern sub-states
    pub player: PlayerData,
    pub queue: QueueData,
    pub library: LibraryData,
    pub search: SearchData,
    pub lyrics: LyricsData,
    pub theme: ThemeData,
    pub overlays: OverlayData,
    pub filter: FilterData,

    // Misc
    pub lastfm_status: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            ui_width: 120,
            show_artwork: true,
            lyrics_enabled: true,
            active_tab: Tab::Queue,
            player: PlayerData::default(),
            queue: QueueData::default(),
            library: LibraryData::default(),
            search: SearchData::default(),
            lyrics: LyricsData::default(),
            theme: ThemeData::default(),
            overlays: OverlayData::default(),
            filter: FilterData::default(),
            lastfm_status: String::new(),
        }
    }
}
