//! Spotify backend via the Spotify Web API.
//!
//! Uses OAuth PKCE flow for authentication (no client secret needed).
//! Token is cached in `~/.config/muse/spotify_token.json` and auto-refreshed.
//! Playback state changes are detected by polling the API in a background thread.

use std::io::{BufRead, Read, Write as IoWrite};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{mpsc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::backend::{
    FullState, LyricsLine, LyricsResult, MusicBackend, NotificationInfo, PlaylistTrack,
    PlayerState, RepeatMode, SearchResult, Track,
};

// MARK: - OAuth token

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyToken {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
}

impl SpotifyToken {
    fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now >= self.expires_at.saturating_sub(60) // refresh 60s early
    }
}

// MARK: - SpotifyBackend

pub struct SpotifyBackend {
    client_id: String,
    token: Mutex<Option<SpotifyToken>>,
}

impl SpotifyBackend {
    pub fn new(client_id: &str) -> Self {
        let backend = Self {
            client_id: client_id.to_string(),
            token: Mutex::new(None),
        };

        // Try to load cached token
        if let Some(token) = Self::load_cached_token() {
            *backend.token.lock().unwrap() = Some(token);
        }

        backend
    }

    /// Run the OAuth PKCE authentication flow.
    /// Opens the user's browser and listens for the callback.
    pub fn authenticate(&self) -> Result<(), String> {
        let (verifier, challenge) = Self::pkce_pair();
        let redirect_uri = "http://localhost:18234/callback";

        let auth_url = format!(
            "https://accounts.spotify.com/authorize?\
            client_id={}&response_type=code&redirect_uri={}&\
            code_challenge_method=S256&code_challenge={}&\
            scope={}",
            self.client_id,
            urlencoded(redirect_uri),
            challenge,
            urlencoded(
                "user-read-playback-state \
                 user-modify-playback-state \
                 user-read-currently-playing \
                 user-library-read \
                 user-library-modify \
                 playlist-read-private \
                 playlist-modify-public \
                 playlist-modify-private"
            ),
        );

        // Open browser
        let _ = std::process::Command::new("open")
            .arg(&auth_url)
            .spawn();

        // Listen for the redirect
        let listener = TcpListener::bind("127.0.0.1:18234")
            .map_err(|e| format!("Failed to bind callback server: {}", e))?;
        listener
            .set_nonblocking(false)
            .map_err(|e| format!("Failed to set blocking: {}", e))?;

        let (stream, _) = listener
            .accept()
            .map_err(|e| format!("Failed to accept connection: {}", e))?;

        let reader = std::io::BufReader::new(&stream);
        let request_line = reader
            .lines()
            .next()
            .ok_or("No request received")?
            .map_err(|e| format!("Failed to read request: {}", e))?;

        // Send response before processing
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
            <html><body><h2>Authenticated! You can close this tab.</h2></body></html>";
        let mut stream_w = &stream;
        let _ = stream_w.write_all(response.as_bytes());
        let _ = stream_w.flush();
        drop(stream);
        drop(listener);

        // Extract code from: GET /callback?code=XXXX HTTP/1.1
        let code = request_line
            .split_whitespace()
            .nth(1)
            .and_then(|path| {
                path.split('?')
                    .nth(1)?
                    .split('&')
                    .find(|p| p.starts_with("code="))
                    .map(|p| p.trim_start_matches("code=").to_string())
            })
            .ok_or("No auth code in callback")?;

        // Exchange code for token
        let body = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            urlencoded(&code),
            urlencoded(redirect_uri),
            self.client_id,
            verifier,
        );

        let resp: serde_json::Value = ureq::post("https://accounts.spotify.com/api/token")
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&body)
            .map_err(|e| format!("Token exchange failed: {}", e))?
            .into_json()
            .map_err(|e| format!("Failed to parse token response: {}", e))?;

        let token = Self::parse_token_response(&resp)?;
        Self::save_cached_token(&token);
        *self.token.lock().unwrap() = Some(token);

        Ok(())
    }

    // -- Token management --

    fn ensure_token(&self) -> Result<String, String> {
        let mut guard = self.token.lock().unwrap();
        let token = guard.as_mut().ok_or("Not authenticated")?;

        if token.is_expired() {
            let new_token = Self::refresh_token(&self.client_id, &token.refresh_token)?;
            *token = new_token;
            Self::save_cached_token(token);
        }

        Ok(token.access_token.clone())
    }

    fn refresh_token(client_id: &str, refresh_token: &str) -> Result<SpotifyToken, String> {
        let body = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            urlencoded(refresh_token),
            client_id,
        );

        let resp: serde_json::Value = ureq::post("https://accounts.spotify.com/api/token")
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&body)
            .map_err(|e| format!("Token refresh failed: {}", e))?
            .into_json()
            .map_err(|e| format!("Failed to parse refresh response: {}", e))?;

        Self::parse_token_response(&resp)
    }

    fn parse_token_response(resp: &serde_json::Value) -> Result<SpotifyToken, String> {
        let access_token = resp["access_token"]
            .as_str()
            .ok_or("No access_token in response")?
            .to_string();
        let refresh_token = resp["refresh_token"]
            .as_str()
            .ok_or("No refresh_token in response")?
            .to_string();
        let expires_in = resp["expires_in"].as_u64().unwrap_or(3600);
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + expires_in;

        Ok(SpotifyToken {
            access_token,
            refresh_token,
            expires_at,
        })
    }

    fn token_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("muse")
            .join("spotify_token.json")
    }

    fn load_cached_token() -> Option<SpotifyToken> {
        let data = std::fs::read_to_string(Self::token_path()).ok()?;
        serde_json::from_str(&data).ok()
    }

    fn save_cached_token(token: &SpotifyToken) {
        let path = Self::token_path();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, serde_json::to_string_pretty(token).unwrap_or_default());
    }

    fn is_authenticated(&self) -> bool {
        self.token.lock().unwrap().is_some()
    }

    // -- PKCE helpers --

    fn pkce_pair() -> (String, String) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Generate a random 64-byte verifier using available entropy
        let mut verifier_bytes = [0u8; 64];
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        std::process::id().hash(&mut hasher);
        let mut state = hasher.finish();
        for byte in verifier_bytes.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *byte = (state >> 33) as u8;
        }

        // Base64url encode the verifier
        let verifier = base64url_encode(&verifier_bytes);

        // SHA-256 hash for challenge
        let challenge = {
            let hash = sha256(verifier.as_bytes());
            base64url_encode(&hash)
        };

        (verifier, challenge)
    }

    // -- API helpers --

    fn api_get(&self, path: &str) -> Result<serde_json::Value, String> {
        let token = self.ensure_token()?;
        let url = if path.starts_with("https://") {
            path.to_string()
        } else {
            format!("https://api.spotify.com/v1{}", path)
        };

        let resp = ureq::get(&url)
            .set("Authorization", &format!("Bearer {}", token))
            .call()
            .map_err(|e| format!("API request failed: {}", e))?;

        if resp.status() == 204 {
            return Ok(serde_json::Value::Null);
        }

        resp.into_json()
            .map_err(|e| format!("Failed to parse API response: {}", e))
    }

    fn api_put(&self, path: &str, body: Option<&serde_json::Value>) -> Result<(), String> {
        let token = self.ensure_token()?;
        let url = format!("https://api.spotify.com/v1{}", path);

        let req = ureq::put(&url).set("Authorization", &format!("Bearer {}", token));

        if let Some(body) = body {
            req.set("Content-Type", "application/json")
                .send_string(&body.to_string())
                .map_err(|e| format!("API PUT failed: {}", e))?;
        } else {
            req.call()
                .map_err(|e| format!("API PUT failed: {}", e))?;
        }

        Ok(())
    }

    fn api_post(&self, path: &str, body: Option<&serde_json::Value>) -> Result<(), String> {
        let token = self.ensure_token()?;
        let url = format!("https://api.spotify.com/v1{}", path);

        let req = ureq::post(&url).set("Authorization", &format!("Bearer {}", token));

        if let Some(body) = body {
            req.set("Content-Type", "application/json")
                .send_string(&body.to_string())
                .map_err(|e| format!("API POST failed: {}", e))?;
        } else {
            req.call()
                .map_err(|e| format!("API POST failed: {}", e))?;
        }

        Ok(())
    }

    fn api_delete(&self, path: &str, body: Option<&serde_json::Value>) -> Result<(), String> {
        let token = self.ensure_token()?;
        let url = format!("https://api.spotify.com/v1{}", path);

        let req = ureq::delete(&url).set("Authorization", &format!("Bearer {}", token));

        if let Some(body) = body {
            req.set("Content-Type", "application/json")
                .send_string(&body.to_string())
                .map_err(|e| format!("API DELETE failed: {}", e))?;
        } else {
            req.call()
                .map_err(|e| format!("API DELETE failed: {}", e))?;
        }

        Ok(())
    }

    // -- Lyrics via LRCLIB --

    fn fetch_lrclib(track_name: &str, artist: &str) -> Option<LyricsResult> {
        let url = format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}",
            urlencoded(track_name),
            urlencoded(artist),
        );

        let resp: serde_json::Value = ureq::get(&url)
            .call()
            .ok()?
            .into_json()
            .ok()?;

        // Prefer synced lyrics
        if let Some(synced) = resp["syncedLyrics"].as_str() {
            if !synced.is_empty() {
                let lines = parse_synced_lyrics(synced);
                if !lines.is_empty() {
                    return Some(LyricsResult {
                        lines,
                        synced: true,
                    });
                }
            }
        }

        // Fall back to plain lyrics
        if let Some(plain) = resp["plainLyrics"].as_str() {
            if !plain.is_empty() {
                let lines = plain
                    .lines()
                    .map(|l| LyricsLine {
                        text: l.to_string(),
                        time: None,
                    })
                    .collect();
                return Some(LyricsResult {
                    lines,
                    synced: false,
                });
            }
        }

        None
    }
}

/// Parse synced lyrics in LRC format: `[MM:SS.mm]text`
fn parse_synced_lyrics(text: &str) -> Vec<LyricsLine> {
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('[') {
            if let Some(bracket_end) = rest.find(']') {
                let timestamp = &rest[..bracket_end];
                let text = rest[bracket_end + 1..].to_string();

                // Parse MM:SS.mm
                let time = parse_lrc_timestamp(timestamp);
                lines.push(LyricsLine { text, time });
            }
        }
    }
    lines
}

fn parse_lrc_timestamp(ts: &str) -> Option<f64> {
    let parts: Vec<&str> = ts.split(':').collect();
    if parts.len() == 2 {
        let minutes: f64 = parts[0].parse().ok()?;
        let seconds: f64 = parts[1].parse().ok()?;
        Some(minutes * 60.0 + seconds)
    } else {
        None
    }
}

// MARK: - MusicBackend implementation

impl MusicBackend for SpotifyBackend {
    fn name(&self) -> &str {
        "Spotify"
    }

    fn ensure_running(&self) {
        if !self.is_authenticated() {
            eprintln!("Spotify: not authenticated. Running OAuth flow...");
            if let Err(e) = self.authenticate() {
                eprintln!("Spotify authentication failed: {}", e);
            }
        }
    }

    fn fetch_state(&self) -> FullState {
        let default = FullState {
            music_running: self.is_authenticated(),
            player_state: PlayerState::Stopped,
            volume: 50,
            shuffle_enabled: false,
            repeat_mode: RepeatMode::Off,
            track: None,
            track_favorited: false,
        };

        let resp = match self.api_get("/me/player") {
            Ok(v) => v,
            Err(_) => return default,
        };

        if resp.is_null() {
            return default;
        }

        let is_playing = resp["is_playing"].as_bool().unwrap_or(false);
        let volume = resp["device"]["volume_percent"].as_i64().unwrap_or(50) as i32;
        let shuffle = resp["shuffle_state"].as_bool().unwrap_or(false);
        let repeat = match resp["repeat_state"].as_str().unwrap_or("off") {
            "track" => RepeatMode::One,
            "context" => RepeatMode::All,
            _ => RepeatMode::Off,
        };

        let track = resp["item"].as_object().map(|item| {
            let artists: Vec<&str> = item
                .get("artists")
                .and_then(|a| a.as_array())
                .map(|arr| arr.iter().filter_map(|a| a["name"].as_str()).collect())
                .unwrap_or_default();

            Track {
                name: item
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
                artist: artists.join(", "),
                album: item
                    .get("album")
                    .and_then(|a| a["name"].as_str())
                    .unwrap_or("")
                    .to_string(),
                duration: item
                    .get("duration_ms")
                    .and_then(|d| d.as_f64())
                    .unwrap_or(0.0)
                    / 1000.0,
                position: resp["progress_ms"].as_f64().unwrap_or(0.0) / 1000.0,
            }
        });

        FullState {
            music_running: true,
            player_state: if is_playing {
                PlayerState::Playing
            } else {
                PlayerState::Paused
            },
            volume,
            shuffle_enabled: shuffle,
            repeat_mode: repeat,
            track,
            track_favorited: false, // checked separately if needed
        }
    }

    fn play_pause(&self) {
        let state = self.fetch_state();
        if state.player_state == PlayerState::Playing {
            let _ = self.api_put("/me/player/pause", None);
        } else {
            let _ = self.api_put("/me/player/play", None);
        }
    }

    fn next_track(&self) {
        let _ = self.api_post("/me/player/next", None);
    }

    fn previous_track(&self) {
        let _ = self.api_post("/me/player/previous", None);
    }

    fn set_volume(&self, vol: i32) {
        let vol = vol.clamp(0, 100);
        let _ = self.api_put(&format!("/me/player/volume?volume_percent={}", vol), None);
    }

    fn toggle_shuffle(&self) {
        let state = self.fetch_state();
        let new_state = !state.shuffle_enabled;
        let _ = self.api_put(
            &format!("/me/player/shuffle?state={}", new_state),
            None,
        );
    }

    fn cycle_repeat(&self) {
        let state = self.fetch_state();
        let new_mode = match state.repeat_mode {
            RepeatMode::Off => "context",
            RepeatMode::All => "track",
            RepeatMode::One => "off",
        };
        let _ = self.api_put(&format!("/me/player/repeat?state={}", new_mode), None);
    }

    fn toggle_favorite(&self) {
        // Check if current track is saved, then toggle
        let state = self.fetch_state();
        if let Some(_track) = &state.track {
            // Get track ID from current playback
            if let Ok(resp) = self.api_get("/me/player/currently-playing") {
                if let Some(id) = resp["item"]["id"].as_str() {
                    // Check if saved
                    if let Ok(check) = self.api_get(&format!("/me/tracks/contains?ids={}", id)) {
                        let is_saved = check
                            .as_array()
                            .and_then(|a| a.first())
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);

                        let body =
                            serde_json::json!({ "ids": [id] });
                        if is_saved {
                            let _ = self.api_delete("/me/tracks", Some(&body));
                        } else {
                            let _ = self.api_put("/me/tracks", Some(&body));
                        }
                    }
                }
            }
        }
    }

    fn get_playlists(&self) -> Vec<String> {
        let mut playlists = Vec::new();
        let mut offset = 0;

        loop {
            let resp = match self.api_get(&format!(
                "/me/playlists?limit=50&offset={}",
                offset
            )) {
                Ok(v) => v,
                Err(_) => break,
            };

            let items = match resp["items"].as_array() {
                Some(items) => items,
                None => break,
            };

            if items.is_empty() {
                break;
            }

            for item in items {
                if let Some(name) = item["name"].as_str() {
                    playlists.push(name.to_string());
                }
            }

            let total = resp["total"].as_u64().unwrap_or(0) as usize;
            offset += items.len();
            if offset >= total {
                break;
            }
        }

        playlists
    }

    fn get_playlist_tracks(&self, name: &str) -> Vec<PlaylistTrack> {
        // First, find the playlist ID by name
        let playlist_id = match self.find_playlist_id(name) {
            Some(id) => id,
            None => return Vec::new(),
        };

        let mut tracks = Vec::new();
        let mut offset = 0;

        loop {
            let resp = match self.api_get(&format!(
                "/playlists/{}/tracks?limit=100&offset={}&fields=items(track(name,artists,album(name),duration_ms)),total",
                playlist_id, offset
            )) {
                Ok(v) => v,
                Err(_) => break,
            };

            let items = match resp["items"].as_array() {
                Some(items) => items,
                None => break,
            };

            if items.is_empty() {
                break;
            }

            for item in items {
                let track = &item["track"];
                if track.is_null() {
                    continue;
                }

                let artists: Vec<&str> = track["artists"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|a| a["name"].as_str()).collect())
                    .unwrap_or_default();

                tracks.push(PlaylistTrack {
                    name: track["name"].as_str().unwrap_or("").to_string(),
                    artist: artists.join(", "),
                    album: track["album"]["name"].as_str().unwrap_or("").to_string(),
                    duration: track["duration_ms"].as_f64().unwrap_or(0.0) / 1000.0,
                });
            }

            let total = resp["total"].as_u64().unwrap_or(0) as usize;
            offset += items.len();
            if offset >= total {
                break;
            }
        }

        tracks
    }

    fn play_track_in_playlist(&self, playlist: &str, index: usize) {
        if let Some(playlist_id) = self.find_playlist_id(playlist) {
            let body = serde_json::json!({
                "context_uri": format!("spotify:playlist:{}", playlist_id),
                "offset": { "position": index },
            });
            let _ = self.api_put("/me/player/play", Some(&body));
        }
    }

    fn search(&self, query: &str) -> Vec<SearchResult> {
        let resp = match self.api_get(&format!(
            "/search?q={}&type=track&limit=20",
            urlencoded(query)
        )) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        resp["tracks"]["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let artists: Vec<&str> = item["artists"]
                            .as_array()
                            .map(|arr| {
                                arr.iter().filter_map(|a| a["name"].as_str()).collect()
                            })
                            .unwrap_or_default();

                        SearchResult {
                            name: item["name"].as_str().unwrap_or("").to_string(),
                            artist: artists.join(", "),
                            album: item["album"]["name"].as_str().unwrap_or("").to_string(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn play_track(&self, name: &str, artist: &str) {
        // Search for the track and play the first result
        let query = format!("track:{} artist:{}", name, artist);
        let resp = match self.api_get(&format!(
            "/search?q={}&type=track&limit=1",
            urlencoded(&query)
        )) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(uri) = resp["tracks"]["items"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|t| t["uri"].as_str())
        {
            let body = serde_json::json!({ "uris": [uri] });
            let _ = self.api_put("/me/player/play", Some(&body));
        }
    }

    fn get_lyrics(&self, track_name: &str, artist: &str) -> Option<LyricsResult> {
        Self::fetch_lrclib(track_name, artist)
    }

    fn get_artwork_data(&self) -> Option<Vec<u8>> {
        let resp = self.api_get("/me/player/currently-playing").ok()?;
        let images = resp["item"]["album"]["images"].as_array()?;

        // Pick the largest image (first in array is usually largest)
        let url = images.first()?.get("url")?.as_str()?;

        let resp = ureq::get(url).call().ok()?;
        let mut data = Vec::new();
        resp.into_reader()
            .read_to_end(&mut data)
            .ok()?;

        Some(data)
    }

    fn reveal_artist(&self, artist: &str) {
        // Search for artist and open in Spotify
        let resp = match self.api_get(&format!(
            "/search?q={}&type=artist&limit=1",
            urlencoded(artist)
        )) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(uri) = resp["artists"]["items"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a["external_urls"]["spotify"].as_str())
        {
            let _ = std::process::Command::new("open").arg(uri).spawn();
        }
    }

    fn reveal_album(&self, album: &str, artist: &str) {
        let query = format!("album:{} artist:{}", album, artist);
        let resp = match self.api_get(&format!(
            "/search?q={}&type=album&limit=1",
            urlencoded(&query)
        )) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(url) = resp["albums"]["items"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a["external_urls"]["spotify"].as_str())
        {
            let _ = std::process::Command::new("open").arg(url).spawn();
        }
    }

    fn add_to_playlist(&self, playlist_name: &str) {
        let playlist_id = match self.find_playlist_id(playlist_name) {
            Some(id) => id,
            None => return,
        };

        // Get current track URI
        let resp = match self.api_get("/me/player/currently-playing") {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(uri) = resp["item"]["uri"].as_str() {
            let body = serde_json::json!({ "uris": [uri] });
            let _ = self.api_post(
                &format!("/playlists/{}/tracks", playlist_id),
                Some(&body),
            );
        }
    }

    fn remove_from_playlist(&self, playlist_name: &str, index: usize) {
        let playlist_id = match self.find_playlist_id(playlist_name) {
            Some(id) => id,
            None => return,
        };

        // Get the track URI at the given index
        let resp = match self.api_get(&format!(
            "/playlists/{}/tracks?limit=1&offset={}&fields=items(track(uri))",
            playlist_id, index
        )) {
            Ok(v) => v,
            Err(_) => return,
        };

        if let Some(uri) = resp["items"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|item| item["track"]["uri"].as_str())
        {
            let body = serde_json::json!({
                "tracks": [{ "uri": uri, "positions": [index] }]
            });
            let _ = self.api_delete(
                &format!("/playlists/{}/tracks", playlist_id),
                Some(&body),
            );
        }
    }

    fn setup_notifications(&self, tx: mpsc::Sender<NotificationInfo>) {
        // Spawn a polling thread that checks playback state every 2 seconds
        let client_id = self.client_id.clone();
        let token = self.token.lock().unwrap().clone();

        std::thread::spawn(move || {
            let mut last_track_name = String::new();
            let mut last_player_state = String::new();

            loop {
                std::thread::sleep(Duration::from_secs(2));

                // Build a temporary backend just for API calls in this thread
                let backend = SpotifyBackend {
                    client_id: client_id.clone(),
                    token: Mutex::new(token.clone()),
                };

                let state = backend.fetch_state();
                let player_state_str = match state.player_state {
                    PlayerState::Playing => "Playing",
                    PlayerState::Paused => "Paused",
                    PlayerState::Stopped => "Stopped",
                }
                .to_string();

                let (track_name, artist, album, duration_ms) =
                    if let Some(ref track) = state.track {
                        (
                            track.name.clone(),
                            track.artist.clone(),
                            track.album.clone(),
                            track.duration * 1000.0,
                        )
                    } else {
                        (String::new(), String::new(), String::new(), 0.0)
                    };

                // Only send notification if something changed
                if track_name != last_track_name || player_state_str != last_player_state {
                    last_track_name = track_name.clone();
                    last_player_state = player_state_str.clone();

                    let info = NotificationInfo {
                        player_state: player_state_str,
                        name: track_name,
                        artist,
                        album,
                        total_time_ms: duration_ms,
                    };

                    if tx.send(info).is_err() {
                        break;
                    }
                }
            }
        });
    }

    fn tick(&self) {
        // No-op for Spotify — notifications are handled by the polling thread
    }

    fn needs_queue_advance(&self) -> bool {
        false
    }
}

// MARK: - Private helpers

impl SpotifyBackend {
    fn find_playlist_id(&self, name: &str) -> Option<String> {
        let mut offset = 0;

        loop {
            let resp = self
                .api_get(&format!("/me/playlists?limit=50&offset={}", offset))
                .ok()?;

            let items = resp["items"].as_array()?;
            if items.is_empty() {
                return None;
            }

            for item in items {
                if item["name"].as_str() == Some(name) {
                    return item["id"].as_str().map(|s| s.to_string());
                }
            }

            let total = resp["total"].as_u64().unwrap_or(0) as usize;
            offset += items.len();
            if offset >= total {
                return None;
            }
        }
    }
}

// MARK: - Utility functions

fn urlencoded(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => result.push_str("%20"),
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

fn base64url_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if i + 1 < data.len() {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        }
        if i + 2 < data.len() {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        }

        i += 3;
    }
    result
}

/// Minimal SHA-256 implementation for PKCE code challenge.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Pre-processing: padding
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        result[i * 4..i * 4 + 4].copy_from_slice(&val.to_be_bytes());
    }
    result
}
