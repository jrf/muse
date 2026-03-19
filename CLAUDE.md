# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Muse is a terminal UI for controlling music playback on macOS. It supports Apple Music (via Swift FFI + AppleScript) and Spotify (via Web API). Rust (ratatui) handles the TUI frontend; a `MusicBackend` trait abstracts the music service.

## Build Commands

```bash
just          # Build release + install to ~/.local/bin + codesign
just build    # Debug build
just run      # Run debug build
just release  # Release build only
just clean    # Remove build artifacts
```

### Feature Flags

Both backends are enabled by default. Build with only one:

```bash
cargo build --no-default-features --features spotify      # Spotify only (no Swift/Xcode needed)
cargo build --no-default-features --features apple-music   # Apple Music only (no ureq/serde)
```

The build process (defined in `build.rs`) compiles `swift-bridge/MusicBridge.swift` into a static library only when the `apple-music` feature is enabled **and** the target is macOS. On non-macOS platforms, the Apple Music backend is automatically skipped even if the feature is enabled — `cargo build` just works on any OS. The `spotify` feature gates the `ureq`, `serde`, and `serde_json` dependencies.

## Architecture

### Backend Trait (`src/backend.rs`)

Defines `MusicBackend` trait and shared types (`Track`, `PlayerState`, `RepeatMode`, `FullState`, `PlaylistTrack`, `SearchResult`, `LyricsLine`, `NotificationInfo`). All backends implement this trait. The main loop uses `Arc<dyn MusicBackend>` for runtime backend selection. Backend-specific behavior is expressed through trait methods (e.g. `needs_queue_advance()`), not name checks in the frontend.

### Apple Music Backend (`src/bridge.rs`)

`AppleMusicBackend` wraps ~40 `extern "C"` FFI functions from the Swift bridge. Uses opaque pointer pattern (`*mut c_void`) with accessor functions for each field, plus explicit `_free()` calls for memory management. Notifications arrive via `NSDistributedNotificationCenter` → C callback → channel.

### Spotify Backend (`src/spotify.rs`)

`SpotifyBackend` uses the Spotify Web API via `ureq` (sync HTTP). Includes OAuth PKCE auth flow (browser → localhost:18234 callback), token caching at `~/.config/muse/spotify_token.json` with auto-refresh, and a polling thread for state change detection. Lyrics come from LRCLIB API. Inline SHA-256 + base64url for PKCE (no crypto deps).

### Swift Bridge (`swift-bridge/MusicBridge.swift`)

Single file exporting ~40 `@_cdecl` functions. Controls Music.app via AppleScript (`NSAppleScript`), extracts artwork via CoreGraphics, fetches synced lyrics from LRCLIB API with embedded lyrics fallback. All complex types use opaque pointer pattern (allocate with `Unmanaged.passRetained`, free via exported `_free` functions).

### Other Modules

- **`main.rs`** — Event loop with `mpsc` channel. Spawns background threads for expensive operations. `interpolated_state()` smooths playback position between refreshes. Backend selection via config. CLI subcommands (`muse next`, `muse prev`, etc.).
- **`ui.rs`** — Ratatui rendering. Centered 120-column layout with player section, tab bar, and tab content area (Queue, Library, Search, Lyrics, Themes).
- **`state.rs`** — `AppState` struct with all UI state: selected indices, scroll positions, loaded data, active tab.
- **`playlist.rs`** — Apple Music-specific queue state persistence and CLI playlist-aware next/prev. Not used by Spotify backend.
- **`lastfm.rs`** — Last.fm scrobbling via external `muse-scrobble` CLI. `ScrobbleTracker` tracks play timing in-process (50% or 4min threshold).
- **`theme.rs`** — Six color themes using 256-color indexed palette.

### Legacy Swift Frontend (`Sources/`)

The original pure-Swift TUI (direct ANSI rendering). Not used when building with Cargo.

## Key Patterns

- **Backend abstraction**: `Arc<dyn MusicBackend>` passed through `run_app` → handlers. All `bridge::*` calls replaced with `backend.*` method calls. Background threads clone the `Arc`.
- **Async event model**: Background threads send results via `AppEvent` enum through `mpsc::Sender`. The main loop renders on each event or 50ms timeout.
- **Notification bridge**: Backend sends `NotificationInfo` through a channel. Apple Music: C callback from `NSDistributedNotificationCenter`. Spotify: polling thread every 2s.
- **Position interpolation**: Base position from periodic polls; `interpolated_state()` adds elapsed wall-clock time for smooth progress display.
- **Transient resilience**: `apply_fresh_state()` keeps the previous track visible during backend failures that occur during track transitions.
- **`fire_and_refresh`**: Spawns a thread to execute an action, waits 500ms for the service to update, then fetches fresh state. Used for all playback control actions.

## Configuration

File: `~/.config/muse/config` (plain text `KEY=VALUE`):

```
backend=spotify              # optional, default is apple_music
spotify_client_id=YOUR_ID    # required if backend=spotify
theme=synthwave              # color theme
```

## Workflow

- **TODO.md must be kept meticulously updated.** When completing a feature or fix, add it to the Done section. When discovering new work, add it to the appropriate section (Now/Next/Later).
- **README.md must be kept meticulously updated.** When adding or changing features, keybindings, configuration options, or setup instructions, update the README to match.
- **CLAUDE.md must be kept meticulously updated.** When changing architecture, adding modules, altering key patterns, or modifying build/config behavior, update the relevant sections.

## macOS Requirements

- macOS 13+, Apple Music app must be running (for Apple Music backend)
- Binary must be codesigned (even ad-hoc) to use AppleScript (`codesign -s -`)
- Album art uses sixel graphics when terminal supports it
