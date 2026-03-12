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

enum AppMode: Sendable {
    case nowPlaying
    case library
    case playlistTracks
    case search
}

struct AppState: Sendable {
    var track: Track?
    var playerState: PlayerState = .stopped
    var volume: Int = 50
    var shuffleEnabled: Bool = false
    var repeatMode: RepeatMode = .off
    var mode: AppMode = .nowPlaying
    var playlists: [String] = []
    var selectedIndex: Int = 0
    var scrollOffset: Int = 0
    var searchQuery: String = ""
    var searchResults: [(name: String, artist: String, album: String)] = []
    var currentPlaylistName: String = ""
    var playlistTracks: [PlaylistTrack] = []
    var musicRunning: Bool = true
}

