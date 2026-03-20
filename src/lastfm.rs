//! Last.fm integration via the `muse-scrobble` CLI.
//!
//! Shells out to `muse-scrobble` for auth, now-playing, and scrobble calls.
//! Keeps ScrobbleTracker in-process for timing logic.

use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const BIN: &str = "muse-scrobble";

/// Check if muse-scrobble is available and authenticated.
pub fn is_available() -> bool {
    Command::new(BIN)
        .arg("status")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Send "now playing" update.
pub fn now_playing(artist: &str, track: &str, album: &str, duration: u64) -> Result<(), String> {
    let output = Command::new(BIN)
        .args(["now-playing", "--artist", artist, "--track", track, "--album", album, "--duration", &duration.to_string()])
        .output()
        .map_err(|e| format!("Failed to run muse-scrobble: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

/// Submit a scrobble (fire and forget from a background thread).
pub fn scrobble(artist: &str, track: &str, album: &str, duration: u64, timestamp: u64) -> Result<(), String> {
    let output = Command::new(BIN)
        .args([
            "scrobble",
            "--artist", artist,
            "--track", track,
            "--album", album,
            "--duration", &duration.to_string(),
            "--timestamp", &timestamp.to_string(),
        ])
        .output()
        .map_err(|e| format!("Failed to run muse-scrobble: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

// MARK: - Scrobble Tracker

pub struct ScrobbleTracker {
    current_key: String,
    started_playing: Option<Instant>,
    accumulated_secs: f64,
    scrobbled: bool,
    sent_now_playing: bool,
    start_timestamp: u64,
    pub artist: String,
    pub track_name: String,
    pub album: String,
    pub duration: f64,
}

impl ScrobbleTracker {
    pub fn new() -> Self {
        Self {
            current_key: String::new(),
            started_playing: None,
            accumulated_secs: 0.0,
            scrobbled: false,
            sent_now_playing: false,
            start_timestamp: 0,
            artist: String::new(),
            track_name: String::new(),
            album: String::new(),
            duration: 0.0,
        }
    }

    pub fn on_track_change(&mut self, name: &str, artist: &str, album: &str, duration: f64) {
        let key = format!("{}\t{}", artist, name);
        if key == self.current_key {
            return;
        }
        self.current_key = key;
        self.artist = artist.to_string();
        self.track_name = name.to_string();
        self.album = album.to_string();
        self.duration = duration;
        self.accumulated_secs = 0.0;
        self.scrobbled = false;
        self.sent_now_playing = false;
        self.started_playing = Some(Instant::now());
        self.start_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    pub fn on_play(&mut self) {
        if self.started_playing.is_none() {
            self.started_playing = Some(Instant::now());
        }
    }

    pub fn on_pause(&mut self) {
        if let Some(start) = self.started_playing.take() {
            self.accumulated_secs += start.elapsed().as_secs_f64();
        }
    }

    pub fn should_send_now_playing(&self) -> bool {
        !self.sent_now_playing && !self.current_key.is_empty()
    }

    pub fn mark_now_playing_sent(&mut self) {
        self.sent_now_playing = true;
    }

    pub fn should_scrobble(&self) -> bool {
        if self.scrobbled || self.current_key.is_empty() || self.duration < 30.0 {
            return false;
        }
        let total = self.total_play_time();
        let threshold = (self.duration * 0.5).min(240.0);
        total >= threshold
    }

    pub fn mark_scrobbled(&mut self) {
        self.scrobbled = true;
    }

    pub fn start_timestamp(&self) -> u64 {
        self.start_timestamp
    }

    fn total_play_time(&self) -> f64 {
        let extra = self
            .started_playing
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        self.accumulated_secs + extra
    }
}
