import Foundation
import Darwin

final class App: @unchecked Sendable {
    private let terminal = Terminal()
    private let music = MusicController()
    private var state = AppState()
    private var running = true
    private var lastRefresh: Date = .distantPast
    private let refreshInterval: TimeInterval = 1.0
    private var refreshInFlight = false
    private let refreshQueue = DispatchQueue(label: "muse.refresh")
    private var pendingState: AppState?
    private let stateLock = NSLock()

    func run() {
        terminal.enableRawMode()

        defer {
            terminal.restoreMode()
            terminal.clearScreen()
        }

        signal(SIGINT, SIG_IGN)

        // Initial state fetch (blocking for first render)
        applyFresh(music.fetchFullState())

        // Pre-load playlists so library tab is populated immediately
        refreshQueue.async { [self] in
            let playlists = music.getPlaylists()
            stateLock.lock()
            state.playlists = playlists
            stateLock.unlock()
        }

        render()

        while running {
            if let key = terminal.readKey() {
                handleKey(key)
                render()
            }

            // Apply async refresh result if available
            stateLock.lock()
            if let fresh = pendingState {
                pendingState = nil
                stateLock.unlock()
                applyFresh(fresh)
                render()
            } else {
                stateLock.unlock()
            }

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
        state.musicRunning = fresh.musicRunning
        state.playerState = fresh.playerState
        state.track = fresh.track
        state.volume = fresh.volume
        state.shuffleEnabled = fresh.shuffleEnabled
        state.repeatMode = fresh.repeatMode
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
        }
    }

    /// Returns true if the key was consumed globally.
    private func handleGlobalKey(_ key: Key) -> Bool {
        let inSearch = state.activeTab == .search

        switch key {
        case .character("q"):
            running = false
            return true
        case .character("1"):
            state.activeTab = .queue
            return true
        case .character("2"):
            if !inSearch {
                state.activeTab = .library
                return true
            }
        case .character("l"):
            if !inSearch {
                state.activeTab = .library
                return true
            }
        case .character("3"):
            state.activeTab = .search
            return true
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
        case .escape:
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
        case .escape:
            state.searchQuery = ""
            state.searchResults = []
            state.searchSelected = 0
            state.searchScroll = 0
        case .backspace:
            if !state.searchQuery.isEmpty {
                state.searchQuery.removeLast()
                performSearch()
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

    // MARK: - Render

    private func render() {
        let size = terminal.getSize()
        var screen = Screen()
        screen.renderMain(state: state, width: size.width, height: size.height)
        screen.flush(to: terminal)
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
}
