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
        let script = """
        tell application "Music"
            set results to (search playlist "Library" for "\(escapedName)")
            if (count of results) > 0 then
                play playlist "Library"
                set idx to index of item 1 of results
                play track idx of playlist "Library"
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
            return ps & "||" & vol & "||" & sh & "||" & sr & "||" & tn & "||" & ta & "||" & tal & "||" & td & "||" & tp
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
            }
        }
        return state
    }
}
