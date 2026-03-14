import AppKit
import Foundation

struct MusicController: Sendable {

    private func runAppleScript(_ script: String) -> String? {
        let appleScript = NSAppleScript(source: script)
        var error: NSDictionary?
        guard let result = appleScript?.executeAndReturnError(&error) else { return nil }
        let output = result.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines)
        return output?.isEmpty == true ? nil : output
    }

    func isRunning() -> Bool {
        let script = """
        tell application "System Events" to (name of processes) contains "Music"
        """
        return runAppleScript(script) == "true"
    }

    /// Launch Music.app hidden in the background if it isn't already running.
    func ensureRunning() {
        guard !isRunning() else { return }
        let script = """
        tell application "Music" to launch
        delay 1
        tell application "System Events"
            set visible of process "Music" to false
        end tell
        """
        _ = runAppleScript(script)
    }

    func playPause() {
        _ = runAppleScript(#"tell application "Music" to playpause"#)
    }

    func play() {
        _ = runAppleScript(#"tell application "Music" to play"#)
    }

    func pause() {
        _ = runAppleScript(#"tell application "Music" to pause"#)
    }

    func stop() {
        _ = runAppleScript(#"tell application "Music" to stop"#)
    }

    func nextTrack() {
        _ = runAppleScript(#"tell application "Music" to next track"#)
    }

    func previousTrack() {
        _ = runAppleScript(#"tell application "Music" to previous track"#)
    }

    func getCurrentTrack() -> Track? {
        let script = """
        tell application "Music"
            if player state is stopped then return "||STOPPED||"
            set t to current track
            set n to name of t
            set a to artist of t
            set al to album of t
            set d to duration of t
            set p to player position
            return n & "||" & a & "||" & al & "||" & d & "||" & p
        end tell
        """
        guard let result = runAppleScript(script), result != "||STOPPED||" else {
            return nil
        }
        let parts = result.components(separatedBy: "||")
        guard parts.count >= 5 else { return nil }
        return Track(
            name: parts[0],
            artist: parts[1],
            album: parts[2],
            duration: Double(parts[3]) ?? 0,
            position: Double(parts[4]) ?? 0
        )
    }

    func getPlayerState() -> PlayerState {
        let script = #"tell application "Music" to return player state as string"#
        guard let result = runAppleScript(script) else { return .stopped }
        switch result {
        case "playing": return .playing
        case "paused": return .paused
        default: return .stopped
        }
    }

    func getVolume() -> Int {
        let script = #"tell application "Music" to return sound volume"#
        guard let result = runAppleScript(script) else { return 50 }
        return Int(result) ?? 50
    }

    func setVolume(_ vol: Int) {
        let clamped = max(0, min(100, vol))
        _ = runAppleScript(#"tell application "Music" to set sound volume to \#(clamped)"#)
    }

    func getShuffleEnabled() -> Bool {
        let script = #"tell application "Music" to return shuffle enabled"#
        return runAppleScript(script) == "true"
    }

    func toggleShuffle() {
        let script = """
        tell application "Music"
            set shuffle enabled to not shuffle enabled
        end tell
        """
        _ = runAppleScript(script)
    }

    func getRepeatMode() -> RepeatMode {
        let script = #"tell application "Music" to return song repeat as string"#
        guard let result = runAppleScript(script) else { return .off }
        switch result {
        case "all": return .all
        case "one": return .one
        default: return .off
        }
    }

    func cycleRepeat() {
        let current = getRepeatMode()
        let next = current.next
        let value: String
        switch next {
        case .off: value = "off"
        case .all: value = "all"
        case .one: value = "one"
        }
        _ = runAppleScript(#"tell application "Music" to set song repeat to \#(value)"#)
    }

    func getPlaylists() -> [String] {
        let script = """
        tell application "Music"
            set playlistNames to name of every user playlist
            set output to ""
            repeat with p in playlistNames
                set output to output & p & "||"
            end repeat
            return output
        end tell
        """
        guard let result = runAppleScript(script) else { return [] }
        return result.components(separatedBy: "||").filter { !$0.isEmpty }
    }

    func playPlaylist(_ name: String) {
        let escaped = name.replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Music"
            play playlist "\(escaped)"
        end tell
        """
        _ = runAppleScript(script)
    }

    func searchTracks(_ query: String) -> [(name: String, artist: String, album: String)] {
        let escaped = query.replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Music"
            set results to (search playlist "Library" for "\(escaped)")
            set output to ""
            set maxResults to 20
            if (count of results) < maxResults then set maxResults to (count of results)
            repeat with i from 1 to maxResults
                set t to item i of results
                set output to output & name of t & "||" & artist of t & "||" & album of t & "\\n"
            end repeat
            return output
        end tell
        """
        guard let result = runAppleScript(script) else { return [] }
        return result.components(separatedBy: "\n").compactMap { line in
            let parts = line.components(separatedBy: "||")
            guard parts.count >= 3 else { return nil }
            let name = parts[0].trimmingCharacters(in: .whitespaces)
            guard !name.isEmpty else { return nil }
            return (name: name, artist: parts[1], album: parts[2])
        }
    }

    func getPlaylistTracks(_ playlistName: String) -> [PlaylistTrack] {
        let escaped = playlistName.replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Music"
            set pl to playlist "\(escaped)"
            set trks to every track of pl
            set output to ""
            set maxTracks to 50
            if (count of trks) < maxTracks then set maxTracks to (count of trks)
            repeat with i from 1 to maxTracks
                set t to item i of trks
                set output to output & name of t & "||" & artist of t & "||" & album of t & "||" & duration of t & "\\n"
            end repeat
            return output
        end tell
        """
        guard let result = runAppleScript(script) else { return [] }
        return result.components(separatedBy: "\n").compactMap { line in
            let parts = line.components(separatedBy: "||")
            guard parts.count >= 4 else { return nil }
            let name = parts[0].trimmingCharacters(in: .whitespaces)
            guard !name.isEmpty else { return nil }
            return PlaylistTrack(
                name: name,
                artist: parts[1],
                album: parts[2],
                duration: Double(parts[3].trimmingCharacters(in: .whitespaces)) ?? 0
            )
        }
    }

    func playTrackInPlaylist(_ playlistName: String, trackIndex: Int) {
        let escaped = playlistName.replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Music"
            play track \(trackIndex + 1) of playlist "\(escaped)"
        end tell
        """
        _ = runAppleScript(script)
    }

    func playTrack(_ name: String, artist: String) {
        let escapedName = name.replacingOccurrences(of: "\"", with: "\\\"")
        // For search results, just play the track directly.
        // This won't set up a queue, but skipping through a large library is too slow.
        let script = """
        tell application "Music"
            set results to (search playlist "Library" for "\(escapedName)")
            if (count of results) > 0 then
                play item 1 of results
            end if
        end tell
        """
        _ = runAppleScript(script)
    }

    struct LyricsResult: Sendable {
        var lines: [(time: Double?, text: String)]
        var synced: Bool
    }

    func getLyrics(trackName: String, artist: String) -> LyricsResult? {
        // First try embedded lyrics via AppleScript
        let script = """
        tell application "Music"
            if player state is stopped then return ""
            set t to current track
            try
                set ly to lyrics of t
                if ly is not missing value and ly is not "" then return ly
            end try
            return ""
        end tell
        """
        if let embedded = runAppleScript(script), !embedded.isEmpty {
            let lines = embedded.components(separatedBy: "\n").map { (time: nil as Double?, text: $0) }
            return LyricsResult(lines: lines, synced: false)
        }

        // Fall back to LRCLIB API
        guard !trackName.isEmpty, !artist.isEmpty else { return nil }
        var components = URLComponents(string: "https://lrclib.net/api/get")!
        components.queryItems = [
            URLQueryItem(name: "track_name", value: trackName),
            URLQueryItem(name: "artist_name", value: artist),
        ]
        guard let url = components.url else { return nil }
        var request = URLRequest(url: url)
        request.timeoutInterval = 5
        request.setValue("muse-tui/1.0", forHTTPHeaderField: "User-Agent")

        guard let (data, _) = try? synchronousDataTask(request) else { return nil }
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }

        // Prefer synced lyrics (has timestamps for auto-scroll)
        if let synced = json["syncedLyrics"] as? String, !synced.isEmpty {
            let lines = parseSyncedLyrics(synced)
            return LyricsResult(lines: lines, synced: true)
        } else if let plain = json["plainLyrics"] as? String, !plain.isEmpty {
            let lines = plain.components(separatedBy: "\n").map { (time: nil as Double?, text: $0) }
            return LyricsResult(lines: lines, synced: false)
        }
        return nil
    }

    private func parseSyncedLyrics(_ raw: String) -> [(time: Double?, text: String)] {
        return raw.components(separatedBy: "\n").map { line in
            // Format: [mm:ss.xx] text
            guard line.hasPrefix("["), let bracket = line.firstIndex(of: "]") else {
                return (time: nil, text: line)
            }
            let timeStr = String(line[line.index(after: line.startIndex)..<bracket])
            let after = line.index(after: bracket)
            let text = String(line[after...]).trimmingCharacters(in: .whitespaces)
            // Parse mm:ss.xx
            let parts = timeStr.split(separator: ":")
            guard parts.count == 2,
                  let minutes = Double(parts[0]),
                  let seconds = Double(parts[1]) else {
                return (time: nil, text: text)
            }
            return (time: minutes * 60.0 + seconds, text: text)
        }
    }

    private func synchronousDataTask(_ request: URLRequest) throws -> (Data, URLResponse) {
        let semaphore = DispatchSemaphore(value: 0)
        nonisolated(unsafe) var resultData: Data?
        nonisolated(unsafe) var resultResponse: URLResponse?
        nonisolated(unsafe) var resultError: Error?
        URLSession.shared.dataTask(with: request) { data, response, error in
            resultData = data
            resultResponse = response
            resultError = error
            semaphore.signal()
        }.resume()
        semaphore.wait()
        if let error = resultError { throw error }
        return (resultData ?? Data(), resultResponse!)
    }

    func isTrackFavorited() -> Bool {
        let script = """
        tell application "Music"
            if player state is stopped then return "false"
            try
                return loved of current track as string
            end try
            return "false"
        end tell
        """
        return runAppleScript(script) == "true"
    }

    func toggleFavorite() {
        let script = """
        tell application "Music"
            if player state is not stopped then
                try
                    set loved of current track to not (loved of current track)
                end try
            end if
        end tell
        """
        _ = runAppleScript(script)
    }

    func revealArtistInMusic(_ artist: String) {
        if let url = iTunesSearchURL(term: artist, entity: "musicArtist") {
            NSWorkspace.shared.open(url)
        } else {
            revealCurrentTrack()
        }
    }

    func revealAlbumInMusic(album: String, artist: String) {
        if let url = iTunesSearchURL(term: "\(album) \(artist)", entity: "album") {
            NSWorkspace.shared.open(url)
        } else {
            revealCurrentTrack()
        }
    }

    private func iTunesSearchURL(term: String, entity: String) -> URL? {
        guard let encoded = term.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed),
              let searchURL = URL(string: "https://itunes.apple.com/search?term=\(encoded)&entity=\(entity)&limit=1"),
              let (data, _) = try? synchronousDataTask(URLRequest(url: searchURL)),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let results = json["results"] as? [[String: Any]],
              let first = results.first else { return nil }
        let urlKey = entity == "musicArtist" ? "artistLinkUrl" : "collectionViewUrl"
        guard let urlString = first[urlKey] as? String,
              var components = URLComponents(string: urlString) else { return nil }
        // Strip affiliate query params so Music.app opens the full page
        components.queryItems = nil
        return components.url
    }

    private func revealCurrentTrack() {
        let script = """
        tell application "Music"
            reveal current track
            activate
        end tell
        """
        _ = runAppleScript(script)
    }

    func addCurrentTrackToPlaylist(_ name: String) {
        let escaped = name.replacingOccurrences(of: "\"", with: "\\\"")
        let script = """
        tell application "Music"
            if player state is not stopped then
                duplicate current track to playlist "\(escaped)"
            end if
        end tell
        """
        _ = runAppleScript(script)
    }

    func fetchFullState() -> AppState {
        var state = AppState()
        // Single osascript call to get everything at once
        let script = """
        tell application "System Events"
            if not ((name of processes) contains "Music") then return "NOT_RUNNING"
        end tell
        tell application "Music"
            set ps to player state as string
            set vol to sound volume
            set sh to shuffle enabled
            set sr to song repeat as string
            if ps is "stopped" then
                return ps & "||" & vol & "||" & sh & "||" & sr & "||||||"
            end if
            set t to current track
            set tn to name of t
            set ta to artist of t
            set tal to album of t
            set td to duration of t
            set tp to player position
            set tl to false
            try
                set tl to loved of t
            end try
            return ps & "||" & vol & "||" & sh & "||" & sr & "||" & tn & "||" & ta & "||" & tal & "||" & td & "||" & tp & "||" & tl
        end tell
        """
        guard let result = runAppleScript(script) else {
            state.musicRunning = false
            return state
        }
        if result == "NOT_RUNNING" {
            state.musicRunning = false
            return state
        }
        state.musicRunning = true
        let parts = result.components(separatedBy: "||")
        guard parts.count >= 4 else { return state }

        switch parts[0] {
        case "playing": state.playerState = .playing
        case "paused": state.playerState = .paused
        default: state.playerState = .stopped
        }
        state.volume = Int(parts[1]) ?? 50
        state.shuffleEnabled = parts[2] == "true"
        switch parts[3] {
        case "all": state.repeatMode = .all
        case "one": state.repeatMode = .one
        default: state.repeatMode = .off
        }
        if parts.count >= 9 {
            let name = parts[4]
            if !name.isEmpty {
                state.track = Track(
                    name: name,
                    artist: parts[5],
                    album: parts[6],
                    duration: Double(parts[7]) ?? 0,
                    position: Double(parts[8]) ?? 0
                )
                if parts.count >= 10 {
                    state.currentTrackFavorited = parts[9] == "true"
                }
            }
        }
        return state
    }
}
