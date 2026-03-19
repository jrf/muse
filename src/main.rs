mod backend;
#[cfg(all(feature = "apple-music", target_os = "macos"))]
mod bridge;
mod lastfm;
mod playlist;
#[cfg(feature = "spotify")]
mod spotify;
mod state;
mod theme;
mod ui;

use std::io;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::ImageReader;
use ratatui::prelude::*;
use ratatui_image::picker::Picker;

use backend::MusicBackend;
use state::{AppState, LibrarySubView, Tab};
use theme::Theme;

const PAGE_SIZE: usize = 20;

/// Map vim-style key combos to equivalent navigation keys.
/// When `vim_letters` is true, also map j/k/g/G to arrow keys.
/// Pass false when the user is typing into a text field.
fn normalize_nav_key(key: &KeyEvent, vim_letters: bool) -> KeyCode {
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
fn list_nav(code: KeyCode, selected: usize, scroll: usize, len: usize) -> Option<(usize, usize)> {
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

/// Events sent to the main loop.
enum AppEvent {
    Key(KeyEvent),
    Tick,
    MusicNotification(backend::NotificationInfo),
    StateRefreshed(backend::FullState),
    PlaylistsLoaded(Vec<String>),
    PlaylistTracksLoaded(String, Vec<backend::PlaylistTrack>),
    SearchResults(String, Vec<backend::SearchResult>),
    LyricsLoaded(String, Option<backend::LyricsResult>),
    ArtworkLoaded(String, ratatui_image::protocol::StatefulProtocol),
    LastfmScrobbled,
}


fn create_backend() -> Arc<dyn MusicBackend> {
    let config = load_backend_config();
    match config.as_deref() {
        #[cfg(feature = "spotify")]
        Some("spotify") => {
            let client_id = load_spotify_client_id().unwrap_or_else(|| {
                eprintln!(
                    "Spotify backend requires spotify_client_id in ~/.config/muse/config\n\
                     Get one at https://developer.spotify.com/dashboard"
                );
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

    // Detect image protocol before entering raw mode (queries terminal)
    // Falls back to halfblocks (unicode) if terminal doesn't support Sixel/Kitty/iTerm2
    let picker = Some(
        Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks()),
    );

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
) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<AppEvent>();

    let mut state = AppState::default();
    let mut current_theme = theme::default_theme();
    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(2);
    let picker = picker.map(std::sync::Arc::new);

    // Apply initial artwork BEFORE apply_fresh_state so it doesn't see a key
    // change and spawn a redundant (possibly failing) background fetch.
    state.artwork = initial_artwork;
    state.artwork_key = initial_artwork_key;

    // Apply the initial state fetched before raw mode
    apply_fresh_state(&mut state, &initial_state, &picker, &tx, &backend);
    state.playlists = initial_playlists;
    let mut last_position_update = Instant::now();

    // Load config
    load_config(&mut state, &mut current_theme);

    // Last.fm (via muse-scrobble CLI)
    let lastfm_available = lastfm::is_available();
    let mut scrobble_tracker = lastfm::ScrobbleTracker::new();
    if lastfm_available {
        state.lastfm_status = "last.fm".to_string();
    }

    // Set up notification delivery from the backend
    {
        let tx_notify = tx.clone();
        let (notify_tx, notify_rx) = mpsc::channel::<backend::NotificationInfo>();
        backend.setup_notifications(notify_tx);

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
    std::thread::spawn(move || loop {
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
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        if tx_tick.send(AppEvent::Tick).is_err() {
            break;
        }
    });

    loop {
        // Render
        let display_state = interpolated_state(&state, &last_position_update);
        terminal.draw(|f| ui::draw(f, &display_state, &current_theme, &mut state.artwork))?;

        // Wait for events (short timeout to keep rendering smooth)
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => match event {
                AppEvent::Key(key) => {
                    if handle_key(key, &mut state, &mut current_theme, &tx, &backend) {
                        return Ok(());
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
                            let _ = lastfm::scrobble(&artist, &track, &album, dur, ts);
                            let _ = tx2.send(AppEvent::LastfmScrobbled);
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
                            std::thread::spawn(move || {
                                lastfm::now_playing(&artist, &track, &album, dur);
                            });
                        }
                    }

                    // Fetch lyrics for new track if needed
                    if !info.name.is_empty() {
                        let lyrics_key = format!("{}\t{}", info.name, info.artist);
                        if lyrics_key != state.lyrics_track_key {
                            state.lyrics_track_key = lyrics_key.clone();
                            state.lyrics_scroll = 0;
                            state.lyrics_manual_scroll = false;
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
                    let was_not_running = !state.music_running;
                    apply_fresh_state(&mut state, &fresh, &picker, &tx, &backend);
                    last_position_update = Instant::now();

                    // When music service transitions to running, load playlists
                    if was_not_running && state.music_running && state.playlists.is_empty() {
                        let tx2 = tx.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || {
                            let playlists = b.get_playlists();
                            let _ = tx2.send(AppEvent::PlaylistsLoaded(playlists));
                        });
                    }
                }
                AppEvent::PlaylistsLoaded(playlists) => {
                    state.playlists = playlists;
                }
                AppEvent::PlaylistTracksLoaded(playlist_name, tracks) => {
                    if let LibrarySubView::Tracks(ref current) = state.library_sub_view {
                        if *current == playlist_name {
                            state.playlist_tracks = tracks;
                        }
                    }
                }
                AppEvent::SearchResults(query, results) => {
                    if state.search_query == query {
                        state.search_results = results;
                    }
                }
                AppEvent::LyricsLoaded(key, result) => {
                    if state.lyrics_track_key == key {
                        if let Some(r) = result {
                            state.lyrics_lines = r.lines;
                            state.lyrics_synced = r.synced;
                        } else {
                            state.lyrics_lines.clear();
                            state.lyrics_synced = false;
                        }
                        state.lyrics_scroll = 0;
                        state.lyrics_manual_scroll = false;
                    }
                }
                AppEvent::ArtworkLoaded(key, proto) => {
                    if state.artwork_key == key {
                        state.artwork = Some(proto);
                    }
                }
                AppEvent::LastfmScrobbled => {
                    state.lastfm_status = "last.fm ✓".to_string();
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn apply_fresh_state(
    state: &mut AppState,
    fresh: &backend::FullState,
    picker: &Option<std::sync::Arc<Picker>>,
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
            state.track = Some(track.clone());
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

fn interpolated_state(state: &AppState, last_update: &Instant) -> AppState {
    let mut display = AppState {
        track: state.track.clone(),
        artwork: None, // artwork is rendered separately via mutable ref
        artwork_key: state.artwork_key.clone(),
        player_state: state.player_state,
        volume: state.volume,
        shuffle_enabled: state.shuffle_enabled,
        repeat_mode: state.repeat_mode,
        music_running: state.music_running,
        active_tab: state.active_tab,
        queue_tracks: state.queue_tracks.clone(),
        queue_selected: state.queue_selected,
        queue_scroll: state.queue_scroll,
        queue_playlist_name: state.queue_playlist_name.clone(),
        playlists: state.playlists.clone(),
        library_sub_view: state.library_sub_view.clone(),
        library_selected: state.library_selected,
        library_scroll: state.library_scroll,
        playlist_tracks: state.playlist_tracks.clone(),
        playlist_tracks_selected: state.playlist_tracks_selected,
        playlist_tracks_scroll: state.playlist_tracks_scroll,
        search_query: state.search_query.clone(),
        search_results: state.search_results.clone(),
        search_selected: state.search_selected,
        search_scroll: state.search_scroll,
        lyrics_lines: state.lyrics_lines.clone(),
        lyrics_synced: state.lyrics_synced,
        lyrics_scroll: state.lyrics_scroll,
        lyrics_manual_scroll: state.lyrics_manual_scroll,
        lyrics_track_key: state.lyrics_track_key.clone(),
        theme_name: state.theme_name.clone(),
        theme_selected: state.theme_selected,
        theme_scroll: state.theme_scroll,
        show_help: state.show_help,
        current_track_favorited: state.current_track_favorited,
        show_playlist_picker: state.show_playlist_picker,
        playlist_picker_selected: state.playlist_picker_selected,
        playlist_picker_scroll: state.playlist_picker_scroll,
        lastfm_status: state.lastfm_status.clone(),
    };

    // Interpolate position when playing
    if state.player_state == backend::PlayerState::Playing {
        if let Some(ref mut track) = display.track {
            let elapsed = last_update.elapsed().as_secs_f64();
            track.position = (track.position + elapsed).min(track.duration);
        }
    }

    // Auto-scroll lyrics
    if display.lyrics_synced && !display.lyrics_manual_scroll {
        if let Some(current_idx) = display.track.as_ref().and_then(|t| {
            display
                .lyrics_lines
                .iter()
                .enumerate()
                .rev()
                .find(|(_, l)| l.time.map_or(false, |time| time <= t.position))
                .map(|(i, _)| i)
        }) {
            let max_rows = 20; // approximate; will be corrected by actual render area
            let target = current_idx.saturating_sub(max_rows / 2);
            let max_scroll = display
                .lyrics_lines
                .len()
                .saturating_sub(max_rows);
            display.lyrics_scroll = target.min(max_scroll);
        }
    }

    display
}

fn handle_notification(
    state: &mut AppState,
    info: &backend::NotificationInfo,
    picker: &Option<std::sync::Arc<Picker>>,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) {
    match info.player_state.as_str() {
        "Playing" => state.player_state = backend::PlayerState::Playing,
        "Paused" => state.player_state = backend::PlayerState::Paused,
        "Stopped" => {
            // Don't immediately clear — may be a transient transition.
            // Only update player state if there's no track name coming
            // (i.e. this is genuinely the end of playback, not a transition).
            if info.name.is_empty() {
                state.player_state = backend::PlayerState::Stopped;
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

        // Sync queue_selected if the new track matches a queue entry
        // (handles CLI next/prev while TUI is running)
        if is_new && !state.queue_tracks.is_empty() {
            if let Some(pos) = playlist::sync_queue_selection(
                &state.queue_tracks,
                &state.queue_playlist_name,
                &info.name,
                &info.artist,
            ) {
                state.queue_selected = pos;
                if pos < state.queue_scroll || pos >= state.queue_scroll + PAGE_SIZE {
                    state.queue_scroll = pos.saturating_sub(3);
                }
            }
        }

        // Fetch artwork for new track
        if is_new {
            let new_key = format!("{}\t{}", info.artist, info.album);
            if new_key != state.artwork_key {
                state.artwork_key = new_key.clone();
                // Keep old artwork visible until new one arrives
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

/// Returns true if the app should quit.
fn handle_key(
    key: KeyEvent,
    state: &mut AppState,
    theme: &mut Theme,
    tx: &mpsc::Sender<AppEvent>,
    backend: &Arc<dyn MusicBackend>,
) -> bool {
    // Help overlay
    if key.code == KeyCode::Char('?') {
        state.show_help = !state.show_help;
        return false;
    }
    if state.show_help {
        state.show_help = false;
        return false;
    }

    // Playlist picker
    if state.show_playlist_picker {
        handle_playlist_picker_key(key, state, tx, backend);
        return false;
    }

    let in_search = state.active_tab == Tab::Search;

    // Global keys
    match key.code {
        KeyCode::Char('q') if !in_search => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Tab => {
            if state.active_tab == Tab::Themes {
                restore_saved_theme(state, theme);
            }
            state.active_tab = state.active_tab.next();
            return false;
        }
        KeyCode::BackTab => {
            if state.active_tab == Tab::Themes {
                restore_saved_theme(state, theme);
            }
            state.active_tab = state.active_tab.prev();
            return false;
        }
        KeyCode::Char('l') if !in_search => {
            if state.active_tab == Tab::Themes {
                restore_saved_theme(state, theme);
            }
            state.active_tab = Tab::Library;
            return false;
        }
        KeyCode::Char('L') if !in_search => {
            if state.active_tab == Tab::Themes {
                restore_saved_theme(state, theme);
            }
            state.active_tab = Tab::Lyrics;
            return false;
        }
        KeyCode::Char('/') => {
            if state.active_tab == Tab::Themes {
                restore_saved_theme(state, theme);
            }
            state.active_tab = Tab::Search;
            return false;
        }
        KeyCode::Char(' ') if !in_search => {
            state.player_state = if state.player_state == backend::PlayerState::Playing {
                backend::PlayerState::Paused
            } else {
                backend::PlayerState::Playing
            };
            fire_and_refresh(backend, tx, |b| b.play_pause());
            return false;
        }
        KeyCode::Char('n') if !in_search => {
            fire_and_refresh(backend, tx, |b| b.next_track());
            return false;
        }
        KeyCode::Char('p') if !in_search => {
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
        KeyCode::Char('-') if !in_search => {
            state.volume = (state.volume - 5).max(0);
            let vol = state.volume;
            let b = backend.clone();
            std::thread::spawn(move || b.set_volume(vol));
            return false;
        }
        KeyCode::Char('s') if !in_search => {
            state.shuffle_enabled = !state.shuffle_enabled;
            fire_and_refresh(backend, tx, |b| b.toggle_shuffle());
            return false;
        }
        KeyCode::Char('r') if !in_search => {
            state.repeat_mode = match state.repeat_mode {
                backend::RepeatMode::Off => backend::RepeatMode::All,
                backend::RepeatMode::All => backend::RepeatMode::One,
                backend::RepeatMode::One => backend::RepeatMode::Off,
            };
            fire_and_refresh(backend, tx, |b| b.cycle_repeat());
            return false;
        }
        KeyCode::Char('C') if !in_search => {
            state.queue_tracks.clear();
            state.queue_selected = 0;
            state.queue_scroll = 0;
            state.queue_playlist_name.clear();
            playlist::clear_queue_state();
            return false;
        }
        KeyCode::Char('f') if !in_search && !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.current_track_favorited = !state.current_track_favorited;
            fire_and_refresh(backend, tx, |b| b.toggle_favorite());
            return false;
        }
        KeyCode::Char('P') if !in_search => {
            state.show_playlist_picker = !state.show_playlist_picker;
            state.playlist_picker_selected = 0;
            state.playlist_picker_scroll = 0;
            return false;
        }
        KeyCode::Char('a') if !in_search => {
            if let Some(artist) = state.track.as_ref().map(|t| t.artist.clone()) {
                if !artist.is_empty() {
                    state.active_tab = Tab::Search;
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
                    state.search_query = album;
                    state.search_selected = 0;
                    state.search_scroll = 0;
                    perform_search(state, tx, backend);
                }
            }
            return false;
        }
        KeyCode::Char('o') if !in_search => {
            if let Some(artist) = state.track.as_ref().map(|t| t.artist.clone()) {
                if !artist.is_empty() {
                    let b = backend.clone();
                    std::thread::spawn(move || b.reveal_artist(&artist));
                }
            }
            return false;
        }
        KeyCode::Char('O') if !in_search => {
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
        Tab::Themes => handle_themes_key(key, state, theme),
    }

    false
}

fn handle_queue_key(key: KeyEvent, state: &mut AppState, tx: &mpsc::Sender<AppEvent>, backend: &Arc<dyn MusicBackend>) {
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.queue_selected, state.queue_scroll, state.queue_tracks.len()) {
        state.queue_selected = sel;
        state.queue_scroll = scr;
        return;
    }
    match key.code {
        KeyCode::Enter => {
            if !state.queue_tracks.is_empty() && state.queue_selected < state.queue_tracks.len() {
                playlist::save_queue_state(&state.queue_playlist_name, state.queue_selected, state.queue_tracks.len());
                let playlist = state.queue_playlist_name.clone();
                let idx = state.queue_selected;
                fire_and_refresh(backend, tx, move |b| b.play_track_in_playlist(&playlist, idx));
            }
        }
        KeyCode::Char('d') | KeyCode::Char('x') => {
            if !state.queue_tracks.is_empty() && state.queue_selected < state.queue_tracks.len() {
                state.queue_tracks.remove(state.queue_selected);
                if state.queue_tracks.is_empty() {
                    state.queue_selected = 0;
                    state.queue_scroll = 0;
                    state.queue_playlist_name.clear();
                    playlist::clear_queue_state();
                } else {
                    if state.queue_selected >= state.queue_tracks.len() {
                        state.queue_selected = state.queue_tracks.len() - 1;
                    }
                    playlist::save_queue_state(&state.queue_playlist_name, state.queue_selected, state.queue_tracks.len());
                }
            }
        }
        _ => {}
    }
}

fn handle_library_key(key: KeyEvent, state: &mut AppState, tx: &mpsc::Sender<AppEvent>, backend: &Arc<dyn MusicBackend>) {
    match &state.library_sub_view {
        LibrarySubView::Playlists => {
            if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.library_selected, state.library_scroll, state.playlists.len()) {
                state.library_selected = sel;
                state.library_scroll = scr;
                return;
            }
            match key.code {
            KeyCode::Enter => {
                if !state.playlists.is_empty() && state.library_selected < state.playlists.len() {
                    let name = state.playlists[state.library_selected].clone();
                    state.library_sub_view = LibrarySubView::Tracks(name.clone());
                    state.playlist_tracks.clear();
                    state.playlist_tracks_selected = 0;
                    state.playlist_tracks_scroll = 0;
                    let tx2 = tx.clone();
                    let b = backend.clone();
                    std::thread::spawn(move || {
                        let tracks = b.get_playlist_tracks(&name);
                        let _ = tx2.send(AppEvent::PlaylistTracksLoaded(name, tracks));
                    });
                }
            }
            _ => {}
            }
        }
        LibrarySubView::Tracks(_) => {
            if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.playlist_tracks_selected, state.playlist_tracks_scroll, state.playlist_tracks.len()) {
                state.playlist_tracks_selected = sel;
                state.playlist_tracks_scroll = scr;
                return;
            }
            match key.code {
            KeyCode::Backspace => {
                state.library_sub_view = LibrarySubView::Playlists;
            }
            KeyCode::Enter => {
                if !state.playlist_tracks.is_empty()
                    && state.playlist_tracks_selected < state.playlist_tracks.len()
                {
                    if let LibrarySubView::Tracks(ref playlist_name) = state.library_sub_view {
                        let idx = state.playlist_tracks_selected;
                        state.queue_tracks = state.playlist_tracks.clone();
                        state.queue_playlist_name = playlist_name.clone();
                        state.queue_selected = idx;
                        if idx < state.queue_scroll || idx >= state.queue_scroll + PAGE_SIZE {
                            state.queue_scroll = idx.saturating_sub(3);
                        }
                        playlist::save_queue_state(playlist_name, idx, state.playlist_tracks.len());
                        let name = playlist_name.clone();
                        fire_and_refresh(backend, tx, move |b| {
                            b.play_track_in_playlist(&name, idx)
                        });
                    }
                }
            }
            KeyCode::Char('d') | KeyCode::Char('x') => {
                if !state.playlist_tracks.is_empty()
                    && state.playlist_tracks_selected < state.playlist_tracks.len()
                {
                    if let LibrarySubView::Tracks(ref playlist_name) = state.library_sub_view {
                        let idx = state.playlist_tracks_selected;
                        let name = playlist_name.clone();
                        let b = backend.clone();
                        std::thread::spawn(move || b.remove_from_playlist(&name, idx));
                        state.playlist_tracks.remove(idx);
                        if state.playlist_tracks_selected >= state.playlist_tracks.len()
                            && !state.playlist_tracks.is_empty()
                        {
                            state.playlist_tracks_selected = state.playlist_tracks.len() - 1;
                        }
                    }
                }
            }
            _ => {}
            }
        }
    }
}

fn handle_search_key(key: KeyEvent, state: &mut AppState, tx: &mpsc::Sender<AppEvent>, backend: &Arc<dyn MusicBackend>) {
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, false), state.search_selected, state.search_scroll, state.search_results.len()) {
        state.search_selected = sel;
        state.search_scroll = scr;
        return;
    }
    match key.code {
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
        KeyCode::Enter => {
            if !state.search_results.is_empty()
                && state.search_selected < state.search_results.len()
            {
                let result = state.search_results[state.search_selected].clone();
                fire_and_refresh(backend, tx, move |b| b.play_track(&result.name, &result.artist));
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
            // Scroll to a large value; rendering will clamp it
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

fn handle_themes_key(key: KeyEvent, state: &mut AppState, theme: &mut Theme) {
    if let Some((sel, scr)) = list_nav(normalize_nav_key(&key, true), state.theme_selected, state.theme_scroll, theme::ALL_THEMES.len()) {
        state.theme_selected = sel;
        state.theme_scroll = scr;
        preview_theme(state, theme);
        return;
    }
    match key.code {
        KeyCode::Enter => {
            if state.theme_selected < theme::ALL_THEMES.len() {
                let (name, t) = theme::ALL_THEMES[state.theme_selected];
                state.theme_name = name.to_string();
                *theme = t;
                save_theme(name);
            }
        }
        _ => {}
    }
}

fn handle_playlist_picker_key(key: KeyEvent, state: &mut AppState, _tx: &mpsc::Sender<AppEvent>, backend: &Arc<dyn MusicBackend>) {
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

fn preview_theme(state: &AppState, theme: &mut Theme) {
    if state.theme_selected < theme::ALL_THEMES.len() {
        *theme = theme::ALL_THEMES[state.theme_selected].1;
    }
}

fn restore_saved_theme(state: &AppState, theme: &mut Theme) {
    if let Some((_, t)) = theme::find_theme(&state.theme_name) {
        *theme = t;
    }
}

fn perform_search(state: &AppState, tx: &mpsc::Sender<AppEvent>, backend: &Arc<dyn MusicBackend>) {
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

fn fetch_artwork(picker: &Picker, backend: &dyn MusicBackend) -> Option<ratatui_image::protocol::StatefulProtocol> {
    // Retry a few times — backend calls can fail intermittently
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

fn fire_and_refresh<F: FnOnce(&dyn MusicBackend) + Send + 'static>(
    backend: &Arc<dyn MusicBackend>,
    tx: &mpsc::Sender<AppEvent>,
    action: F,
) {
    let tx2 = tx.clone();
    let b = backend.clone();
    std::thread::spawn(move || {
        action(&*b);
        // Give the music service time to update before fetching state.
        std::thread::sleep(Duration::from_millis(500));
        let fresh = b.fetch_state();
        let _ = tx2.send(AppEvent::StateRefreshed(fresh));
    });
}

// Config

fn config_dir() -> std::path::PathBuf {
    dirs_or_home().join(".config").join("muse")
}

fn config_file() -> std::path::PathBuf {
    config_dir().join("config")
}

fn dirs_or_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
}

fn load_config(state: &mut AppState, theme: &mut Theme) {
    let path = config_file();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    for line in contents.lines() {
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() == 2 && parts[0].trim() == "theme" {
            let name = parts[1].trim();
            if let Some((idx, t)) = theme::find_theme(name) {
                state.theme_name = name.to_string();
                state.theme_selected = idx;
                *theme = t;
            }
        }
    }
}

fn load_backend_config() -> Option<String> {
    let contents = std::fs::read_to_string(config_file()).ok()?;
    for line in contents.lines() {
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() == 2 && parts[0].trim() == "backend" {
            return Some(parts[1].trim().to_string());
        }
    }
    None
}

#[cfg(feature = "spotify")]
fn load_spotify_client_id() -> Option<String> {
    let contents = std::fs::read_to_string(config_file()).ok()?;
    for line in contents.lines() {
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() == 2 && parts[0].trim() == "spotify_client_id" {
            let val = parts[1].trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

fn save_theme(name: &str) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);

    // Preserve existing config lines, only update theme
    let path = config_file();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| {
            let parts: Vec<&str> = l.splitn(2, '=').collect();
            parts.len() != 2 || parts[0].trim() != "theme"
        })
        .map(|l| l.to_string())
        .collect();
    lines.push(format!("theme={}", name));
    let _ = std::fs::write(path, lines.join("\n") + "\n");
}
