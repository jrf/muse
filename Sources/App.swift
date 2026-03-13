import Foundation
import Darwin

private nonisolated(unsafe) var sigwinchReceived: sig_atomic_t = 0

final class App: @unchecked Sendable {
    private let terminal = Terminal()
    private let music = MusicController()
    private var state = AppState()
    private var currentTheme = Theme.defaultTheme
    private var running = true
    private var lastRefresh: Date = .distantPast
    private let refreshInterval: TimeInterval = 2.0
    private var lastPositionUpdate: Date = Date()
    private var refreshInFlight = false
    private let refreshQueue = DispatchQueue(label: "muse.refresh")
    private var pendingState: AppState?
    private let stateLock = NSLock()

    // Sixel artwork
    private var sixelSupported = false
    private var artworkCache: SixelArt.ArtworkCache?
    private var artworkCacheKey = ""
    private var pendingArtwork: SixelArt.ArtworkCache?
    private var artworkInFlight = false
    private var autoAdvancePending = false

    func run() {
        terminal.enableRawMode()
        sixelSupported = SixelArt.detectSixelSupport()

        defer {
            terminal.restoreMode()
            terminal.clearScreen()
        }

        signal(SIGINT, SIG_IGN)
        signal(SIGWINCH) { _ in sigwinchReceived = 1 }

        // Load persisted theme
        loadConfig()

        // Initial state fetch (blocking for first render)
        applyFresh(music.fetchFullState())

        // Pre-load playlists so library tab is populated immediately
        refreshQueue.async { [self] in
            let playlists = music.getPlaylists()
            stateLock.lock()
            state.playlists = playlists
            stateLock.unlock()
        }

        // Register for Music.app distributed notifications
        let nc = DistributedNotificationCenter.default()
        nc.addObserver(forName: Notification.Name("com.apple.Music.playerInfo"),
                       object: nil, queue: nil) { [weak self] notification in
            self?.handleMusicNotification(notification)
        }
        nc.addObserver(forName: Notification.Name("com.apple.iTunes.playerInfo"),
                       object: nil, queue: nil) { [weak self] notification in
            self?.handleMusicNotification(notification)
        }

        render()

        while running {
            // Pump RunLoop to process distributed notifications
            RunLoop.current.run(mode: .default, before: Date())

            if let key = terminal.readKey() {
                handleKey(key)
            }

            // Apply async refresh result if available
            stateLock.lock()
            if let fresh = pendingState {
                pendingState = nil
                stateLock.unlock()
                applyFresh(fresh)
            } else {
                stateLock.unlock()
            }

            // Full clear on terminal resize to remove stale content
            if sigwinchReceived != 0 {
                sigwinchReceived = 0
                terminal.clearScreen()
            }

            // Always render for smooth progress interpolation
            render()

            // Kick off async refresh if needed
            let now = Date()
            if !refreshInFlight && now.timeIntervalSince(lastRefresh) >= refreshInterval {
                lastRefresh = now
                refreshInFlight = true
                refreshQueue.async { [self] in
                    let fresh = music.fetchFullState()
                    stateLock.lock()
                    pendingState = fresh
                    refreshInFlight = false
                    stateLock.unlock()
                }
            }
        }
    }

    private func applyFresh(_ fresh: AppState) {
        let oldKey = artworkCacheKey(for: state.track)
        state.musicRunning = fresh.musicRunning
        state.playerState = fresh.playerState
        state.track = fresh.track
        state.volume = fresh.volume
        state.shuffleEnabled = fresh.shuffleEnabled
        state.repeatMode = fresh.repeatMode
        lastPositionUpdate = Date()

        let newKey = artworkCacheKey(for: state.track)
        if sixelSupported && newKey != oldKey {
            fetchArtworkAsync(key: newKey)
        }
    }

    // MARK: - Key Handling

    private func handleKey(_ key: Key) {
        if handleGlobalKey(key) { return }

        switch state.activeTab {
        case .queue:
            handleQueueKey(key)
        case .library:
            handleLibraryKey(key)
        case .search:
            handleSearchKey(key)
        case .themes:
            handleThemesKey(key)
        }
    }

    /// Returns true if the key was consumed globally.
    private func handleGlobalKey(_ key: Key) -> Bool {
        let inSearch = state.activeTab == .search

        switch key {
        case .character("q"):
            running = false
            return true
        case .tab:
            switch state.activeTab {
            case .queue: state.activeTab = .library
            case .library: state.activeTab = .search
            case .search: state.activeTab = .themes
            case .themes: state.activeTab = .queue
            }
            return true
        case .shiftTab:
            switch state.activeTab {
            case .queue: state.activeTab = .themes
            case .library: state.activeTab = .queue
            case .search: state.activeTab = .library
            case .themes: state.activeTab = .search
            }
            return true
        case .character("l"):
            if !inSearch {
                state.activeTab = .library
                return true
            }
        case .character("/"):
            state.activeTab = .search
            return true
        case .space:
            state.playerState = state.playerState == .playing ? .paused : .playing
            fireAndRefresh { [self] in music.playPause() }
            return true
        case .character("n"):
            if !inSearch {
                fireAndRefresh { [self] in music.nextTrack() }
                return true
            }
        case .character("p"):
            if !inSearch {
                fireAndRefresh { [self] in music.previousTrack() }
                return true
            }
        case .character("+"), .character("="):
            state.volume = min(100, state.volume + 5)
            let vol = state.volume
            refreshQueue.async { [self] in music.setVolume(vol) }
            return true
        case .character("-"):
            if !inSearch {
                state.volume = max(0, state.volume - 5)
                let vol = state.volume
                refreshQueue.async { [self] in music.setVolume(vol) }
                return true
            }
        case .character("s"):
            if !inSearch {
                state.shuffleEnabled.toggle()
                fireAndRefresh { [self] in music.toggleShuffle() }
                return true
            }
        case .character("r"):
            if !inSearch {
                state.repeatMode = state.repeatMode.next
                fireAndRefresh { [self] in music.cycleRepeat() }
                return true
            }
        default:
            break
        }
        return false
    }

    // MARK: - Queue Tab

    private func handleQueueKey(_ key: Key) {
        switch key {
        case .up:
            if state.queueSelected > 0 {
                state.queueSelected -= 1
                if state.queueSelected < state.queueScroll {
                    state.queueScroll = state.queueSelected
                }
            }
        case .down:
            if state.queueSelected < state.queueTracks.count - 1 {
                state.queueSelected += 1
                let maxVisible = contentRows()
                if state.queueSelected >= state.queueScroll + maxVisible {
                    state.queueScroll = state.queueSelected - maxVisible + 1
                }
            }
        case .enter:
            if !state.queueTracks.isEmpty && state.queueSelected < state.queueTracks.count {
                let playlist = state.queuePlaylistName
                let idx = state.queueSelected
                fireAndRefresh { [self] in music.playTrackInPlaylist(playlist, trackIndex: idx) }
            }
        default:
            break
        }
    }

    // MARK: - Library Tab

    private func handleLibraryKey(_ key: Key) {
        switch state.librarySubView {
        case .playlists:
            handleLibraryPlaylistsKey(key)
        case .tracks:
            handleLibraryTracksKey(key)
        }
    }

    private func handleLibraryPlaylistsKey(_ key: Key) {
        switch key {
        case .up:
            if state.librarySelected > 0 {
                state.librarySelected -= 1
                if state.librarySelected < state.libraryScroll {
                    state.libraryScroll = state.librarySelected
                }
            }
        case .down:
            if state.librarySelected < state.playlists.count - 1 {
                state.librarySelected += 1
                let maxVisible = contentRows()
                if state.librarySelected >= state.libraryScroll + maxVisible {
                    state.libraryScroll = state.librarySelected - maxVisible + 1
                }
            }
        case .enter:
            if !state.playlists.isEmpty && state.librarySelected < state.playlists.count {
                let name = state.playlists[state.librarySelected]
                state.librarySubView = .tracks(name)
                state.playlistTracks = []
                state.playlistTracksSelected = 0
                state.playlistTracksScroll = 0
                refreshQueue.async { [self] in
                    let tracks = music.getPlaylistTracks(name)
                    stateLock.lock()
                    state.playlistTracks = tracks
                    stateLock.unlock()
                }
            }
        default:
            break
        }
    }

    private func handleLibraryTracksKey(_ key: Key) {
        switch key {
        case .backspace:
            state.librarySubView = .playlists
        case .up:
            if state.playlistTracksSelected > 0 {
                state.playlistTracksSelected -= 1
                if state.playlistTracksSelected < state.playlistTracksScroll {
                    state.playlistTracksScroll = state.playlistTracksSelected
                }
            }
        case .down:
            if state.playlistTracksSelected < state.playlistTracks.count - 1 {
                state.playlistTracksSelected += 1
                // Subtract 1 for the "← Back" header row
                let maxVisible = contentRows() - 1
                if state.playlistTracksSelected >= state.playlistTracksScroll + maxVisible {
                    state.playlistTracksScroll = state.playlistTracksSelected - maxVisible + 1
                }
            }
        case .enter:
            if !state.playlistTracks.isEmpty && state.playlistTracksSelected < state.playlistTracks.count {
                if case .tracks(let playlistName) = state.librarySubView {
                    let idx = state.playlistTracksSelected
                    // Populate queue with this playlist's tracks
                    state.queueTracks = state.playlistTracks
                    state.queuePlaylistName = playlistName
                    state.queueSelected = idx
                    state.queueScroll = max(0, idx - 3)
                    fireAndRefresh { [self] in music.playTrackInPlaylist(playlistName, trackIndex: idx) }
                }
            }
        default:
            break
        }
    }

    // MARK: - Search Tab

    private func handleSearchKey(_ key: Key) {
        switch key {
        case .backspace:
            if !state.searchQuery.isEmpty {
                state.searchQuery.removeLast()
                performSearch()
            } else {
                state.searchResults = []
                state.searchSelected = 0
                state.searchScroll = 0
            }
        case .up:
            if state.searchSelected > 0 {
                state.searchSelected -= 1
                if state.searchSelected < state.searchScroll {
                    state.searchScroll = state.searchSelected
                }
            }
        case .down:
            if state.searchSelected < state.searchResults.count - 1 {
                state.searchSelected += 1
                // Subtract 1 for the search input row
                let maxVisible = contentRows() - 1
                if state.searchSelected >= state.searchScroll + maxVisible {
                    state.searchScroll = state.searchSelected - maxVisible + 1
                }
            }
        case .enter:
            if !state.searchResults.isEmpty && state.searchSelected < state.searchResults.count {
                let result = state.searchResults[state.searchSelected]
                fireAndRefresh { [self] in music.playTrack(result.name, artist: result.artist) }
            }
        case .character(let ch):
            state.searchQuery.append(ch)
            state.searchSelected = 0
            state.searchScroll = 0
            performSearch()
        default:
            break
        }
    }

    private func performSearch() {
        let query = state.searchQuery
        guard query.count >= 2 else {
            state.searchResults = []
            return
        }
        refreshQueue.async { [self] in
            let results = music.searchTracks(query)
            stateLock.lock()
            if state.searchQuery == query {
                state.searchResults = results
            }
            stateLock.unlock()
        }
    }

    // MARK: - Themes Tab

    private func handleThemesKey(_ key: Key) {
        let themes = Theme.allThemes
        switch key {
        case .up:
            if state.themeSelected > 0 {
                state.themeSelected -= 1
                if state.themeSelected < state.themeScroll {
                    state.themeScroll = state.themeSelected
                }
            }
        case .down:
            if state.themeSelected < themes.count - 1 {
                state.themeSelected += 1
                let maxVisible = contentRows()
                if state.themeSelected >= state.themeScroll + maxVisible {
                    state.themeScroll = state.themeSelected - maxVisible + 1
                }
            }
        case .enter:
            if state.themeSelected < themes.count {
                let (name, theme) = themes[state.themeSelected]
                state.themeName = name
                currentTheme = theme
                saveTheme(name)
            }
        default:
            break
        }
    }

    // MARK: - Config

    private static let configDir = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".config/muse")
    private static let configFile = configDir.appendingPathComponent("config")

    private func loadConfig() {
        guard let contents = try? String(contentsOf: Self.configFile, encoding: .utf8) else { return }
        for line in contents.split(separator: "\n") {
            let parts = line.split(separator: "=", maxSplits: 1)
            if parts.count == 2 && parts[0].trimmingCharacters(in: .whitespaces) == "theme" {
                let name = parts[1].trimmingCharacters(in: .whitespaces)
                if let entry = Theme.allThemes.first(where: { $0.name == name }) {
                    state.themeName = entry.name
                    currentTheme = entry.theme
                    if let idx = Theme.allThemes.firstIndex(where: { $0.name == name }) {
                        state.themeSelected = idx
                    }
                }
            }
        }
    }

    private func saveTheme(_ name: String) {
        let fm = FileManager.default
        try? fm.createDirectory(at: Self.configDir, withIntermediateDirectories: true)
        try? "theme=\(name)\n".write(to: Self.configFile, atomically: true, encoding: .utf8)
    }

    // MARK: - Render

    private func render() {
        // Apply pending artwork
        stateLock.lock()
        if let art = pendingArtwork {
            pendingArtwork = nil
            stateLock.unlock()
            artworkCache = art
            artworkCacheKey = art.key
        } else {
            stateLock.unlock()
        }

        let size = terminal.getSize()
        var displayState = state
        // Interpolate progress position when playing
        if state.playerState == .playing, let track = state.track {
            let elapsed = Date().timeIntervalSince(lastPositionUpdate)
            displayState.track?.position = min(track.position + elapsed, track.duration)
        }
        var screen = Screen()
        let artwork = sixelSupported ? artworkCache : nil
        screen.renderMain(state: displayState, width: size.width, height: size.height, theme: currentTheme, artwork: artwork)
        screen.flush(to: terminal)
    }

    // MARK: - Music Notifications

    private func handleMusicNotification(_ notification: Notification) {
        guard let info = notification.userInfo else { return }

        if let stateStr = info["Player State"] as? String {
            switch stateStr {
            case "Playing":
                state.playerState = .playing
                autoAdvancePending = false
            case "Paused": state.playerState = .paused
            case "Stopped":
                state.playerState = .stopped
                state.track = nil
                autoAdvanceQueue()
            default: break
            }
        }

        if let name = info["Name"] as? String {
            let artist = info["Artist"] as? String ?? ""
            let album = info["Album"] as? String ?? ""
            let totalTimeMs = info["Total Time"] as? Double ?? 0
            let duration = totalTimeMs / 1000.0
            let isNewTrack = state.track?.name != name || state.track?.artist != artist

            state.track = Track(
                name: name,
                artist: artist,
                album: album,
                duration: duration,
                position: isNewTrack ? 0 : (state.track?.position ?? 0)
            )

            if isNewTrack {
                lastPositionUpdate = Date()
                if sixelSupported {
                    let key = "\(artist)\t\(album)"
                    fetchArtworkAsync(key: key)
                }
            }
        }

        state.musicRunning = true
    }

    // MARK: - Auto-Advance Queue

    private func autoAdvanceQueue() {
        guard !state.queueTracks.isEmpty,
              state.queueSelected + 1 < state.queueTracks.count else { return }
        // Debounce: only advance if still stopped after a short delay.
        // This avoids cascading when playTrackInPlaylist triggers a brief "Stopped".
        autoAdvancePending = true
        refreshQueue.asyncAfter(deadline: .now() + 1.5) { [self] in
            guard autoAdvancePending else { return }
            autoAdvancePending = false
            let nextIdx = state.queueSelected + 1
            state.queueSelected = nextIdx
            let playlist = state.queuePlaylistName
            music.playTrackInPlaylist(playlist, trackIndex: nextIdx)
            let fresh = music.fetchFullState()
            stateLock.lock()
            pendingState = fresh
            stateLock.unlock()
        }
    }

    // MARK: - Helpers

    private func fireAndRefresh(_ action: @escaping @Sendable () -> Void) {
        refreshQueue.async { [self] in
            action()
            let fresh = music.fetchFullState()
            stateLock.lock()
            pendingState = fresh
            refreshInFlight = false
            stateLock.unlock()
        }
    }

    /// Compute the number of content rows available for the tab's list area.
    private func contentRows() -> Int {
        let size = terminal.getSize()
        // Chrome: top border(1) + title(1) + sep(1) + player(~7) + sep(1) + tab bar(1) + sep(1) + sep(1) + help(1) + bottom border(1) = ~16
        return max(3, size.height - 16)
    }

    private func artworkCacheKey(for track: Track?) -> String {
        guard let t = track else { return "" }
        return "\(t.artist)\t\(t.album)"
    }

    private func fetchArtworkAsync(key: String) {
        guard !key.isEmpty, !artworkInFlight else { return }
        if key == artworkCacheKey { return }
        artworkInFlight = true
        refreshQueue.async { [self] in
            let result = SixelArt.generateSixel(artRows: 7)
            stateLock.lock()
            if var art = result {
                art.key = key
                pendingArtwork = art
            } else {
                pendingArtwork = SixelArt.ArtworkCache(key: key)
            }
            artworkInFlight = false
            stateLock.unlock()
        }
    }
}
