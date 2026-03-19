# TODO

## Now

- [ ] Test Spotify backend end-to-end with a real Spotify account
- [ ] Device detection — check for active Spotify Connect devices instead of assuming connected

## Next

- [ ] Spotify queue API — use native queue endpoint instead of playlist-based queue
- [ ] Handle Spotify Premium requirement gracefully (playback control needs Premium)
- [ ] `muse auth` subcommand — trigger Spotify OAuth flow explicitly

## Later

- [ ] Spotify device picker — select which Spotify Connect device to control
- [ ] Transfer playback between devices

## Done

- [x] Spotify port — `MusicBackend` trait in `backend.rs`, Apple Music refactored behind it in `bridge.rs`
- [x] `SpotifyBackend` in `spotify.rs` via Spotify Web API (ureq HTTP)
- [x] OAuth 2.0 PKCE flow (browser open → localhost:18234 callback → token cached at `~/.config/muse/spotify_token.json`)
- [x] Runtime backend selection via `backend=spotify` in `~/.config/muse/config`
- [x] Playback control, playlists, search, artwork, lyrics (LRCLIB), favorites, polling notifications
- [x] Ctrl+F / Ctrl+B for page down/up navigation (vim-style)
- [x] Remove track from queue (`d`/`x` in Queue tab)
- [x] Remove track from playlist (`d`/`x` in Library tracks view)
- [x] Feature flags: `--features apple-music` / `--features spotify` (both on by default, buildable independently)
- [x] Removed auto-advance (was conflicting with Music.app's native playlist advancement)
- [x] Fixed favorite toggle (split into separate read/write AppleScript calls)
- [x] OS-aware build: Apple Music auto-skipped on non-macOS, no manual flags needed
- [x] Queue restored on relaunch from persisted playlist state
- [x] Reverted auto-advance feature — both backends handle queue advancement natively, `sync_queue_selection` keeps the UI in sync
