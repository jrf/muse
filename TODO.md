# TODO

## Now

- [ ] Test Spotify backend end-to-end with a real Spotify account #chore
- [ ] Device detection — check for active Spotify Connect devices instead of assuming connected #improvement

## Next

- [ ] Spotify queue API — use native queue endpoint instead of playlist-based queue #improvement
- [ ] Handle Spotify Premium requirement gracefully (playback control needs Premium) #improvement
- [ ] `muse auth` subcommand — trigger Spotify OAuth flow explicitly #feature

## Later

- [ ] Spotify device picker — select which Spotify Connect device to control #feature
- [ ] Transfer playback between devices #feature

## Done

- [x] Fix progress bar time text readability across themes (monochrome, sunset, purple, fire) #bug
- [x] Spotify port — `MusicBackend` trait in `backend.rs`, Apple Music refactored behind it in `bridge.rs` #refactor
- [x] `SpotifyBackend` in `spotify.rs` via Spotify Web API (ureq HTTP) #feature
- [x] OAuth 2.0 PKCE flow (browser open → localhost:18234 callback → token cached at `~/.config/muse/spotify_token.json`) #feature
- [x] Runtime backend selection via `backend=spotify` in `~/.config/muse/config` #feature
- [x] Playback control, playlists, search, artwork, lyrics (LRCLIB), favorites, polling notifications #feature
- [x] Ctrl+F / Ctrl+B for page down/up navigation (vim-style) #feature
- [x] Remove track from queue (`d`/`x` in Queue tab) #feature
- [x] Remove track from playlist (`d`/`x` in Library tracks view) #feature
- [x] Feature flags: `--features apple-music` / `--features spotify` (both on by default, buildable independently) #improvement
- [x] Removed auto-advance (was conflicting with Music.app's native playlist advancement) #bug
- [x] Fixed favorite toggle (split into separate read/write AppleScript calls) #bug
- [x] OS-aware build: Apple Music auto-skipped on non-macOS, no manual flags needed #improvement
- [x] Queue restored on relaunch from persisted playlist state #feature
- [x] Reverted auto-advance feature — both backends handle queue advancement natively, `sync_queue_selection` keeps the UI in sync #bug
- [x] Fix end-of-track stall — call `next_track()` when track finishes but backend doesn't advance on its own #bug
- [x] External theme files — themes loaded from `~/.config/muse/themes/*.toml` instead of hardcoded in Rust #refactor
- [x] TOML config file (`config.toml`) replacing legacy plain-text KEY=VALUE format #refactor
- [x] Theme picker moved from tab bar to overlay (activated by `t`) #improvement
- [x] Configurable `default_tab`, `ui_width`, and `show_artwork` in config.toml #feature
