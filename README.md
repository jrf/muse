# muse

A terminal UI for controlling Apple Music on macOS.

```
╭──────────────────────────────────────────────────────────────────────────────╮
│  muse ♫                                                                      │
├──────────────────────────────────────────────────────────────────────────────┤
│                         Song Title                                           │
│                      Artist — Album                                          │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━───────────────────────────  2:34 / 4:12       │
│                ◂◂   ▶  ▸▸      ⤮ off  ⟳ off   Vol: 80%                       │
├──────────────────────────────────────────────────────────────────────────────┤
│  [Queue]   Library   Search                                                  │
├──────────────────────────────────────────────────────────────────────────────┤
│  ▸ Track 1 — Artist                                                 3:42     │
│    Track 2 — Artist                                                 4:15     │
│    Track 3 — Artist                                                 2:58     │
├──────────────────────────────────────────────────────────────────────────────┤
│  1/2/3 Tabs · ↑/↓ Nav · Enter Play · space Pause · q Quit                    │
╰──────────────────────────────────────────────────────────────────────────────╯
```

## Requirements

- macOS 13+
- Swift 6.2+
- Apple Music app (must be running)

## Install

```
git clone <repo-url>
cd muse
swift build -c release
cp .build/release/muse /usr/local/bin/
```

Or run directly:

```
swift run muse
```

## Usage

Launch `muse` in any terminal. The player panel at the top always shows the current track. Use the tabbed panel below to browse your library, manage the queue, or search.

### Key Bindings

| Key | Action |
|-----|--------|
| `1` | Queue tab |
| `2` / `l` | Library tab |
| `3` / `/` | Search tab |
| `space` | Play / Pause |
| `n` | Next track |
| `p` | Previous track |
| `+` / `=` | Volume up |
| `-` | Volume down |
| `s` | Toggle shuffle |
| `r` | Cycle repeat (off → all → one) |
| `↑` / `↓` | Navigate list |
| `Enter` | Play track / Browse playlist |
| `Esc` | Back (library) / Clear (search) |
| `q` | Quit |

### Tabs

- **Queue** — tracks from the last playlist you played. Select a track and press Enter to jump to it.
- **Library** — browse your playlists. Press Enter to see tracks, Enter again to play. Esc goes back to the playlist list.
- **Search** — type to search your library. Results appear as you type (minimum 2 characters). Enter plays the selected result.

Playback controls (`space`, `n`, `p`, `+`/`-`, `s`, `r`) work from any tab. In the Search tab, letter keys are captured for typing, so `n`/`p`/`s`/`r` only work as playback controls from the other tabs.
