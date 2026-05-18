mod backend;
#[cfg(all(feature = "apple-music", target_os = "macos"))]
mod bridge;
mod handlers;
mod lastfm;
mod playlist;
#[cfg(feature = "spotify")]
mod spotify;
mod state;
mod theme;
mod ui;

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use ratatui_image::picker::{Picker, ProtocolType};

use backend::MusicBackend;
use handlers::{apply_fresh_state, fetch_artwork, handle_key, handle_notification, AppEvent};
use state::{AppState, LibrarySubView, Tab};
use theme::Theme;

const PAGE_SIZE: usize = 20;

fn create_backend() -> Arc<dyn MusicBackend> {
    let config = load_backend_config();
    match config.as_deref() {
        #[cfg(feature = "spotify")]
        Some("spotify") => {
            let client_id = load_spotify_client_id().unwrap_or_else(|| {
                eprintln!("muse: Spotify backend selected but client_id is missing.");
                eprintln!();
                eprintln!("Add a [spotify] section to ~/.config/muse/config.toml:");
                eprintln!();
                eprintln!("    [spotify]");
                eprintln!("    client_id = \"YOUR_CLIENT_ID\"");
                eprintln!();
                eprintln!("Register an app at https://developer.spotify.com/dashboard");
                eprintln!("to obtain a client_id (no client secret needed — PKCE flow).");
                std::process::exit(1);
            });
            Arc::new(spotify::SpotifyBackend::new(&client_id))
        }
        #[cfg(not(feature = "spotify"))]
        Some("spotify") => {
            eprintln!("Spotify support not compiled in. Build with: cargo build --features spotify");
            std::process::exit(1);
        }
        #[cfg(all(feature = "apple-music", target_os = "macos"))]
        _ => Arc::new(bridge::AppleMusicBackend::new()),
        #[cfg(not(feature = "apple-music"))]
        _ => {
            eprintln!("No backend available. Set backend=spotify in ~/.config/muse/config");
            std::process::exit(1);
        }
    }
}

fn handle_command(cmd: &str, backend: &dyn MusicBackend) -> io::Result<()> {
    match cmd {
        "next" => playlist::cli_next(backend),
        "prev" | "previous" => playlist::cli_prev(backend),
        "play" | "pause" | "toggle" => backend.play_pause(),
        "shuffle" => backend.toggle_shuffle(),
        "favorite" | "fav" => backend.toggle_favorite(),
        _ => {
            eprintln!("Unknown command: {cmd}");
            eprintln!("Usage: muse [next|prev|play|shuffle|fav]");
            std::process::exit(1);
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let backend = create_backend();

    // Handle CLI subcommands (e.g. `muse next`, `muse prev`)
    if let Some(cmd) = std::env::args().nth(1) {
        return handle_command(&cmd, &*backend);
    }

    // Ensure music service is running and fetch initial state BEFORE entering raw mode.
    backend.ensure_running();
    let initial_state = backend.fetch_state();
    let initial_playlists = backend.get_playlists();

    // Check config for artwork preference before querying terminal
    let show_artwork = read_config()
        .and_then(|doc| doc.get("show_artwork").and_then(|v| v.as_bool()))
        .unwrap_or(true);

    // Detect image protocol before entering raw mode (queries terminal).
    // On failure, fall back to halfblocks and surface the reason via the
    // error overlay so users can see why sixel/kitty wasn't selected.
    //
    // If the user has set image_protocol in config.toml, we still run
    // from_query_stdio to learn the font size, then override the protocol
    // type. This lets users force kitty in terminals where the graphics
    // capability query doesn't make it through (e.g. nested tmux).
    let mut startup_error: Option<String> = None;
    let configured_protocol = load_image_protocol();
    let picker = if show_artwork {
        let mut p = match Picker::from_query_stdio() {
            Ok(p) => p,
            Err(e) => {
                startup_error = Some(format!(
                    "Terminal image protocol detection failed: {}\n\nFalling back to halfblocks.",
                    e
                ));
                Picker::halfblocks()
            }
        };
        if let Some(name) = configured_protocol.as_deref() {
            match parse_protocol_type(name) {
                Some(pt) => p.set_protocol_type(pt),
                None => {
                    startup_error = Some(format!(
                        "Unknown image_protocol \"{}\". Expected: auto, kitty, sixel, iterm2, or halfblocks.",
                        name
                    ));
                }
            }
        }
        Some(p)
    } else {
        None
    };

    // Fetch initial artwork before raw mode
    let initial_artwork = if let (Some(_), Some(ref picker)) = (&initial_state.track, &picker) {
        fetch_artwork(picker, &*backend)
    } else {
        None
    };
    let initial_artwork_key = initial_state
        .track
        .as_ref()
        .map(|t| format!("{}\t{}", t.artist, t.album))
        .unwrap_or_default();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
    let ratatui_backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(ratatui_backend)?;

    let result = run_app(
        &mut terminal,
        picker,
        initial_state,
        initial_playlists,
        initial_artwork,
        initial_artwork_key,
        backend,
        startup_error,
    );

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    picker: Option<Picker>,
    initial_state: backend::FullState,
    initial_playlists: Vec<String>,
    initial_artwork: Option<ratatui_image::protocol::StatefulProtocol>,
    initial_artwork_key: String,
    backend: Arc<dyn MusicBackend>,
    startup_error: Option<String>,
) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<AppEvent>();

    let mut state = AppState::default();
    state.theme.all = theme::load_themes();
    state.overlays.error_message = startup_error;
    let mut current_theme = theme::default_theme();
    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(2);
    let picker = picker.map(std::sync::Arc::new);

    // Artwork lives outside AppState so ui::draw can hold a stable `&mut`
    // to it across renders. The kitty graphics protocol relies on the
    // StatefulProtocol's transmit-once state surviving frame-to-frame; if it
    // sat inside AppState we'd have to take()/restore it every frame to
    // satisfy the borrow checker, which broke kitty placeholder rendering.
    let mut current_artwork: Option<ratatui_image::protocol::StatefulProtocol> = initial_artwork;

    // Apply initial artwork key BEFORE apply_fresh_state so it doesn't see a key
    // change and spawn a redundant (possibly failing) background fetch.
    state.player.artwork_key = initial_artwork_key;

    // Apply the initial state fetched before raw mode
    apply_fresh_state(&mut state, &mut current_artwork, &initial_state, &picker, &tx, &backend);
    state.library.playlists = initial_playlists;
    let mut last_position_update = Instant::now();

    // Restore queue from persisted state (playlist name + index)
    if let Some((playlist_name, selected, _total)) = playlist::load_queue_state() {
        let tracks = backend.get_playlist_tracks(&playlist_name);
        if !tracks.is_empty() {
            let sel = selected.min(tracks.len() - 1);
            state.queue.playlist_name = playlist_name;
            state.queue.tracks = tracks;
            state.queue.selected = sel;
            state.queue.playing = Some(sel);
            if sel >= PAGE_SIZE {
                state.queue.scroll = sel.saturating_sub(3);
            }
        }
    }

    // Load config
    load_config(&mut state, &mut current_theme);

    // Last.fm (via muse-scrobble CLI)
    let mut lastfm_available = lastfm::is_available();
    let mut lastfm_last_check = Instant::now();
    let lastfm_recheck_interval = Duration::from_secs(60);
    let mut scrobble_tracker = lastfm::ScrobbleTracker::new();
    if lastfm_available {
        state.lastfm_status = "last.fm".to_string();
    }

    // Shared flag used to tell background threads to exit promptly when the
    // app is quitting (rather than relying on channel-disconnect, which can
    // leave threads sleeping for a full polling interval after main returns).
    let shutdown = Arc::new(AtomicBool::new(false));

    // Set up notification delivery from the backend
    {
        let tx_notify = tx.clone();
        let (notify_tx, notify_rx) = mpsc::channel::<backend::NotificationInfo>();
        backend.setup_notifications(notify_tx, shutdown.clone());

        // Bridge thread: forward backend notifications to AppEvent channel
        std::thread::spawn(move || {
            for info in notify_rx {
                if tx_notify
                    .send(AppEvent::MusicNotification(info))
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    // Spawn input thread
    let tx_input = tx.clone();
    let shutdown_input = shutdown.clone();
    std::thread::spawn(move || loop {
        if shutdown_input.load(Ordering::Relaxed) {
            break;
        }
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if tx_input.send(AppEvent::Key(key)).is_err() {
                    break;
                }
            }
        }
    });

    // Spawn tick thread (for progress interpolation + runloop pumping)
    let tx_tick = tx.clone();
    let shutdown_tick = shutdown.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        if shutdown_tick.load(Ordering::Relaxed) {
            break;
        }
        if tx_tick.send(AppEvent::Tick).is_err() {
            break;
        }
    });

    let result = 'main_loop: loop {
        // Render — pass elapsed-since-last-position-update to ui::draw so it
        // can interpolate the displayed track position without us cloning
        // AppState every tick.
        let elapsed = if state.player.playback == backend::PlayerState::Playing {
            last_position_update.elapsed().as_secs_f64()
        } else {
            0.0
        };
        terminal.draw(|f| {
            ui::draw(f, &mut state, &current_theme, &mut current_artwork, elapsed)
        })?;

        // Wait for events (short timeout to keep rendering smooth)
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => match event {
                AppEvent::Key(key) => {
                    if handle_key(key, &mut state, &mut current_theme, &tx, &backend) {
                        break 'main_loop Ok(());
                    }
                }
                AppEvent::Tick => {
                    // Let backend do periodic main-thread work (e.g. pump RunLoop)
                    backend.tick();

                    // Periodic state refresh
                    if last_refresh.elapsed() >= refresh_interval {
                        last_refresh = Instant::now();
                        let tx2 = tx.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || {
                            let fresh = b.fetch_state();
                            let _ = tx2.send(AppEvent::StateRefreshed(fresh));
                        });
                    }

                    // Last.fm: periodically re-check availability if not connected
                    if !lastfm_available && lastfm_last_check.elapsed() >= lastfm_recheck_interval {
                        lastfm_last_check = Instant::now();
                        lastfm_available = lastfm::is_available();
                        if lastfm_available {
                            state.lastfm_status = "last.fm".to_string();
                        }
                    }

                    // Last.fm scrobble check
                    if lastfm_available && scrobble_tracker.should_scrobble() {
                        scrobble_tracker.mark_scrobbled();
                        let artist = scrobble_tracker.artist.clone();
                        let track = scrobble_tracker.track_name.clone();
                        let album = scrobble_tracker.album.clone();
                        let ts = scrobble_tracker.start_timestamp();
                        let dur = scrobble_tracker.duration as u64;
                        let tx2 = tx.clone();
                        std::thread::spawn(move || {
                            let result = lastfm::scrobble(&artist, &track, &album, dur, ts);
                            let _ = tx2.send(AppEvent::LastfmScrobbleResult(result));
                        });
                    }
                }
                AppEvent::MusicNotification(info) => {
                    handle_notification(&mut state, &info, &picker, &tx, &backend);
                    last_position_update = Instant::now();

                    // Last.fm: track play state changes
                    match info.player_state.as_str() {
                        "Playing" => scrobble_tracker.on_play(),
                        "Paused" => scrobble_tracker.on_pause(),
                        _ => {}
                    }

                    // Last.fm: new track detection
                    if !info.name.is_empty() {
                        let is_new_track = scrobble_tracker.track_name != info.name
                            || scrobble_tracker.artist != info.artist;
                        if is_new_track {
                            scrobble_tracker.on_track_change(
                                &info.name,
                                &info.artist,
                                &info.album,
                                info.total_time_ms / 1000.0,
                            );
                        }
                        // Send "now playing" if needed
                        if lastfm_available && scrobble_tracker.should_send_now_playing() {
                            scrobble_tracker.mark_now_playing_sent();
                            let artist = info.artist.clone();
                            let track = info.name.clone();
                            let album = info.album.clone();
                            let dur = (info.total_time_ms / 1000.0) as u64;
                            let tx2 = tx.clone();
                            std::thread::spawn(move || {
                                if let Err(_) = lastfm::now_playing(&artist, &track, &album, dur) {
                                    let _ = tx2.send(AppEvent::LastfmScrobbleResult(Err(String::new())));
                                }
                            });
                        }
                    }

                    // Fetch lyrics for new track if needed
                    if state.lyrics_enabled && !info.name.is_empty() {
                        let lyrics_key = format!("{}\t{}", info.name, info.artist);
                        if lyrics_key != state.lyrics.track_key {
                            state.lyrics.track_key = lyrics_key.clone();
                            state.lyrics.scroll = 0;
                            state.lyrics.manual_scroll = false;
                            let tx2 = tx.clone();
                            let name = info.name.clone();
                            let artist = info.artist.clone();
                            let b = backend.clone();
                            std::thread::spawn(move || {
                                let result = b.get_lyrics(&name, &artist);
                                let _ = tx2.send(AppEvent::LyricsLoaded(lyrics_key, result));
                            });
                        }
                    }
                }
                AppEvent::StateRefreshed(fresh) => {
                    let was_not_running = !state.player.music_running;
                    apply_fresh_state(&mut state, &mut current_artwork, &fresh, &picker, &tx, &backend);
                    last_position_update = Instant::now();

                    // When music service transitions to running, load playlists
                    if was_not_running && state.player.music_running && state.library.playlists.is_empty() {
                        let tx2 = tx.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || {
                            let playlists = b.get_playlists();
                            let _ = tx2.send(AppEvent::PlaylistsLoaded(playlists));
                        });
                    }
                }
                AppEvent::PlaylistsLoaded(playlists) => {
                    state.library.playlists = playlists;
                }
                AppEvent::PlaylistTracksLoaded(playlist_name, tracks) => {
                    if let LibrarySubView::Tracks(ref current) = state.library.sub_view {
                        if *current == playlist_name {
                            state.library.tracks = tracks;
                        }
                    }
                }
                AppEvent::SearchResults(query, results) => {
                    if state.search.query == query {
                        state.search.results = results;
                    }
                }
                AppEvent::LyricsLoaded(key, result) => {
                    if state.lyrics.track_key == key {
                        if let Some(r) = result {
                            state.lyrics.lines = r.lines;
                            state.lyrics.synced = r.synced;
                        } else {
                            state.lyrics.lines.clear();
                            state.lyrics.synced = false;
                        }
                        state.lyrics.scroll = 0;
                        state.lyrics.manual_scroll = false;
                    }
                }
                AppEvent::ArtworkLoaded(key, proto) => {
                    if state.player.artwork_key == key {
                        current_artwork = Some(proto);
                    }
                }
                AppEvent::LastfmScrobbleResult(result) => {
                    match result {
                        Ok(()) => {
                            state.lastfm_status = "last.fm ✓".to_string();
                        }
                        Err(_) => {
                            // Auth may have expired — recheck on next tick cycle
                            lastfm_available = false;
                            lastfm_last_check = Instant::now() - lastfm_recheck_interval;
                            state.lastfm_status = String::new();
                        }
                    }
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break 'main_loop Ok(()),
        }
    };

    // Signal background threads to exit promptly before we tear down.
    shutdown.store(true, Ordering::Relaxed);
    result
}


// Config

pub(crate) fn config_dir() -> std::path::PathBuf {
    dirs_or_home().join(".config").join("muse")
}

pub(crate) fn config_file() -> std::path::PathBuf {
    config_dir().join("config.toml")
}

/// Read and parse the config file as TOML. Returns None if missing or unparseable.
fn read_config() -> Option<toml::Value> {
    let path = config_file();
    // Migrate legacy plain-text config to TOML on first read
    if !path.exists() {
        let legacy = config_dir().join("config");
        if legacy.exists() {
            if let Ok(contents) = std::fs::read_to_string(&legacy) {
                if let Some(migrated) = migrate_legacy_config(&contents) {
                    let _ = std::fs::write(&path, &migrated);
                    let _ = std::fs::remove_file(&legacy);
                    return migrated.parse().ok();
                }
            }
        }
    }
    std::fs::read_to_string(&path).ok()?.parse().ok()
}

/// Convert legacy KEY=VALUE config to TOML format.
fn migrate_legacy_config(contents: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut spotify_lines = Vec::new();
    for line in contents.lines() {
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {
            continue;
        }
        let (key, val) = (parts[0].trim(), parts[1].trim());
        match key {
            "backend" => lines.push(format!("backend = \"{}\"", val)),
            "theme" => lines.push(format!("theme = \"{}\"", val)),
            "spotify_client_id" => spotify_lines.push(format!("client_id = \"{}\"", val)),
            _ => {}
        }
    }
    if !spotify_lines.is_empty() {
        lines.push(String::new());
        lines.push("[spotify]".to_string());
        lines.extend(spotify_lines);
    }
    if lines.is_empty() {
        return None;
    }
    Some(lines.join("\n") + "\n")
}

fn dirs_or_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
}

fn load_config(state: &mut AppState, theme: &mut Theme) {
    let Some(doc) = read_config() else { return };
    if let Some(name) = doc.get("theme").and_then(|v| v.as_str()) {
        if let Some((idx, t)) = theme::find_theme(name, &state.theme.all) {
            state.theme.name = name.to_string();
            state.theme.selected = idx;
            *theme = t;
        }
    }
    if let Some(tab) = doc.get("default_tab").and_then(|v| v.as_str()) {
        if let Some(t) = Tab::from_name(tab) {
            state.active_tab = t;
        }
    }
    if let Some(val) = doc.get("ui_width") {
        if val.as_str() == Some("auto") {
            state.ui_width = 0;
        } else if let Some(w) = val.as_integer() {
            state.ui_width = (w as u16).max(40);
        }
    }
    if let Some(show) = doc.get("show_artwork").and_then(|v| v.as_bool()) {
        state.show_artwork = show;
    }
    if let Some(enabled) = doc.get("lyrics_enabled").and_then(|v| v.as_bool()) {
        state.lyrics_enabled = enabled;
    }
}

/// Read the `image_protocol` config value. Returns None if missing or set to "auto".
fn load_image_protocol() -> Option<String> {
    let doc = read_config()?;
    let value = doc.get("image_protocol")?.as_str()?;
    if value.eq_ignore_ascii_case("auto") || value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Map a config string to a ProtocolType. Returns None for unknown values.
fn parse_protocol_type(name: &str) -> Option<ProtocolType> {
    match name.to_ascii_lowercase().as_str() {
        "kitty" => Some(ProtocolType::Kitty),
        "sixel" => Some(ProtocolType::Sixel),
        "iterm2" | "iterm" => Some(ProtocolType::Iterm2),
        "halfblocks" | "halfblock" => Some(ProtocolType::Halfblocks),
        _ => None,
    }
}

fn load_backend_config() -> Option<String> {
    let doc = read_config()?;
    doc.get("backend")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(feature = "spotify")]
fn load_spotify_client_id() -> Option<String> {
    let doc = read_config()?;
    doc.get("spotify")
        .and_then(|t| t.get("client_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

