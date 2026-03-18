# TODO

## Spotify Port

Replace `bridge.rs` with a `spotify.rs` backend using Spotify's Web API (REST/HTTP via `reqwest`). The UI, state, and event loop stay nearly identical.

### API Coverage

All needed functionality is supported by Spotify's Web API:

- **Playback control** — play, pause, skip, previous, seek, volume, shuffle, repeat (requires Premium)
- **Playback state** — current track, position, device, shuffle/repeat status
- **Queue** — get queue, add to queue
- **Playlists** — list user playlists, get playlist tracks
- **Search** — tracks, albums, artists
- **Library** — saved tracks, save/unsave (equivalent of "favorite")
- **Album art** — image URLs included in track metadata

### Key Differences from Apple Music Version

1. **OAuth 2.0 auth flow** — one-time browser login, refresh tokens. Requires a registered Spotify developer app (client ID).
2. **HTTP instead of FFI** — all calls are async HTTP. No Swift, no macOS dependency. Makes it **cross-platform**.
3. **Polling instead of notifications** — no push notifications for state changes; poll playback state (same 2s pattern already used for AppleScript).
4. **Lyrics** — Spotify doesn't expose lyrics via API. LRCLIB (already a fallback) becomes the primary source.
5. **Device detection** — instead of `music_ensure_running`/`music_is_running`, check for active Spotify Connect devices.

### Recommended Approach

Define a `MusicBackend` trait, implement it for both Apple Music and Spotify, select at runtime via config/flag. The bridge surface is ~15 functions and the existing types (`Track`, `PlayerState`, `FullState`, etc.) work for both services.

### Steps

1. Define `MusicBackend` trait matching the current `bridge.rs` public API
2. Refactor existing Apple Music code behind the trait
3. Implement Spotify backend (`spotify.rs`) using `reqwest`
4. Add OAuth 2.0 token flow (browser open → localhost callback → token storage)
5. Add runtime backend selection (CLI flag or config file)
