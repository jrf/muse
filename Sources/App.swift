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

        // Install signal handler for clean exit
        signal(SIGINT, SIG_IGN)

        // Initial state fetch (blocking for first render)
        applyFresh(music.fetchFullState())
        render()

        while running {
            // Handle input
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

    private func handleKey(_ key: Key) {
        switch state.mode {
        case .nowPlaying:
            handleNowPlayingKey(key)
        case .library:
            handleLibraryKey(key)
        case .playlistTracks:
            handlePlaylistTracksKey(key)
        case .search:
            handleSearchKey(key)
        }
    }

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

    private func handleNowPlayingKey(_ key: Key) {
        switch key {
        case .character("q"):
            running = false
        case .space:
            // Optimistic toggle
            state.playerState = state.playerState == .playing ? .paused : .playing
            fireAndRefresh { [self] in music.playPause() }
        case .character("n"):
            fireAndRefresh { [self] in music.nextTrack() }
        case .character("p"):
            fireAndRefresh { [self] in music.previousTrack() }
        case .character("+"), .character("="):
            state.volume = min(100, state.volume + 5)
            let vol = state.volume
            refreshQueue.async { [self] in music.setVolume(vol) }
        case .character("-"):
            state.volume = max(0, state.volume - 5)
            let vol = state.volume
            refreshQueue.async { [self] in music.setVolume(vol) }
        case .character("s"):
            state.shuffleEnabled.toggle()
            fireAndRefresh { [self] in music.toggleShuffle() }
        case .character("r"):
            state.repeatMode = state.repeatMode.next
            fireAndRefresh { [self] in music.cycleRepeat() }
        case .character("l"):
            state.mode = .library
            state.selectedIndex = 0
            state.scrollOffset = 0
            state.playlists = [] // show empty while loading
            refreshQueue.async { [self] in
                let playlists = music.getPlaylists()
                stateLock.lock()
                state.playlists = playlists
                stateLock.unlock()
            }
        case .character("/"):
            state.mode = .search
            state.searchQuery = ""
            state.searchResults = []
            state.selectedIndex = 0
            state.scrollOffset = 0
        default:
            break
        }
    }

    private func handleLibraryKey(_ key: Key) {
        switch key {
        case .escape, .character("q"):
            state.mode = .nowPlaying
        case .up:
            if state.selectedIndex > 0 {
                state.selectedIndex -= 1
                if state.selectedIndex < state.scrollOffset {
                    state.scrollOffset = state.selectedIndex
                }
            }
        case .down:
            if state.selectedIndex < state.playlists.count - 1 {
                state.selectedIndex += 1
                let size = terminal.getSize()
                let maxVisible = min(size.height - 10, 15)
                if state.selectedIndex >= state.scrollOffset + maxVisible {
                    state.scrollOffset = state.selectedIndex - maxVisible + 1
                }
            }
        case .enter:
            if !state.playlists.isEmpty && state.selectedIndex < state.playlists.count {
                let name = state.playlists[state.selectedIndex]
                state.currentPlaylistName = name
                state.playlistTracks = [] // show empty while loading
                state.mode = .playlistTracks
                state.selectedIndex = 0
                state.scrollOffset = 0
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

    private func handlePlaylistTracksKey(_ key: Key) {
        switch key {
        case .escape:
            // Go back to playlist list, restore selection
            state.mode = .library
            state.selectedIndex = state.playlists.firstIndex(of: state.currentPlaylistName) ?? 0
            state.scrollOffset = max(0, state.selectedIndex - 5)
        case .character("q"):
            state.mode = .nowPlaying
        case .up:
            if state.selectedIndex > 0 {
                state.selectedIndex -= 1
                if state.selectedIndex < state.scrollOffset {
                    state.scrollOffset = state.selectedIndex
                }
            }
        case .down:
            if state.selectedIndex < state.playlistTracks.count - 1 {
                state.selectedIndex += 1
                let size = terminal.getSize()
                let maxVisible = min(size.height - 10, 15)
                if state.selectedIndex >= state.scrollOffset + maxVisible {
                    state.scrollOffset = state.selectedIndex - maxVisible + 1
                }
            }
        case .enter:
            if !state.playlistTracks.isEmpty && state.selectedIndex < state.playlistTracks.count {
                let playlist = state.currentPlaylistName
                let idx = state.selectedIndex
                state.mode = .nowPlaying
                fireAndRefresh { [self] in music.playTrackInPlaylist(playlist, trackIndex: idx) }
            }
        case .space:
            let playlist = state.currentPlaylistName
            state.mode = .nowPlaying
            fireAndRefresh { [self] in music.playPlaylist(playlist) }
        default:
            break
        }
    }

    private func handleSearchKey(_ key: Key) {
        switch key {
        case .escape:
            state.mode = .nowPlaying
        case .backspace:
            if !state.searchQuery.isEmpty {
                state.searchQuery.removeLast()
                performSearch()
            }
        case .up:
            if state.selectedIndex > 0 {
                state.selectedIndex -= 1
                if state.selectedIndex < state.scrollOffset {
                    state.scrollOffset = state.selectedIndex
                }
            }
        case .down:
            if state.selectedIndex < state.searchResults.count - 1 {
                state.selectedIndex += 1
                let size = terminal.getSize()
                let maxVisible = min(size.height - 12, 12)
                if state.selectedIndex >= state.scrollOffset + maxVisible {
                    state.scrollOffset = state.selectedIndex - maxVisible + 1
                }
            }
        case .enter:
            if !state.searchResults.isEmpty && state.selectedIndex < state.searchResults.count {
                let result = state.searchResults[state.selectedIndex]
                state.mode = .nowPlaying
                fireAndRefresh { [self] in music.playTrack(result.name, artist: result.artist) }
            }
        case .character(let ch):
            state.searchQuery.append(ch)
            state.selectedIndex = 0
            state.scrollOffset = 0
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
            // Only apply if query hasn't changed while we were searching
            if state.searchQuery == query {
                state.searchResults = results
            }
            stateLock.unlock()
        }
    }

    private func render() {
        let size = terminal.getSize()
        var screen = Screen()

        switch state.mode {
        case .nowPlaying:
            screen.renderNowPlaying(state: state, width: size.width)
        case .library:
            screen.renderLibrary(
                playlists: state.playlists,
                selected: state.selectedIndex,
                scrollOffset: state.scrollOffset,
                width: size.width,
                height: size.height
            )
        case .playlistTracks:
            screen.renderPlaylistTracks(
                playlistName: state.currentPlaylistName,
                tracks: state.playlistTracks,
                selected: state.selectedIndex,
                scrollOffset: state.scrollOffset,
                width: size.width,
                height: size.height
            )
        case .search:
            screen.renderSearch(
                query: state.searchQuery,
                results: state.searchResults,
                selected: state.selectedIndex,
                scrollOffset: state.scrollOffset,
                width: size.width,
                height: size.height
            )
        }

        screen.flush(to: terminal)
    }
}
