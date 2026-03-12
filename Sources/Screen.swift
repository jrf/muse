import Foundation

struct Screen {
    private var buffer = ""

    mutating func clear() {
        buffer = ""
    }

    mutating func append(_ text: String) {
        buffer += text
    }

    mutating func moveTo(row: Int, col: Int) {
        buffer += "\u{1B}[\(row);\(col)H"
    }

    mutating func setFg(_ code: Int) {
        buffer += "\u{1B}[38;5;\(code)m"
    }

    mutating func setBold() {
        buffer += "\u{1B}[1m"
    }

    mutating func setDim() {
        buffer += "\u{1B}[2m"
    }

    mutating func reset() {
        buffer += "\u{1B}[0m"
    }

    func flush(to terminal: Terminal) {
        terminal.write("\u{1B}[2J\u{1B}[H") // clear screen + home
        terminal.write(buffer)
    }

    // MARK: - Layout Components

    mutating func renderNowPlaying(state: AppState, width: Int) {
        let boxW = min(width - 2, 56)
        let innerW = boxW - 4 // padding inside box
        let leftPad = max(0, (width - boxW) / 2)
        let pad = String(repeating: " ", count: leftPad)

        var row = 2

        // Top border
        moveTo(row: row, col: 1)
        setFg(75); setBold()
        append(pad + "╭" + String(repeating: "─", count: boxW - 2) + "╮")
        reset()
        row += 1

        // Title bar
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│")
        setBold(); setFg(213)
        let title = "  muse ♫"
        append(title)
        reset(); setFg(75)
        append(String(repeating: " ", count: boxW - 2 - title.visualWidth))
        append("│")
        reset()
        row += 1

        // Separator
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        row += 1

        // Empty line
        row = emptyBoxLine(row: row, pad: pad, boxW: boxW)

        if !state.musicRunning {
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "Music.app is not running", fg: 196)
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "Open Music.app to get started", fg: 245)
        } else if let track = state.track {
            // Song title
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: truncate(track.name, to: innerW), fg: 255, bold: true)

            // Artist — Album
            let subtitle = "\(track.artist) — \(track.album)"
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: truncate(subtitle, to: innerW), fg: 249)

            // Empty line
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)

            // Progress bar
            row = renderProgressBar(row: row, pad: pad, boxW: boxW, position: track.position, duration: track.duration)
        } else {
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "No track playing", fg: 245)
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
        }

        // Empty line
        row = emptyBoxLine(row: row, pad: pad, boxW: boxW)

        // Controls line
        let playIcon = state.playerState == .playing ? "▐▐" : " ▶"
        let shuffleStr = state.shuffleEnabled ? "⤮ on " : "⤮ off"
        let repeatStr = "⟳ \(state.repeatMode.label)"
        let volStr = "Vol: \(state.volume)%"
        let controls = " ◂◂  \(playIcon)  ▸▸      \(shuffleStr)  \(repeatStr)   \(volStr) "
        row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: truncate(controls, to: innerW), fg: 252)

        // Empty line
        row = emptyBoxLine(row: row, pad: pad, boxW: boxW)

        // Separator
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        row += 1

        // Help lines
        row = helpBoxLine(row: row, pad: pad, boxW: boxW, text: "space Play/Pause · n Next · p Prev")
        row = helpBoxLine(row: row, pad: pad, boxW: boxW, text: "+/- Volume · s Shuffle · r Repeat · q Quit")
        row = helpBoxLine(row: row, pad: pad, boxW: boxW, text: "l Library · / Search")

        // Bottom border
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "╰" + String(repeating: "─", count: boxW - 2) + "╯")
        reset()
    }

    mutating func renderLibrary(playlists: [String], selected: Int, scrollOffset: Int, width: Int, height: Int) {
        let boxW = min(width - 2, 56)
        let innerW = boxW - 6
        let leftPad = max(0, (width - boxW) / 2)
        let pad = String(repeating: " ", count: leftPad)
        let maxVisible = min(height - 10, 15)

        var row = 2

        moveTo(row: row, col: 1)
        setFg(75); setBold()
        append(pad + "╭" + String(repeating: "─", count: boxW - 2) + "╮")
        reset()
        row += 1

        // Title
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│")
        setBold(); setFg(213)
        let title = "  Library"
        append(title)
        reset(); setFg(75)
        append(String(repeating: " ", count: boxW - 2 - title.visualWidth))
        append("│")
        reset()
        row += 1

        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        row += 1

        if playlists.isEmpty {
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "No playlists found", fg: 245)
        } else {
            let end = min(scrollOffset + maxVisible, playlists.count)
            for i in scrollOffset..<end {
                moveTo(row: row, col: 1)
                setFg(75)
                append(pad + "│  ")
                if i == selected {
                    setFg(213); setBold()
                    append("▸ ")
                } else {
                    setFg(252)
                    append("  ")
                }
                let name = truncate(playlists[i], to: innerW)
                append(name)
                reset(); setFg(75)
                let used = (i == selected ? 2 : 2) + name.visualWidth
                append(String(repeating: " ", count: max(0, innerW + 2 - used)))
                append("│")
                reset()
                row += 1
            }
        }

        row = emptyBoxLine(row: row, pad: pad, boxW: boxW)

        // Help
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        row += 1

        row = helpBoxLine(row: row, pad: pad, boxW: boxW, text: "↑/↓ Navigate · Enter Browse · Esc Back")

        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "╰" + String(repeating: "─", count: boxW - 2) + "╯")
        reset()
    }

    mutating func renderPlaylistTracks(playlistName: String, tracks: [PlaylistTrack], selected: Int, scrollOffset: Int, width: Int, height: Int) {
        let boxW = min(width - 2, 56)
        let innerW = boxW - 6
        let leftPad = max(0, (width - boxW) / 2)
        let pad = String(repeating: " ", count: leftPad)
        let maxVisible = min(height - 10, 15)

        var row = 2

        moveTo(row: row, col: 1)
        setFg(75); setBold()
        append(pad + "╭" + String(repeating: "─", count: boxW - 2) + "╮")
        reset()
        row += 1

        // Title — playlist name
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│")
        setBold(); setFg(213)
        let title = "  " + truncate(playlistName, to: boxW - 6)
        append(title)
        reset(); setFg(75)
        append(String(repeating: " ", count: max(0, boxW - 2 - title.visualWidth)))
        append("│")
        reset()
        row += 1

        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        row += 1

        if tracks.isEmpty {
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "No tracks", fg: 245)
        } else {
            let end = min(scrollOffset + maxVisible, tracks.count)
            for i in scrollOffset..<end {
                let t = tracks[i]
                moveTo(row: row, col: 1)
                setFg(75)
                append(pad + "│  ")
                if i == selected {
                    setFg(213); setBold()
                    append("▸ ")
                } else {
                    setFg(252)
                    append("  ")
                }
                let dur = formatTime(t.duration)
                let maxNameW = innerW - dur.count - 1
                let entry = "\(t.name) — \(t.artist)"
                let truncEntry = truncate(entry, to: max(0, maxNameW))
                append(truncEntry)
                reset(); setFg(240)
                let used = 2 + truncEntry.visualWidth
                let gap = max(1, innerW + 2 - used - dur.count)
                append(String(repeating: " ", count: gap))
                append(dur)
                reset(); setFg(75)
                append("│")
                reset()
                row += 1
            }

            // Track count
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            let countStr = "\(tracks.count) tracks"
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: countStr, fg: 245)
        }

        row = emptyBoxLine(row: row, pad: pad, boxW: boxW)

        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        row += 1

        row = helpBoxLine(row: row, pad: pad, boxW: boxW, text: "↑/↓ Nav · Enter Play · Space Play All · Esc Back")

        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "╰" + String(repeating: "─", count: boxW - 2) + "╯")
        reset()
    }

    mutating func renderSearch(query: String, results: [(name: String, artist: String, album: String)], selected: Int, scrollOffset: Int, width: Int, height: Int) {
        let boxW = min(width - 2, 56)
        let innerW = boxW - 6
        let leftPad = max(0, (width - boxW) / 2)
        let pad = String(repeating: " ", count: leftPad)
        let maxVisible = min(height - 12, 12)

        var row = 2

        moveTo(row: row, col: 1)
        setFg(75); setBold()
        append(pad + "╭" + String(repeating: "─", count: boxW - 2) + "╮")
        reset()
        row += 1

        // Title
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│")
        setBold(); setFg(213)
        let title = "  Search"
        append(title)
        reset(); setFg(75)
        append(String(repeating: " ", count: boxW - 2 - title.visualWidth))
        append("│")
        reset()
        row += 1

        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        row += 1

        // Search input
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│  ")
        setFg(252)
        let prompt = "/ " + query + "▏"
        append(truncate(prompt, to: innerW + 2))
        reset(); setFg(75)
        append(String(repeating: " ", count: max(0, boxW - 4 - prompt.visualWidth)))
        append("│")
        reset()
        row += 1

        row = emptyBoxLine(row: row, pad: pad, boxW: boxW)

        if results.isEmpty && !query.isEmpty {
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "No results", fg: 245)
        } else {
            let end = min(scrollOffset + maxVisible, results.count)
            for i in scrollOffset..<end {
                let r = results[i]
                moveTo(row: row, col: 1)
                setFg(75)
                append(pad + "│  ")
                if i == selected {
                    setFg(213); setBold()
                    append("▸ ")
                } else {
                    setFg(252)
                    append("  ")
                }
                let entry = "\(r.name) — \(r.artist)"
                append(truncate(entry, to: innerW))
                reset(); setFg(75)
                let used = 2 + min(entry.visualWidth, innerW)
                append(String(repeating: " ", count: max(0, innerW + 2 - used)))
                append("│")
                reset()
                row += 1
            }
        }

        row = emptyBoxLine(row: row, pad: pad, boxW: boxW)

        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        row += 1

        row = helpBoxLine(row: row, pad: pad, boxW: boxW, text: "Type to search · ↑/↓ Nav · Enter Play · Esc Back")

        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "╰" + String(repeating: "─", count: boxW - 2) + "╯")
        reset()
    }

    // MARK: - Helpers

    private mutating func emptyBoxLine(row: Int, pad: String, boxW: Int) -> Int {
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│" + String(repeating: " ", count: boxW - 2) + "│")
        reset()
        return row + 1
    }

    private mutating func centeredBoxLine(row: Int, pad: String, boxW: Int, text: String, fg: Int, bold: Bool = false) -> Int {
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│")
        let textW = text.visualWidth
        let space = boxW - 2
        let leftSpace = max(0, (space - textW) / 2)
        let rightSpace = max(0, space - leftSpace - textW)
        append(String(repeating: " ", count: leftSpace))
        setFg(fg)
        if bold { setBold() }
        append(text)
        reset(); setFg(75)
        append(String(repeating: " ", count: rightSpace))
        append("│")
        reset()
        return row + 1
    }

    private mutating func helpBoxLine(row: Int, pad: String, boxW: Int, text: String) -> Int {
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│  ")
        setDim(); setFg(245)
        let t = truncate(text, to: boxW - 6)
        append(t)
        reset(); setFg(75)
        append(String(repeating: " ", count: max(0, boxW - 4 - t.visualWidth)))
        append("│")
        reset()
        return row + 1
    }

    private mutating func renderProgressBar(row: Int, pad: String, boxW: Int, position: Double, duration: Double) -> Int {
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│  ")

        let timeStr = "  \(formatTime(position)) / \(formatTime(duration))  "
        let barMax = boxW - 4 - timeStr.count
        let progress = duration > 0 ? min(1.0, position / duration) : 0
        let filled = Int(Double(barMax) * progress)
        let empty = barMax - filled

        setFg(213)
        append(String(repeating: "━", count: max(0, filled)))
        setFg(240)
        append(String(repeating: "─", count: max(0, empty)))
        setFg(249)
        append(timeStr)
        reset(); setFg(75)
        append("│")
        reset()
        return row + 1
    }

    private func formatTime(_ seconds: Double) -> String {
        let total = Int(max(0, seconds))
        let m = total / 60
        let s = total % 60
        return String(format: "%d:%02d", m, s)
    }

    private func truncate(_ str: String, to maxWidth: Int) -> String {
        guard maxWidth > 0 else { return "" }
        if str.visualWidth <= maxWidth {
            return str
        }
        var result = ""
        var width = 0
        for char in str {
            let charW = String(char).visualWidth
            if width + charW > maxWidth - 1 {
                result += "…"
                break
            }
            result.append(char)
            width += charW
        }
        return result
    }
}

extension String {
    var visualWidth: Int {
        // Approximate: count characters, treating most as width 1.
        // This is sufficient for ASCII + basic Unicode.
        var w = 0
        for scalar in self.unicodeScalars {
            let v = scalar.value
            // CJK and wide chars
            if (v >= 0x1100 && v <= 0x115F) ||
               (v >= 0x2E80 && v <= 0xA4CF) ||
               (v >= 0xAC00 && v <= 0xD7AF) ||
               (v >= 0xF900 && v <= 0xFAFF) ||
               (v >= 0xFE10 && v <= 0xFE6F) ||
               (v >= 0xFF01 && v <= 0xFF60) ||
               (v >= 0xFFE0 && v <= 0xFFE6) ||
               (v >= 0x20000 && v <= 0x2FA1F) {
                w += 2
            } else if v >= 0x0300 && v <= 0x036F {
                // Combining marks: zero width
            } else {
                w += 1
            }
        }
        return w
    }
}
