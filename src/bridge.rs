//! FFI bindings to the Swift MusicBridge static library.
//!
//! All complex types use an opaque pointer pattern: Swift allocates a heap object,
//! returns it as `*mut c_void`, and we access fields through individual accessor
//! functions. This avoids the `@_cdecl` restriction on Swift struct parameters.

use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;

#[allow(dead_code)]
extern "C" {
    // Simple actions
    pub fn music_free_string(ptr: *mut c_char);
    pub fn music_is_running() -> bool;
    pub fn music_ensure_running();
    pub fn music_play_pause();
    pub fn music_next_track();
    pub fn music_previous_track();
    pub fn music_get_volume() -> i32;
    pub fn music_set_volume(vol: i32);
    pub fn music_toggle_shuffle();
    pub fn music_cycle_repeat();
    pub fn music_toggle_favorite();

    // Full state (opaque pointer)
    fn music_fetch_state() -> *mut c_void;
    fn music_state_free(ptr: *mut c_void);
    fn music_state_music_running(ptr: *const c_void) -> bool;
    fn music_state_player_state(ptr: *const c_void) -> i32;
    fn music_state_volume(ptr: *const c_void) -> i32;
    fn music_state_shuffle_enabled(ptr: *const c_void) -> bool;
    fn music_state_repeat_mode(ptr: *const c_void) -> i32;
    fn music_state_has_track(ptr: *const c_void) -> bool;
    fn music_state_track_name(ptr: *const c_void) -> *mut c_char;
    fn music_state_track_artist(ptr: *const c_void) -> *mut c_char;
    fn music_state_track_album(ptr: *const c_void) -> *mut c_char;
    fn music_state_track_duration(ptr: *const c_void) -> f64;
    fn music_state_track_position(ptr: *const c_void) -> f64;
    fn music_state_track_favorited(ptr: *const c_void) -> bool;

    // Playlists (opaque pointer)
    fn music_get_playlists() -> *mut c_void;
    fn music_string_array_count(ptr: *const c_void) -> i32;
    fn music_string_array_get(ptr: *const c_void, index: i32) -> *mut c_char;
    fn music_string_array_free(ptr: *mut c_void);

    fn music_play_playlist(name: *const c_char);

    // Playlist tracks (opaque pointer)
    fn music_get_playlist_tracks(name: *const c_char) -> *mut c_void;
    fn music_playlist_tracks_count(ptr: *const c_void) -> i32;
    fn music_playlist_tracks_name(ptr: *const c_void, index: i32) -> *mut c_char;
    fn music_playlist_tracks_artist(ptr: *const c_void, index: i32) -> *mut c_char;
    fn music_playlist_tracks_album(ptr: *const c_void, index: i32) -> *mut c_char;
    fn music_playlist_tracks_duration(ptr: *const c_void, index: i32) -> f64;
    fn music_playlist_tracks_free(ptr: *mut c_void);

    fn music_play_track_in_playlist(name: *const c_char, index: i32);

    // Search (opaque pointer)
    fn music_search(query: *const c_char) -> *mut c_void;
    fn music_search_count(ptr: *const c_void) -> i32;
    fn music_search_name(ptr: *const c_void, index: i32) -> *mut c_char;
    fn music_search_artist(ptr: *const c_void, index: i32) -> *mut c_char;
    fn music_search_album(ptr: *const c_void, index: i32) -> *mut c_char;
    fn music_search_free(ptr: *mut c_void);

    fn music_play_track(name: *const c_char, artist: *const c_char);

    // Lyrics (opaque pointer, nullable)
    fn music_get_lyrics(track_name: *const c_char, artist: *const c_char) -> *mut c_void;
    fn music_lyrics_synced(ptr: *const c_void) -> bool;
    fn music_lyrics_count(ptr: *const c_void) -> i32;
    fn music_lyrics_text(ptr: *const c_void, index: i32) -> *mut c_char;
    fn music_lyrics_has_time(ptr: *const c_void, index: i32) -> bool;
    fn music_lyrics_time(ptr: *const c_void, index: i32) -> f64;
    fn music_lyrics_free(ptr: *mut c_void);

    // Open in Music.app
    fn music_reveal_artist(artist: *const c_char);
    fn music_reveal_album(album: *const c_char, artist: *const c_char);
    fn music_add_to_playlist(name: *const c_char);

    // Notifications (callback with opaque pointer)
    fn music_register_notification_callback(cb: extern "C" fn(*mut c_void));
    fn music_notification_player_state(ptr: *const c_void) -> *mut c_char;
    fn music_notification_name(ptr: *const c_void) -> *mut c_char;
    fn music_notification_artist(ptr: *const c_void) -> *mut c_char;
    fn music_notification_album(ptr: *const c_void) -> *mut c_char;
    fn music_notification_total_time_ms(ptr: *const c_void) -> f64;
    fn music_notification_free(ptr: *mut c_void);

    pub fn music_pump_runloop();

    // Artwork
    fn music_get_artwork_data(out_len: *mut i32) -> *mut u8;
    fn music_free_artwork_data(ptr: *mut u8);
}

// Helper: convert a C string to Rust String, freeing the C string
unsafe fn take_c_string(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        let s = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { music_free_string(ptr) };
        s
    }
}

// MARK: - Safe wrappers

pub fn ensure_running() {
    unsafe { music_ensure_running() }
}

pub fn play_pause() {
    unsafe { music_play_pause() }
}

pub fn next_track() {
    unsafe { music_next_track() }
}

pub fn previous_track() {
    unsafe { music_previous_track() }
}

pub fn set_volume(vol: i32) {
    unsafe { music_set_volume(vol) }
}

pub fn toggle_shuffle() {
    unsafe { music_toggle_shuffle() }
}

pub fn cycle_repeat() {
    unsafe { music_cycle_repeat() }
}

pub fn toggle_favorite() {
    unsafe { music_toggle_favorite() }
}

pub fn pump_runloop() {
    unsafe { music_pump_runloop() }
}

// Types

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

pub fn fetch_state() -> FullState {
    unsafe {
        let ptr = music_fetch_state();

        let track = if music_state_has_track(ptr) {
            Some(Track {
                name: take_c_string(music_state_track_name(ptr)),
                artist: take_c_string(music_state_track_artist(ptr)),
                album: take_c_string(music_state_track_album(ptr)),
                duration: music_state_track_duration(ptr),
                position: music_state_track_position(ptr),
            })
        } else {
            None
        };

        let state = FullState {
            music_running: music_state_music_running(ptr),
            player_state: match music_state_player_state(ptr) {
                1 => PlayerState::Playing,
                2 => PlayerState::Paused,
                _ => PlayerState::Stopped,
            },
            volume: music_state_volume(ptr),
            shuffle_enabled: music_state_shuffle_enabled(ptr),
            repeat_mode: match music_state_repeat_mode(ptr) {
                1 => RepeatMode::All,
                2 => RepeatMode::One,
                _ => RepeatMode::Off,
            },
            track,
            track_favorited: music_state_track_favorited(ptr),
        };

        music_state_free(ptr);
        state
    }
}

// Playlists

pub fn get_playlists() -> Vec<String> {
    unsafe {
        let ptr = music_get_playlists();
        let count = music_string_array_count(ptr);
        let mut result = Vec::with_capacity(count as usize);
        for i in 0..count {
            result.push(take_c_string(music_string_array_get(ptr, i)));
        }
        music_string_array_free(ptr);
        result
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PlaylistTrack {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
}

pub fn get_playlist_tracks(name: &str) -> Vec<PlaylistTrack> {
    let c_name = CString::new(name).unwrap_or_default();
    unsafe {
        let ptr = music_get_playlist_tracks(c_name.as_ptr());
        let count = music_playlist_tracks_count(ptr);
        let mut result = Vec::with_capacity(count as usize);
        for i in 0..count {
            result.push(PlaylistTrack {
                name: take_c_string(music_playlist_tracks_name(ptr, i)),
                artist: take_c_string(music_playlist_tracks_artist(ptr, i)),
                album: take_c_string(music_playlist_tracks_album(ptr, i)),
                duration: music_playlist_tracks_duration(ptr, i),
            });
        }
        music_playlist_tracks_free(ptr);
        result
    }
}

#[allow(dead_code)]
pub fn play_playlist(name: &str) {
    let c_name = CString::new(name).unwrap_or_default();
    unsafe { music_play_playlist(c_name.as_ptr()) }
}

pub fn play_track_in_playlist(name: &str, index: i32) {
    let c_name = CString::new(name).unwrap_or_default();
    unsafe { music_play_track_in_playlist(c_name.as_ptr(), index) }
}

// Search

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SearchResult {
    pub name: String,
    pub artist: String,
    pub album: String,
}

pub fn search(query: &str) -> Vec<SearchResult> {
    let c_query = CString::new(query).unwrap_or_default();
    unsafe {
        let ptr = music_search(c_query.as_ptr());
        let count = music_search_count(ptr);
        let mut result = Vec::with_capacity(count as usize);
        for i in 0..count {
            result.push(SearchResult {
                name: take_c_string(music_search_name(ptr, i)),
                artist: take_c_string(music_search_artist(ptr, i)),
                album: take_c_string(music_search_album(ptr, i)),
            });
        }
        music_search_free(ptr);
        result
    }
}

pub fn play_track(name: &str, artist: &str) {
    let c_name = CString::new(name).unwrap_or_default();
    let c_artist = CString::new(artist).unwrap_or_default();
    unsafe { music_play_track(c_name.as_ptr(), c_artist.as_ptr()) }
}

// Lyrics

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

pub fn get_lyrics(track_name: &str, artist: &str) -> Option<LyricsResult> {
    let c_name = CString::new(track_name).unwrap_or_default();
    let c_artist = CString::new(artist).unwrap_or_default();
    unsafe {
        let ptr = music_get_lyrics(c_name.as_ptr(), c_artist.as_ptr());
        if ptr.is_null() {
            return None;
        }
        let count = music_lyrics_count(ptr);
        let synced = music_lyrics_synced(ptr);
        let mut lines = Vec::with_capacity(count as usize);
        for i in 0..count {
            lines.push(LyricsLine {
                text: take_c_string(music_lyrics_text(ptr, i)),
                time: if music_lyrics_has_time(ptr, i) {
                    Some(music_lyrics_time(ptr, i))
                } else {
                    None
                },
            });
        }
        music_lyrics_free(ptr);
        Some(LyricsResult { lines, synced })
    }
}

// Open in Music.app

pub fn reveal_artist(artist: &str) {
    let c = CString::new(artist).unwrap_or_default();
    unsafe { music_reveal_artist(c.as_ptr()) }
}

pub fn reveal_album(album: &str, artist: &str) {
    let c_album = CString::new(album).unwrap_or_default();
    let c_artist = CString::new(artist).unwrap_or_default();
    unsafe { music_reveal_album(c_album.as_ptr(), c_artist.as_ptr()) }
}

pub fn add_to_playlist(name: &str) {
    let c = CString::new(name).unwrap_or_default();
    unsafe { music_add_to_playlist(c.as_ptr()) }
}

// Notifications

#[derive(Debug, Clone)]
pub struct NotificationInfo {
    pub player_state: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub total_time_ms: f64,
}

/// Parse notification info from an opaque pointer, freeing the Swift object.
pub fn parse_notification(ptr: *mut c_void) -> NotificationInfo {
    unsafe {
        let info = NotificationInfo {
            player_state: take_c_string(music_notification_player_state(ptr)),
            name: take_c_string(music_notification_name(ptr)),
            artist: take_c_string(music_notification_artist(ptr)),
            album: take_c_string(music_notification_album(ptr)),
            total_time_ms: music_notification_total_time_ms(ptr),
        };
        music_notification_free(ptr);
        info
    }
}

pub fn register_notification_callback(cb: extern "C" fn(*mut c_void)) {
    unsafe { music_register_notification_callback(cb) }
}

// Artwork

#[allow(dead_code)]
pub fn get_artwork_data() -> Option<Vec<u8>> {
    unsafe {
        let mut len: i32 = 0;
        let ptr = music_get_artwork_data(&mut len);
        if ptr.is_null() || len <= 0 {
            return None;
        }
        let data = std::slice::from_raw_parts(ptr, len as usize).to_vec();
        music_free_artwork_data(ptr);
        Some(data)
    }
}
