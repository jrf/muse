# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Muse is a terminal UI for controlling music playback on macOS. It supports Apple Music (via Swift FFI + AppleScript) and Spotify (via Web API). Rust (ratatui) handles the TUI frontend; a `MusicBackend` trait abstracts the music service.

## Build Commands

```bash
just          # List all available recipes
just install  # Build release + install to ~/.local/bin + codesign
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

- **`main.rs`** — Entry point, terminal setup, `run_app()` event loop with `mpsc` channel, CLI subcommands (`muse next`, `muse prev`, etc.), and config loading. Position interpolation is computed at render time inside `ui::draw` using an `elapsed` parameter (no full AppState clone).
- **`handlers.rs`** — Event handlers (keyboard and mouse handlers, `apply_fresh_state`, `handle_notification`), the `AppEvent` enum, navigation/filter helpers, and the `fire_and_refresh` / `perform_search` thread spawners. Left-click activation selects a UI hit target and delegates to the existing Enter-key path.
- **`ui.rs`** — Ratatui rendering. Takes `&mut AppState` so it can temporarily move `player.artwork` out, apply lyric auto-scroll, and render without cloning. Centered 120-column layout with player section, tab bar, and tab content area (Queue, Library, Search, Lyrics, Themes). `draw` returns an `Option<KittyPlacement>` so the main loop can emit hand-rolled Kitty graphics after the frame flushes.
- **`artwork.rs`** — Album-art rendering abstraction. `ArtCtx` holds the ratatui-image `Picker` plus a `kitty` flag. `Artwork` is an enum: `Ratatui(StatefulProtocol)` for Sixel/iTerm2/halfblocks (drawn as a widget) or `Kitty(KittyArt)` for the hand-rolled **direct-placement** (`a=T`) Kitty path. The Kitty path decodes artwork to a base64 PNG once, computes an aspect-correct cell box (`KittyArt::fit`), and emits chunked `\x1b_G...` escapes. Inline RFC 4648 base64 (no dep). This exists because ratatui-image's Kitty backend uses Unicode placeholders (`U=1`, `\u{10EEEE}`) that WezTerm doesn't render; direct placement (like `rview`) works there.
- **`state.rs`** — `AppState` decomposed into per-concern sub-structs: `PlayerData`, `QueueData`, `LibraryData`, `SearchData`, `LyricsData`, `ThemeData`, `OverlayData`, `FilterData`. Top-level fields are config (`ui_width`, `show_artwork`, `lyrics_enabled`), active tab, and `lastfm_status`.
- **`playlist.rs`** — Apple Music-specific queue state persistence and CLI playlist-aware next/prev. Not used by Spotify backend.
- **`lastfm.rs`** — Last.fm scrobbling via external `muse-scrobble` CLI. `ScrobbleTracker` tracks play timing in-process (50% or 4min threshold).
- **`theme.rs`** — Loads the selected theme and explicit picker catalog from configured file paths. Shared theme files define named `[colors]` and semantic `[ui]` roles. Muse consumes background, cursor_bg, border, accent, selection, key, text variants, heading, and error; values may be 256-color indices or hex RGB colors.

### Legacy Swift Frontend (`Sources/`)

The original pure-Swift TUI (direct ANSI rendering). Not used when building with Cargo.

## Key Patterns

- **Backend abstraction**: `Arc<dyn MusicBackend>` passed through `run_app` → handlers. All `bridge::*` calls replaced with `backend.*` method calls. Background threads clone the `Arc`.
- **Async event model**: Background threads and the crossterm input thread send results through the `AppEvent` enum (`Key`, `Mouse`, backend results, and ticks). The main loop renders on each event or 50ms timeout. Mouse capture is enabled only while the alternate-screen TUI is active and disabled during terminal restoration.
- **Notification bridge**: Backend sends `NotificationInfo` through a channel. Apple Music: C callback from `NSDistributedNotificationCenter`. Spotify: polling thread every 2s.
- **Position interpolation**: Base position from periodic polls; `ui::draw` adds elapsed wall-clock time to `track.position` at render time so playback progresses smoothly between 2-second refreshes. No state clone.
- **Transient resilience**: `apply_fresh_state()` keeps the previous track visible during backend failures that occur during track transitions.
- **`fire_and_refresh`**: Spawns a thread to execute an action, then polls `fetch_state` at 500ms / 1300ms / 2100ms. Multiple polls absorb Web API lag (Spotify); each refresh is applied idempotently.
- **Shutdown signal**: `Arc<AtomicBool>` shared with backend polling threads and the input/tick threads. Flipped before `run_app` returns so polling loops exit promptly rather than sleeping out a 2-second tick.
- **Artwork rendering (dual path)**: The protocol is chosen once at startup (`ArtCtx.kitty`). Non-Kitty protocols render as ratatui-image widgets inside `terminal.draw`. Kitty uses a hand-rolled direct-placement path: `ui::draw` reserves the region, marks those cells `set_skip(true)` so ratatui's diff won't overwrite them, and returns a `KittyPlacement`; `main::emit_kitty_artwork` then emits the transmit/delete escapes after the frame flushes. Retransmission is gated by an `artwork_gen` counter + placement rect, so the PNG is only sent when the image or its position changes. The image is deleted (`a=d,d=A`) on track change without art and on exit.
- **Error overlay**: Runtime failures (e.g. terminal image-protocol detection) are surfaced via `overlays.error_message` as a centered TUI overlay dismissible by any key, instead of `eprintln!` corrupting the alternate screen.

## Configuration

File: `~/.config/muse/config.toml` (TOML):

```toml
backend = "spotify"              # optional, default is "apple_music"
theme = "synthwave"              # color theme (filename from ~/.config/muse/themes/)
default_tab = "queue"            # tab shown on launch: queue, library, search, lyrics
ui_width = 120                   # max UI width in columns (min 40, or "auto")
show_artwork = true              # display album art
image_protocol = "auto"          # auto | kitty | sixel | iterm2 | halfblocks
lyrics_enabled = true            # fetch synced lyrics from LRCLIB

[spotify]
client_id = "YOUR_ID"            # required if backend = "spotify"
```

Legacy plain-text `KEY=VALUE` configs (old `config` file) are auto-migrated to TOML on first read.

Themes are loaded from the path in `theme` and the explicit paths listed by `theme_catalog`; muse does not scan a directory. Theme TOML uses named `[colors]` and semantic `[ui]` roles: background, cursor_bg, border, accent, selection, key, text, text_bright, text_dim, text_muted, heading, and error. Values are 256-color indices, hex RGB strings (`"#82aaff"`), or named-color references.

## Workflow

- **TODO.md must be kept meticulously updated.** When completing a feature or fix, add it to the Done section. When discovering new work, add it to the appropriate section (Now/Next/Later).
- **README.md must be kept meticulously updated.** When adding or changing features, keybindings, configuration options, or setup instructions, update the README to match.
- **CLAUDE.md must be kept meticulously updated.** When changing architecture, adding modules, altering key patterns, or modifying build/config behavior, update the relevant sections.

## macOS Requirements

- macOS 13+, Apple Music app must be running (for Apple Music backend)
- Binary must be codesigned (even ad-hoc) to use AppleScript (`codesign -s -`)
- Album art uses sixel graphics when terminal supports it
