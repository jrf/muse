import Foundation

struct Track: Sendable {
    var name: String
    var artist: String
    var album: String
    var duration: Double // seconds
    var position: Double // seconds
}

enum PlayerState: String, Sendable {
    case playing
    case paused
    case stopped
}

enum RepeatMode: Sendable {
    case off
    case all
    case one

    var label: String {
        switch self {
        case .off: return "off"
        case .all: return "all"
        case .one: return "one"
        }
    }

    var next: RepeatMode {
        switch self {
        case .off: return .all
        case .all: return .one
        case .one: return .off
        }
    }
}

struct PlaylistTrack: Sendable {
    var name: String
    var artist: String
    var album: String
    var duration: Double
}

enum Tab: Sendable {
    case queue, library, search, lyrics, themes
}

enum LibrarySubView: Sendable {
    case playlists
    case tracks(String)
}

struct AppState: Sendable {
    // Player
    var track: Track?
    var playerState: PlayerState = .stopped
    var volume: Int = 50
    var shuffleEnabled: Bool = false
    var repeatMode: RepeatMode = .off
    var musicRunning: Bool = true

    // Tabs
    var activeTab: Tab = .queue

    // Queue tab
    var queueTracks: [PlaylistTrack] = []
    var queueSelected: Int = 0
    var queueScroll: Int = 0
    var queuePlaylistName: String = ""

    // Library tab
    var playlists: [String] = []
    var librarySubView: LibrarySubView = .playlists
    var librarySelected: Int = 0
    var libraryScroll: Int = 0
    var playlistTracks: [PlaylistTrack] = []
    var playlistTracksSelected: Int = 0
    var playlistTracksScroll: Int = 0

    // Search tab
    var searchQuery: String = ""
    var searchResults: [(name: String, artist: String, album: String)] = []
    var searchSelected: Int = 0
    var searchScroll: Int = 0

    // Lyrics tab
    var lyricsText: String = ""
    var lyricsLines: [(time: Double?, text: String)] = []  // parsed lines with optional timestamp
    var lyricsSynced: Bool = false  // true if timestamps are available
    var lyricsScroll: Int = 0
    var lyricsManualScroll: Bool = false  // user overrode auto-scroll
    var lyricsTrackKey: String = ""

    // Themes tab
    var themeName: String = "synthwave"
    var themeSelected: Int = 0
    var themeScroll: Int = 0

    // Help overlay
    var showHelp: Bool = false

    // Favorite
    var currentTrackFavorited: Bool = false

    // Playlist picker overlay
    var showPlaylistPicker: Bool = false
    var playlistPickerSelected: Int = 0
    var playlistPickerScroll: Int = 0
}
