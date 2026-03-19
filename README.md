# muse

[![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)](https://www.rust-lang.org/)
[![Swift](https://img.shields.io/badge/swift-6.2+-F05138?logo=swift&logoColor=white)](https://swift.org/)
[![macOS](https://img.shields.io/badge/macOS-13%2B-000000?logo=apple&logoColor=white)](https://www.apple.com/macos/)

A terminal UI for controlling Apple Music and Spotify on macOS.

![muse](muse.png)

## Features

- **Dual backend** — Apple Music (via AppleScript) or Spotify (via Web API)
- Full playback control (play/pause, next/previous, volume, shuffle, repeat)
- Browse playlists and search your library
- Favorite tracks and add/remove them from playlists
- Remove individual tracks from the queue
- Jump to the current track's artist or album with a single key
- Open the artist page or full album directly in Music.app or Spotify
- Queue management with auto-advance
- Lyrics display fetched from LRCLIB (with embedded lyrics fallback)
- Album art display via sixel graphics in supported terminals
- Vim-style navigation (j/k, g/G, Ctrl+F/Ctrl+B)
- Customizable color themes

## Requirements

- **macOS**: Apple Music and/or Spotify. Swift 6.2+ required for Apple Music backend.
- **Linux/Windows**: Spotify only. No additional dependencies.

## Install

```
git clone <repo-url>
cd muse
just
```

On Linux, `cargo build --release` works out of the box — the Apple Music backend is automatically skipped on non-macOS platforms.

### Spotify-Only (explicit, no Xcode needed)

```
cargo build --release --no-default-features --features spotify
```

## Configuration

Config file: `~/.config/muse/config`

```
backend=spotify              # optional, default is apple_music
spotify_client_id=YOUR_ID    # required if backend=spotify
theme=synthwave              # color theme
```

### Spotify Setup

1. Create an app at [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard)
2. Add `http://localhost:18234/callback` as a redirect URI
3. Add `backend=spotify` and `spotify_client_id=YOUR_ID` to your config
4. Run `muse` — it will open your browser for login on first launch

## Usage

Launch `muse` in any terminal. The player panel at the top always shows the current track. Use the tabbed panel below to browse your library, manage the queue, or search.

### Key Bindings

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle tabs |
| `l` | Library tab |
| `/` | Search tab |
| `L` | Lyrics tab |
| `space` | Play / Pause |
| `n` | Next track |
| `p` | Previous track |
| `+` / `=` | Volume up |
| `-` | Volume down |
| `s` | Toggle shuffle |
| `r` | Cycle repeat (off → all → one) |
| `f` | Toggle favorite |
| `d` / `x` | Remove track (queue or playlist) |
| `C` | Clear queue |
| `P` | Add to playlist |
| `a` | Search current track's artist |
| `A` | Search current track's album |
| `o` | Open artist in Music.app / Spotify |
| `O` | Open album in Music.app / Spotify |
| `j` / `k` | Navigate list (vim-style) |
| `g` / `G` | Jump to top / bottom |
| `Ctrl+F` / `Ctrl+B` | Page down / up |
| `↑` / `↓` | Navigate list / Scroll lyrics |
| `PgUp` / `PgDn` | Page up / down |
| `Home` / `End` | Jump to top / bottom |
| `Enter` | Play track / Browse playlist |
| `Backspace` | Back (library) / Clear (search) |
| `?` | Toggle help overlay |
| `q` | Quit |

### Tabs

- **Queue** — tracks from the last playlist you played. Select a track and press Enter to jump to it. Tracks auto-advance when the current one finishes. Press `d` to remove a track.
- **Library** — browse your playlists. Press Enter to see tracks, Enter again to play. Press `d` to remove a track from the playlist. Backspace goes back to the playlist list.
- **Search** — type to search your library. Results appear as you type (minimum 2 characters). Enter plays the selected result.
- **Lyrics** — displays lyrics for the current track. Fetched from [LRCLIB](https://lrclib.net) (falls back to embedded lyrics if available). Scroll with arrow keys. Shows "No lyrics available" when none are found.
- **Themes** — select a color theme. Press Enter to apply.

Playback controls (`space`, `n`, `p`, `+`/`-`, `s`, `r`) work from any tab. In the Search tab, letter keys are captured for typing, so `n`/`p`/`s`/`r`/`a` only work as playback controls from the other tabs.

### Album Art

Album art is displayed automatically in terminals that support sixel graphics (WezTerm, iTerm2, foot, Ghostty, etc.). Detection uses the DA1 terminal query. To override:

```
MUSE_SIXEL=1 muse   # force enable
MUSE_SIXEL=0 muse   # force disable
```
