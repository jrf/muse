mod bridge;
mod state;
mod theme;
mod ui;

use std::ffi::c_void;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::ImageReader;
use ratatui::prelude::*;
use ratatui_image::picker::Picker;

use state::{AppState, LibrarySubView, Tab};
use theme::Theme;

/// Events sent to the main loop.
enum AppEvent {
    Key(KeyEvent),
    Tick,
    MusicNotification(bridge::NotificationInfo),
    StateRefreshed(bridge::FullState),
    PlaylistsLoaded(Vec<String>),
    PlaylistTracksLoaded(Vec<bridge::PlaylistTrack>),
    SearchResults(String, Vec<bridge::SearchResult>),
    LyricsLoaded(String, Option<bridge::LyricsResult>),
    ArtworkLoaded(String, ratatui_image::protocol::StatefulProtocol),
    AutoAdvance(u64),
}

/// Monotonic token used to cancel stale auto-advance requests.
static AUTO_ADVANCE_TOKEN: AtomicU64 = AtomicU64::new(0);

fn main() -> io::Result<()> {
    // Ensure Music.app is running and fetch initial state BEFORE entering raw mode.
    // NSAppleScript can behave unreliably when terminal is in raw mode.
    bridge::ensure_running();
    let initial_state = bridge::fetch_state();
    let initial_playlists = bridge::get_playlists();

    // Detect image protocol before entering raw mode (queries terminal)
    // Falls back to halfblocks (unicode) if terminal doesn't support Sixel/Kitty/iTerm2
    let picker = Some(
        Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks()),
    );

    // Fetch initial artwork before raw mode (NSAppleScript is more reliable here)
    let initial_artwork = if let (Some(_), Some(ref picker)) = (&initial_state.track, &picker) {
        fetch_artwork(picker)
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
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(
        &mut terminal,
        picker,
        initial_state,
        initial_playlists,
        initial_artwork,
        initial_artwork_key,
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
    initial_state: bridge::FullState,
    initial_playlists: Vec<String>,
    initial_artwork: Option<ratatui_image::protocol::StatefulProtocol>,
    initial_artwork_key: String,
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
    apply_fresh_state(&mut state, &initial_state, &picker, &tx);
    state.playlists = initial_playlists;
    let mut last_position_update = Instant::now();

    // Load config
    load_config(&mut state, &mut current_theme);

    // Register notification callback — sends events to the channel
    {
        let tx_notify = tx.clone();
        NOTIFICATION_TX.lock().unwrap().replace(tx_notify);
        bridge::register_notification_callback(notification_callback);
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
                    if handle_key(key, &mut state, &mut current_theme, &tx) {
                        return Ok(());
                    }
                }
                AppEvent::Tick => {
                    // Pump the RunLoop on the main thread for notifications
                    bridge::pump_runloop();

                    // Periodic state refresh
                    if last_refresh.elapsed() >= refresh_interval {
                        last_refresh = Instant::now();
                        let tx2 = tx.clone();
                        std::thread::spawn(move || {
                            let fresh = bridge::fetch_state();
                            let _ = tx2.send(AppEvent::StateRefreshed(fresh));
                        });
                    }
                }
                AppEvent::MusicNotification(info) => {
                    handle_notification(&mut state, &info, &picker, &tx);
                    last_position_update = Instant::now();

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
                            std::thread::spawn(move || {
                                let result = bridge::get_lyrics(&name, &artist);
                                let _ = tx2.send(AppEvent::LyricsLoaded(lyrics_key, result));
                            });
                        }
                    }
                }
                AppEvent::StateRefreshed(fresh) => {
                    let was_not_running = !state.music_running;
                    apply_fresh_state(&mut state, &fresh, &picker, &tx);
                    last_position_update = Instant::now();

                    // When Music.app transitions to running, load playlists
                    if was_not_running && state.music_running && state.playlists.is_empty() {
                        let tx2 = tx.clone();
                        std::thread::spawn(move || {
                            let playlists = bridge::get_playlists();
                            let _ = tx2.send(AppEvent::PlaylistsLoaded(playlists));
                        });
                    }
                }
                AppEvent::PlaylistsLoaded(playlists) => {
                    state.playlists = playlists;
                }
                AppEvent::PlaylistTracksLoaded(tracks) => {
                    state.playlist_tracks = tracks;
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
                AppEvent::AutoAdvance(token) => {
                    // Only advance if the token is still current (not cancelled by a "Playing" event)
                    if token == AUTO_ADVANCE_TOKEN.load(Ordering::SeqCst)
                        && !state.queue_tracks.is_empty()
                        && state.queue_selected + 1 < state.queue_tracks.len()
                    {
                        let next_idx = state.queue_selected + 1;
                        state.queue_selected = next_idx;
                        state.queue_scroll = next_idx.saturating_sub(3);
                        let playlist = state.queue_playlist_name.clone();
                        fire_and_refresh(&tx, move || {
                            bridge::play_track_in_playlist(&playlist, next_idx as i32)
                        });
                    }
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

// Global sender for the C notification callback
use std::sync::Mutex;
static NOTIFICATION_TX: Mutex<Option<mpsc::Sender<AppEvent>>> = Mutex::new(None);

extern "C" fn notification_callback(ptr: *mut c_void) {
    let parsed = bridge::parse_notification(ptr);
    if let Some(tx) = NOTIFICATION_TX.lock().unwrap().as_ref() {
        let _ = tx.send(AppEvent::MusicNotification(parsed));
    }
}

fn apply_fresh_state(
    state: &mut AppState,
    fresh: &bridge::FullState,
    picker: &Option<std::sync::Arc<Picker>>,
    tx: &mpsc::Sender<AppEvent>,
) {
    let old_art_key = state.artwork_key.clone();

    // Don't flip to "not running" if we were previously running — could be a
    // transient AppleScript error during track transitions.  Only mark not
    // running if we also had no track before (i.e. we never connected).
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
            std::thread::spawn(move || {
                if let Some(proto) = fetch_artwork(&picker) {
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
    };

    // Interpolate position when playing
    if state.player_state == bridge::PlayerState::Playing {
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
    info: &bridge::NotificationInfo,
    picker: &Option<std::sync::Arc<Picker>>,
    tx: &mpsc::Sender<AppEvent>,
) {
    match info.player_state.as_str() {
        "Playing" => {
            state.player_state = bridge::PlayerState::Playing;
            // Cancel any pending auto-advance
            AUTO_ADVANCE_TOKEN.fetch_add(1, Ordering::SeqCst);
        }
        "Paused" => state.player_state = bridge::PlayerState::Paused,
        "Stopped" => {
            // Don't immediately clear — may be a transient transition.
            // Only update player state if there's no track name coming
            // (i.e. this is genuinely the end of playback, not a transition).
            if info.name.is_empty() {
                state.player_state = bridge::PlayerState::Stopped;
                // Schedule auto-advance if there are more tracks in the queue
                if !state.queue_tracks.is_empty()
                    && state.queue_selected + 1 < state.queue_tracks.len()
                {
                    let token = AUTO_ADVANCE_TOKEN.load(Ordering::SeqCst);
                    let tx2 = tx.clone();
                    std::thread::spawn(move || {
                        // Debounce: wait before advancing to avoid cascading from
                        // transient "Stopped" states during track transitions.
                        std::thread::sleep(Duration::from_millis(1500));
                        let _ = tx2.send(AppEvent::AutoAdvance(token));
                    });
                }
            }
        }
        _ => {}
    }

    if !info.name.is_empty() {
        let is_new = state
            .track
            .as_ref()
            .map_or(true, |t| t.name != info.name || t.artist != info.artist);

        state.track = Some(bridge::Track {
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

        // Fetch artwork for new track
        if is_new {
            let new_key = format!("{}\t{}", info.artist, info.album);
            if new_key != state.artwork_key {
                state.artwork_key = new_key.clone();
                // Keep old artwork visible until new one arrives
                if let Some(ref picker) = picker {
                    let tx2 = tx.clone();
                    let picker = picker.clone();
                    std::thread::spawn(move || {
                        if let Some(proto) = fetch_artwork(&picker) {
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
        handle_playlist_picker_key(key, state, tx);
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
            state.player_state = if state.player_state == bridge::PlayerState::Playing {
                bridge::PlayerState::Paused
            } else {
                bridge::PlayerState::Playing
            };
            fire_and_refresh(tx, || bridge::play_pause());
            return false;
        }
        KeyCode::Char('n') if !in_search => {
            fire_and_refresh(tx, || bridge::next_track());
            return false;
        }
        KeyCode::Char('p') if !in_search => {
            fire_and_refresh(tx, || bridge::previous_track());
            return false;
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            state.volume = (state.volume + 5).min(100);
            let vol = state.volume;
            std::thread::spawn(move || bridge::set_volume(vol));
            return false;
        }
        KeyCode::Char('-') if !in_search => {
            state.volume = (state.volume - 5).max(0);
            let vol = state.volume;
            std::thread::spawn(move || bridge::set_volume(vol));
            return false;
        }
        KeyCode::Char('s') if !in_search => {
            state.shuffle_enabled = !state.shuffle_enabled;
            fire_and_refresh(tx, || bridge::toggle_shuffle());
            return false;
        }
        KeyCode::Char('r') if !in_search => {
            state.repeat_mode = match state.repeat_mode {
                bridge::RepeatMode::Off => bridge::RepeatMode::All,
                bridge::RepeatMode::All => bridge::RepeatMode::One,
                bridge::RepeatMode::One => bridge::RepeatMode::Off,
            };
            fire_and_refresh(tx, || bridge::cycle_repeat());
            return false;
        }
        KeyCode::Char('C') if !in_search => {
            state.queue_tracks.clear();
            state.queue_selected = 0;
            state.queue_scroll = 0;
            state.queue_playlist_name.clear();
            return false;
        }
        KeyCode::Char('f') if !in_search => {
            state.current_track_favorited = !state.current_track_favorited;
            fire_and_refresh(tx, || bridge::toggle_favorite());
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
                    perform_search(state, tx);
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
                    perform_search(state, tx);
                }
            }
            return false;
        }
        KeyCode::Char('o') if !in_search => {
            if let Some(artist) = state.track.as_ref().map(|t| t.artist.clone()) {
                if !artist.is_empty() {
                    std::thread::spawn(move || bridge::reveal_artist(&artist));
                }
            }
            return false;
        }
        KeyCode::Char('O') if !in_search => {
            if let Some(track) = state.track.clone() {
                if !track.album.is_empty() {
                    std::thread::spawn(move || {
                        bridge::reveal_album(&track.album, &track.artist)
                    });
                }
            }
            return false;
        }
        _ => {}
    }

    // Tab-specific keys
    match state.active_tab {
        Tab::Queue => handle_queue_key(key, state, tx),
        Tab::Library => handle_library_key(key, state, tx),
        Tab::Search => handle_search_key(key, state, tx),
        Tab::Lyrics => handle_lyrics_key(key, state),
        Tab::Themes => handle_themes_key(key, state, theme),
    }

    false
}

fn handle_queue_key(key: KeyEvent, state: &mut AppState, tx: &mpsc::Sender<AppEvent>) {
    match key.code {
        KeyCode::Up => {
            if state.queue_selected > 0 {
                state.queue_selected -= 1;
                if state.queue_selected < state.queue_scroll {
                    state.queue_scroll = state.queue_selected;
                }
            }
        }
        KeyCode::Down => {
            if state.queue_selected + 1 < state.queue_tracks.len() {
                state.queue_selected += 1;
                // Approximate visible rows
                if state.queue_selected >= state.queue_scroll + 20 {
                    state.queue_scroll = state.queue_selected - 19;
                }
            }
        }
        KeyCode::Enter => {
            if !state.queue_tracks.is_empty() && state.queue_selected < state.queue_tracks.len() {
                let playlist = state.queue_playlist_name.clone();
                let idx = state.queue_selected as i32;
                fire_and_refresh(tx, move || bridge::play_track_in_playlist(&playlist, idx));
            }
        }
        _ => {}
    }
}

fn handle_library_key(key: KeyEvent, state: &mut AppState, tx: &mpsc::Sender<AppEvent>) {
    match &state.library_sub_view {
        LibrarySubView::Playlists => match key.code {
            KeyCode::Up => {
                if state.library_selected > 0 {
                    state.library_selected -= 1;
                    if state.library_selected < state.library_scroll {
                        state.library_scroll = state.library_selected;
                    }
                }
            }
            KeyCode::Down => {
                if state.library_selected + 1 < state.playlists.len() {
                    state.library_selected += 1;
                    if state.library_selected >= state.library_scroll + 20 {
                        state.library_scroll = state.library_selected - 19;
                    }
                }
            }
            KeyCode::Enter => {
                if !state.playlists.is_empty() && state.library_selected < state.playlists.len() {
                    let name = state.playlists[state.library_selected].clone();
                    state.library_sub_view = LibrarySubView::Tracks(name.clone());
                    state.playlist_tracks.clear();
                    state.playlist_tracks_selected = 0;
                    state.playlist_tracks_scroll = 0;
                    let tx2 = tx.clone();
                    std::thread::spawn(move || {
                        let tracks = bridge::get_playlist_tracks(&name);
                        let _ = tx2.send(AppEvent::PlaylistTracksLoaded(tracks));
                    });
                }
            }
            _ => {}
        },
        LibrarySubView::Tracks(_) => match key.code {
            KeyCode::Backspace => {
                state.library_sub_view = LibrarySubView::Playlists;
            }
            KeyCode::Up => {
                if state.playlist_tracks_selected > 0 {
                    state.playlist_tracks_selected -= 1;
                    if state.playlist_tracks_selected < state.playlist_tracks_scroll {
                        state.playlist_tracks_scroll = state.playlist_tracks_selected;
                    }
                }
            }
            KeyCode::Down => {
                if state.playlist_tracks_selected + 1 < state.playlist_tracks.len() {
                    state.playlist_tracks_selected += 1;
                    if state.playlist_tracks_selected >= state.playlist_tracks_scroll + 19 {
                        state.playlist_tracks_scroll = state.playlist_tracks_selected - 18;
                    }
                }
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
                        state.queue_scroll = idx.saturating_sub(3);
                        let name = playlist_name.clone();
                        fire_and_refresh(tx, move || {
                            bridge::play_track_in_playlist(&name, idx as i32)
                        });
                    }
                }
            }
            _ => {}
        },
    }
}

fn handle_search_key(key: KeyEvent, state: &mut AppState, tx: &mpsc::Sender<AppEvent>) {
    match key.code {
        KeyCode::Backspace => {
            if !state.search_query.is_empty() {
                state.search_query.pop();
                perform_search(state, tx);
            } else {
                state.search_results.clear();
                state.search_selected = 0;
                state.search_scroll = 0;
            }
        }
        KeyCode::Up => {
            if state.search_selected > 0 {
                state.search_selected -= 1;
                if state.search_selected < state.search_scroll {
                    state.search_scroll = state.search_selected;
                }
            }
        }
        KeyCode::Down => {
            if state.search_selected + 1 < state.search_results.len() {
                state.search_selected += 1;
                if state.search_selected >= state.search_scroll + 19 {
                    state.search_scroll = state.search_selected - 18;
                }
            }
        }
        KeyCode::Enter => {
            if !state.search_results.is_empty()
                && state.search_selected < state.search_results.len()
            {
                let result = state.search_results[state.search_selected].clone();
                fire_and_refresh(tx, move || bridge::play_track(&result.name, &result.artist));
            }
        }
        KeyCode::Char(ch) => {
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                state.search_query.push(ch);
                state.search_selected = 0;
                state.search_scroll = 0;
                perform_search(state, tx);
            }
        }
        _ => {}
    }
}

fn handle_lyrics_key(key: KeyEvent, state: &mut AppState) {
    match key.code {
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
        KeyCode::Char('0') => {
            state.lyrics_manual_scroll = false;
        }
        _ => {}
    }
}

fn handle_themes_key(key: KeyEvent, state: &mut AppState, theme: &mut Theme) {
    match key.code {
        KeyCode::Up => {
            if state.theme_selected > 0 {
                state.theme_selected -= 1;
                if state.theme_selected < state.theme_scroll {
                    state.theme_scroll = state.theme_selected;
                }
                preview_theme(state, theme);
            }
        }
        KeyCode::Down => {
            if state.theme_selected + 1 < theme::ALL_THEMES.len() {
                state.theme_selected += 1;
                if state.theme_selected >= state.theme_scroll + 20 {
                    state.theme_scroll = state.theme_selected - 19;
                }
                preview_theme(state, theme);
            }
        }
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

fn handle_playlist_picker_key(key: KeyEvent, state: &mut AppState, _tx: &mpsc::Sender<AppEvent>) {
    match key.code {
        KeyCode::Up => {
            if state.playlist_picker_selected > 0 {
                state.playlist_picker_selected -= 1;
                if state.playlist_picker_selected < state.playlist_picker_scroll {
                    state.playlist_picker_scroll = state.playlist_picker_selected;
                }
            }
        }
        KeyCode::Down => {
            if state.playlist_picker_selected + 1 < state.playlists.len() {
                state.playlist_picker_selected += 1;
                if state.playlist_picker_selected >= state.playlist_picker_scroll + 20 {
                    state.playlist_picker_scroll = state.playlist_picker_selected - 19;
                }
            }
        }
        KeyCode::Enter => {
            if !state.playlists.is_empty()
                && state.playlist_picker_selected < state.playlists.len()
            {
                let name = state.playlists[state.playlist_picker_selected].clone();
                state.show_playlist_picker = false;
                std::thread::spawn(move || bridge::add_to_playlist(&name));
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

fn perform_search(state: &AppState, tx: &mpsc::Sender<AppEvent>) {
    let query = state.search_query.clone();
    if query.len() < 2 {
        return;
    }
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        let results = bridge::search(&query);
        let _ = tx2.send(AppEvent::SearchResults(query, results));
    });
}

fn fetch_artwork(picker: &Picker) -> Option<ratatui_image::protocol::StatefulProtocol> {
    // Retry a few times — NSAppleScript can fail intermittently from background threads
    for _ in 0..3 {
        if let Some(data) = bridge::get_artwork_data() {
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

fn fire_and_refresh<F: FnOnce() + Send + 'static>(tx: &mpsc::Sender<AppEvent>, action: F) {
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        action();
        // Give Music.app time to start the new track before fetching state.
        // Without this delay, fetch_state returns stale/empty data during transitions.
        std::thread::sleep(Duration::from_millis(500));
        let fresh = bridge::fetch_state();
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

fn save_theme(name: &str) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(config_file(), format!("theme={}\n", name));
}
