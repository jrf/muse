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

    // MARK: - Main Renderer

    mutating func renderMain(state: AppState, width: Int, height: Int) {
        let boxW = min(width - 2, 80)
        let leftPad = max(0, (width - boxW) / 2)
        let pad = String(repeating: " ", count: leftPad)

        var row = 1

        // Top border
        moveTo(row: row, col: 1)
        setFg(75); setBold()
        append(pad + "╭" + String(repeating: "─", count: boxW - 2) + "╮")
        reset()
        row += 1

        // Title bar
        row = titleBarLine(row: row, pad: pad, boxW: boxW, text: "  muse ♫")

        // Separator
        row = separatorLine(row: row, pad: pad, boxW: boxW)

        // Player section
        row = renderPlayerSection(state: state, row: row, pad: pad, boxW: boxW)

        // Separator
        row = separatorLine(row: row, pad: pad, boxW: boxW)

        // Tab bar
        row = renderTabBar(state: state, row: row, pad: pad, boxW: boxW)

        // Separator
        row = separatorLine(row: row, pad: pad, boxW: boxW)

        // Tab content — fills remaining height
        // Reserve 3 rows for footer (separator + help + bottom border)
        let contentRows = max(3, height - row - 2)
        row = renderTabContent(state: state, row: row, pad: pad, boxW: boxW, maxRows: contentRows)

        // Separator
        row = separatorLine(row: row, pad: pad, boxW: boxW)

        // Help line
        row = renderHelpLine(state: state, row: row, pad: pad, boxW: boxW)

        // Bottom border
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "╰" + String(repeating: "─", count: boxW - 2) + "╯")
        reset()
    }

    // MARK: - Player Section

    private mutating func renderPlayerSection(state: AppState, row: Int, pad: String, boxW: Int) -> Int {
        var row = row
        let innerW = boxW - 4

        if !state.musicRunning {
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "Music.app is not running", fg: 196)
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "Open Music.app to get started", fg: 245)
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
        } else if let track = state.track {
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: truncate(track.name, to: innerW), fg: 255, bold: true)
            let subtitle = "\(track.artist) — \(track.album)"
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: truncate(subtitle, to: innerW), fg: 249)
            row = renderProgressBar(row: row, pad: pad, boxW: boxW, position: track.position, duration: track.duration)

            // Controls line
            let playIcon = state.playerState == .playing ? "▐▐" : " ▶"
            let shuffleStr = state.shuffleEnabled ? "⤮ on " : "⤮ off"
            let repeatStr = "⟳ \(state.repeatMode.label)"
            let volStr = "Vol: \(state.volume)%"
            let controls = " ◂◂  \(playIcon)  ▸▸      \(shuffleStr)  \(repeatStr)   \(volStr) "
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: truncate(controls, to: innerW), fg: 252)
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
        } else {
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "No track playing", fg: 245)
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
        }

        return row
    }

    // MARK: - Tab Bar

    private mutating func renderTabBar(state: AppState, row: Int, pad: String, boxW: Int) -> Int {
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│  ")

        let tabs: [(Tab, String)] = [(.queue, "Queue"), (.library, "Library"), (.search, "Search")]
        var used = 0
        for (i, (tab, label)) in tabs.enumerated() {
            if i > 0 {
                reset(); setFg(240)
                append("  ")
                used += 2
            }
            if tab == state.activeTab {
                reset(); setFg(213); setBold()
                let text = "[\(label)]"
                append(text)
                used += text.visualWidth
            } else {
                reset(); setFg(252)
                append(" \(label) ")
                used += label.count + 2
            }
        }

        reset(); setFg(75)
        append(String(repeating: " ", count: max(0, boxW - 4 - used)))
        append("│")
        reset()
        return row + 1
    }

    // MARK: - Tab Content

    private mutating func renderTabContent(state: AppState, row: Int, pad: String, boxW: Int, maxRows: Int) -> Int {
        switch state.activeTab {
        case .queue:
            return renderQueueContent(state: state, row: row, pad: pad, boxW: boxW, maxRows: maxRows)
        case .library:
            return renderLibraryContent(state: state, row: row, pad: pad, boxW: boxW, maxRows: maxRows)
        case .search:
            return renderSearchContent(state: state, row: row, pad: pad, boxW: boxW, maxRows: maxRows)
        }
    }

    private mutating func renderQueueContent(state: AppState, row: Int, pad: String, boxW: Int, maxRows: Int) -> Int {
        var row = row
        let innerW = boxW - 6

        if state.queueTracks.isEmpty {
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "No queue — play a playlist to fill", fg: 245)
            // Fill remaining rows
            let filled = 2
            for _ in filled..<maxRows {
                row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            }
        } else {
            let end = min(state.queueScroll + maxRows, state.queueTracks.count)
            var rendered = 0
            for i in state.queueScroll..<end {
                let t = state.queueTracks[i]
                row = renderTrackLine(row: row, pad: pad, boxW: boxW, innerW: innerW,
                                      name: "\(t.name) — \(t.artist)", duration: t.duration,
                                      selected: i == state.queueSelected)
                rendered += 1
            }
            // Fill remaining rows
            for _ in rendered..<maxRows {
                row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            }
        }
        return row
    }

    private mutating func renderLibraryContent(state: AppState, row: Int, pad: String, boxW: Int, maxRows: Int) -> Int {
        switch state.librarySubView {
        case .playlists:
            return renderPlaylistList(state: state, row: row, pad: pad, boxW: boxW, maxRows: maxRows)
        case .tracks(let name):
            return renderPlaylistTracksContent(state: state, playlistName: name, row: row, pad: pad, boxW: boxW, maxRows: maxRows)
        }
    }

    private mutating func renderPlaylistList(state: AppState, row: Int, pad: String, boxW: Int, maxRows: Int) -> Int {
        var row = row
        let innerW = boxW - 6

        if state.playlists.isEmpty {
            row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "Loading playlists…", fg: 245)
            let filled = 2
            for _ in filled..<maxRows {
                row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            }
        } else {
            let end = min(state.libraryScroll + maxRows, state.playlists.count)
            var rendered = 0
            for i in state.libraryScroll..<end {
                moveTo(row: row, col: 1)
                setFg(75)
                append(pad + "│  ")
                if i == state.librarySelected {
                    setFg(213); setBold()
                    append("▸ ")
                } else {
                    setFg(252)
                    append("  ")
                }
                let name = truncate(state.playlists[i], to: innerW)
                append(name)
                reset(); setFg(75)
                let used = 2 + name.visualWidth
                append(String(repeating: " ", count: max(0, innerW + 2 - used)))
                append("│")
                reset()
                row += 1
                rendered += 1
            }
            for _ in rendered..<maxRows {
                row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            }
        }
        return row
    }

    private mutating func renderPlaylistTracksContent(state: AppState, playlistName: String, row: Int, pad: String, boxW: Int, maxRows: Int) -> Int {
        var row = row
        let innerW = boxW - 6

        // Header with back indicator and playlist name
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│  ")
        setFg(245); setDim()
        let header = "← \(truncate(playlistName, to: innerW - 2))"
        append(header)
        reset(); setFg(75)
        append(String(repeating: " ", count: max(0, boxW - 4 - header.visualWidth)))
        append("│")
        reset()
        row += 1

        let trackRows = maxRows - 1 // one row used by header

        if state.playlistTracks.isEmpty {
            row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "Loading…", fg: 245)
            let filled = 1
            for _ in filled..<trackRows {
                row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            }
        } else {
            let end = min(state.playlistTracksScroll + trackRows, state.playlistTracks.count)
            var rendered = 0
            for i in state.playlistTracksScroll..<end {
                let t = state.playlistTracks[i]
                row = renderTrackLine(row: row, pad: pad, boxW: boxW, innerW: innerW,
                                      name: "\(t.name) — \(t.artist)", duration: t.duration,
                                      selected: i == state.playlistTracksSelected)
                rendered += 1
            }
            for _ in rendered..<trackRows {
                row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            }
        }
        return row
    }

    private mutating func renderSearchContent(state: AppState, row: Int, pad: String, boxW: Int, maxRows: Int) -> Int {
        var row = row
        let innerW = boxW - 6

        // Search input
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│  ")
        setFg(252)
        let prompt = "/ " + state.searchQuery + "▏"
        append(truncate(prompt, to: innerW + 2))
        reset(); setFg(75)
        append(String(repeating: " ", count: max(0, boxW - 4 - prompt.visualWidth)))
        append("│")
        reset()
        row += 1

        let resultRows = maxRows - 1 // one row used by search input

        if state.searchResults.isEmpty {
            if !state.searchQuery.isEmpty {
                row = centeredBoxLine(row: row, pad: pad, boxW: boxW, text: "No results", fg: 245)
                let filled = 1
                for _ in filled..<resultRows {
                    row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
                }
            } else {
                for _ in 0..<resultRows {
                    row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
                }
            }
        } else {
            let end = min(state.searchScroll + resultRows, state.searchResults.count)
            var rendered = 0
            for i in state.searchScroll..<end {
                let r = state.searchResults[i]
                moveTo(row: row, col: 1)
                setFg(75)
                append(pad + "│  ")
                if i == state.searchSelected {
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
                rendered += 1
            }
            for _ in rendered..<resultRows {
                row = emptyBoxLine(row: row, pad: pad, boxW: boxW)
            }
        }
        return row
    }

    // MARK: - Help Line

    private mutating func renderHelpLine(state: AppState, row: Int, pad: String, boxW: Int) -> Int {
        let help: String
        switch state.activeTab {
        case .queue:
            help = "1/2/3 Tabs · ↑/↓ Nav · Enter Play · space Pause · q Quit"
        case .library:
            switch state.librarySubView {
            case .playlists:
                help = "1/2/3 Tabs · ↑/↓ Nav · Enter Browse · space Pause · q Quit"
            case .tracks:
                help = "1/2/3 Tabs · ↑/↓ Nav · Enter Play · Esc Back · q Quit"
            }
        case .search:
            help = "Type to search · ↑/↓ Nav · Enter Play · Esc Clear · q Quit"
        }
        return helpBoxLine(row: row, pad: pad, boxW: boxW, text: help)
    }

    // MARK: - Helpers

    private mutating func renderTrackLine(row: Int, pad: String, boxW: Int, innerW: Int,
                                          name: String, duration: Double, selected: Bool) -> Int {
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│  ")
        if selected {
            setFg(213); setBold()
            append("▸ ")
        } else {
            setFg(252)
            append("  ")
        }
        let dur = formatTime(duration)
        let maxNameW = innerW - dur.count - 1
        let truncEntry = truncate(name, to: max(0, maxNameW))
        append(truncEntry)
        reset(); setFg(240)
        let used = 2 + truncEntry.visualWidth
        let gap = max(1, innerW + 2 - used - dur.count)
        append(String(repeating: " ", count: gap))
        append(dur)
        reset(); setFg(75)
        append("│")
        reset()
        return row + 1
    }

    private mutating func titleBarLine(row: Int, pad: String, boxW: Int, text: String) -> Int {
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "│")
        setBold(); setFg(213)
        append(text)
        reset(); setFg(75)
        append(String(repeating: " ", count: max(0, boxW - 2 - text.visualWidth)))
        append("│")
        reset()
        return row + 1
    }

    private mutating func separatorLine(row: Int, pad: String, boxW: Int) -> Int {
        moveTo(row: row, col: 1)
        setFg(75)
        append(pad + "├" + String(repeating: "─", count: boxW - 2) + "┤")
        reset()
        return row + 1
    }

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
        var w = 0
        for scalar in self.unicodeScalars {
            let v = scalar.value
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
