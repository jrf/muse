# TODO

## Now

- [ ] Test Spotify backend end-to-end with a real Spotify account
- [ ] Device detection — check for active Spotify Connect devices instead of assuming connected

## Next

- [ ] Cross-platform support (Linux/Windows) — gate Swift bridge behind feature flag
- [ ] Spotify queue API — use native queue endpoint instead of playlist-based queue
- [ ] Handle Spotify Premium requirement gracefully (playback control needs Premium)
- [ ] `muse auth` subcommand — trigger Spotify OAuth flow explicitly

## Later

- [ ] Feature flags: `--features apple-music` / `--features spotify` to avoid unnecessary deps
- [ ] Spotify device picker — select which Spotify Connect device to control
- [ ] Transfer playback between devices

---

## Done

- [x] Spotify port — `MusicBackend` trait in `backend.rs`, Apple Music refactored behind it in `bridge.rs`
- [x] `SpotifyBackend` in `spotify.rs` via Spotify Web API (ureq HTTP)
- [x] OAuth 2.0 PKCE flow (browser open → localhost:18234 callback → token cached at `~/.config/muse/spotify_token.json`)
- [x] Runtime backend selection via `backend=spotify` in `~/.config/muse/config`
- [x] Playback control, playlists, search, artwork, lyrics (LRCLIB), favorites, polling notifications
