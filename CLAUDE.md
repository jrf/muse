# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Muse is a terminal UI for controlling Apple Music on macOS. It uses a hybrid Rust + Swift architecture: Rust (ratatui) handles the TUI frontend, Swift handles Apple Music interaction via AppleScript and macOS frameworks.

## Build Commands

```bash
just          # Build release + install to ~/.local/bin + codesign
just build    # Debug build
just run      # Run debug build
just release  # Release build only
just clean    # Remove build artifacts
```

The build process (defined in `build.rs`) automatically compiles `swift-bridge/MusicBridge.swift` into a static library (`libmusic_bridge.a`) before Rust compilation. Requires Swift 6.2+ and Xcode command line tools.

## Architecture

### Rust Frontend (`src/`)

- **`main.rs`** — Event loop with `mpsc` channel. Spawns background threads for expensive operations (playlist loading, search, lyrics fetch, artwork). Handles key input, state refresh (every 2s via AppleScript), and Music.app notifications. `interpolated_state()` smooths playback position between refreshes.
- **`bridge.rs`** — Safe Rust wrappers around `extern "C"` FFI functions. Uses opaque pointer pattern (`*mut c_void`) with accessor functions for each field, plus explicit `_free()` calls for memory management.
- **`ui.rs`** — Ratatui rendering. Centered 80-column layout with player section, tab bar, and tab content area (Queue, Library, Search, Lyrics, Themes).
- **`state.rs`** — `AppState` struct with all UI state: selected indices, scroll positions, loaded data, active tab.
- **`playlist.rs`** — Apple Music-specific playlist sync logic: queue state persistence, CLI playlist-aware next/prev, queue selection sync. This module is intentionally separated because it will not be needed when Spotify support is added (Spotify handles playlists/queuing natively via its API).
- **`lastfm.rs`** — Last.fm scrobbling via external `muse-scrobble` CLI. Tracks play timing in-process.
- **`theme.rs`** — Six color themes using 256-color indexed palette.

### Swift Bridge (`swift-bridge/MusicBridge.swift`)

Single file exporting ~40 `@_cdecl` functions. Controls Music.app via AppleScript (`NSAppleScript`), extracts artwork via CoreGraphics, fetches synced lyrics from LRCLIB API with embedded lyrics fallback. All complex types use opaque pointer pattern (allocate with `Unmanaged.passRetained`, free via exported `_free` functions).

### Legacy Swift Frontend (`Sources/`)

The original pure-Swift TUI (direct ANSI rendering). Still present on `main` branch. Not used when building with Cargo.

## Key Patterns

- **Async event model**: Background threads send results via `AppEvent` enum through `mpsc::Sender`. The main loop renders on each event or 50ms timeout.
- **Notification bridge**: Music.app state changes arrive via `NSDistributedNotificationCenter` → C callback → global `NOTIFICATION_TX` mutex → event channel.
- **Position interpolation**: Base position from periodic AppleScript polls; `interpolated_state()` adds elapsed wall-clock time for smooth progress display.
- **Transient resilience**: `apply_fresh_state()` keeps the previous track visible during AppleScript failures that occur during track transitions.

## macOS Requirements

- macOS 13+, Apple Music app must be running
- Binary must be codesigned (even ad-hoc) to use AppleScript (`codesign -s -`)
- Album art uses sixel graphics when terminal supports it (override with `MUSE_SIXEL=0|1`)
