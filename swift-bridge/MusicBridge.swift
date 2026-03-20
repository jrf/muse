import AppKit
import Foundation

// MARK: - Internal helpers

private func makeCString(_ s: String) -> UnsafeMutablePointer<CChar> {
    return strdup(s)!
}

private func runAppleScript(_ script: String) -> String? {
    let appleScript = NSAppleScript(source: script)
    var error: NSDictionary?
    guard let result = appleScript?.executeAndReturnError(&error) else { return nil }
    let output = result.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines)
    return output?.isEmpty == true ? nil : output
}

// MARK: - Opaque types (heap-allocated, passed as raw pointers)

private class PlayerStateBox {
    var musicRunning: Bool = false
    var playerState: Int32 = 0  // 0=stopped, 1=playing, 2=paused
    var volume: Int32 = 50
    var shuffleEnabled: Bool = false
    var repeatMode: Int32 = 0   // 0=off, 1=all, 2=one
    var hasTrack: Bool = false
    var trackName: String = ""
    var trackArtist: String = ""
    var trackAlbum: String = ""
    var trackDuration: Double = 0
    var trackPosition: Double = 0
    var trackFavorited: Bool = false
}

private class StringArrayBox {
    var items: [String]
    init(_ items: [String]) { self.items = items }
}

private class PlaylistTrackArrayBox {
    var items: [(name: String, artist: String, album: String, duration: Double)]
    init(_ items: [(name: String, artist: String, album: String, duration: Double)]) { self.items = items }
}

private class SearchResultArrayBox {
    var items: [(name: String, artist: String, album: String)]
    init(_ items: [(name: String, artist: String, album: String)]) { self.items = items }
}

private class LyricsBox {
    var lines: [(text: String, time: Double?, hasTime: Bool)] = []
    var synced: Bool = false
}

private class NotificationInfoBox {
    var playerState: String = ""
    var name: String = ""
    var artist: String = ""
    var album: String = ""
    var totalTimeMs: Double = 0
}

// MARK: - Exported Functions

@_cdecl("music_free_string")
public func music_free_string(_ ptr: UnsafeMutablePointer<CChar>?) {
    if let ptr = ptr { free(ptr) }
}

@_cdecl("music_is_running")
public func music_is_running() -> Bool {
    // Avoid System Events (requires separate Automation permission).
    // Instead, check if Music responds to a simple query.
    let script = """
    try
        tell application "Music" to return running
    on error
        return false
    end try
    """
    return runAppleScript(script) == "true"
}

@_cdecl("music_ensure_running")
public func music_ensure_running() {
    let script = """
    tell application "Music" to launch
    """
    _ = runAppleScript(script)
}

@_cdecl("music_play_pause")
public func music_play_pause() {
    _ = runAppleScript(#"tell application "Music" to playpause"#)
}

@_cdecl("music_next_track")
public func music_next_track() {
    _ = runAppleScript(#"tell application "Music" to next track"#)
}

@_cdecl("music_previous_track")
public func music_previous_track() {
    _ = runAppleScript(#"tell application "Music" to previous track"#)
}

@_cdecl("music_get_volume")
public func music_get_volume() -> Int32 {
    let script = #"tell application "Music" to return sound volume"#
    guard let result = runAppleScript(script) else { return 50 }
    return Int32(result) ?? 50
}

@_cdecl("music_set_volume")
public func music_set_volume(_ vol: Int32) {
    let clamped = max(0, min(100, vol))
    _ = runAppleScript(#"tell application "Music" to set sound volume to \#(clamped)"#)
}

@_cdecl("music_toggle_shuffle")
public func music_toggle_shuffle() {
    let script = """
    tell application "Music"
        set shuffle enabled to not shuffle enabled
    end tell
    """
    _ = runAppleScript(script)
}

@_cdecl("music_cycle_repeat")
public func music_cycle_repeat() {
    let script = #"tell application "Music" to return song repeat as string"#
    guard let result = runAppleScript(script) else { return }
    let next: String
    switch result {
    case "off": next = "all"
    case "all": next = "one"
    default: next = "off"
    }
    _ = runAppleScript(#"tell application "Music" to set song repeat to \#(next)"#)
}

@_cdecl("music_toggle_favorite")
public func music_toggle_favorite() {
    // Read current state first, then set explicitly
    let readScript = """
    tell application "Music"
        if player state is not stopped then
            try
                return loved of current track
            end try
        end if
        return false
    end tell
    """
    let isLoved = runAppleScript(readScript) == "true"
    let newValue = isLoved ? "false" : "true"
    let writeScript = """
    tell application "Music"
        if player state is not stopped then
            set loved of current track to \(newValue)
        end if
    end tell
    """
    _ = runAppleScript(writeScript)
}

// MARK: - Full State (opaque pointer pattern)

@_cdecl("music_fetch_state")
public func music_fetch_state() -> UnsafeMutableRawPointer {
    let box_ = PlayerStateBox()

    let script = """
    tell application "Music"
        if not running then return "NOT_RUNNING"
        set ps to player state as string
        set vol to sound volume
        set sh to shuffle enabled
        set sr to song repeat as string
        if ps is "stopped" then
            return ps & "||" & vol & "||" & sh & "||" & sr & "||||||"
        end if
        try
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
        on error
            -- Track info unavailable (e.g. during track change), but Music is running
            return ps & "||" & vol & "||" & sh & "||" & sr & "||||||"
        end try
    end tell
    """
    guard let result = runAppleScript(script) else {
        // NSAppleScript failed entirely — Music may not be running
        return Unmanaged.passRetained(box_).toOpaque()
    }
    if result == "NOT_RUNNING" {
        return Unmanaged.passRetained(box_).toOpaque()
    }

    box_.musicRunning = true
    let parts = result.components(separatedBy: "||")
    guard parts.count >= 4 else { return Unmanaged.passRetained(box_).toOpaque() }

    switch parts[0] {
    case "playing": box_.playerState = 1
    case "paused": box_.playerState = 2
    default: box_.playerState = 0
    }
    box_.volume = Int32(parts[1]) ?? 50
    box_.shuffleEnabled = parts[2] == "true"
    switch parts[3] {
    case "all": box_.repeatMode = 1
    case "one": box_.repeatMode = 2
    default: box_.repeatMode = 0
    }

    if parts.count >= 9 {
        let name = parts[4]
        if !name.isEmpty {
            box_.hasTrack = true
            box_.trackName = name
            box_.trackArtist = parts[5]
            box_.trackAlbum = parts[6]
            box_.trackDuration = Double(parts[7]) ?? 0
            box_.trackPosition = Double(parts[8]) ?? 0
            if parts.count >= 10 {
                box_.trackFavorited = parts[9] == "true"
            }
        }
    }
    return Unmanaged.passRetained(box_).toOpaque()
}

@_cdecl("music_state_free")
public func music_state_free(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).release()
}

@_cdecl("music_state_music_running")
public func music_state_music_running(_ ptr: UnsafeRawPointer) -> Bool {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().musicRunning
}

@_cdecl("music_state_player_state")
public func music_state_player_state(_ ptr: UnsafeRawPointer) -> Int32 {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().playerState
}

@_cdecl("music_state_volume")
public func music_state_volume(_ ptr: UnsafeRawPointer) -> Int32 {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().volume
}

@_cdecl("music_state_shuffle_enabled")
public func music_state_shuffle_enabled(_ ptr: UnsafeRawPointer) -> Bool {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().shuffleEnabled
}

@_cdecl("music_state_repeat_mode")
public func music_state_repeat_mode(_ ptr: UnsafeRawPointer) -> Int32 {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().repeatMode
}

@_cdecl("music_state_has_track")
public func music_state_has_track(_ ptr: UnsafeRawPointer) -> Bool {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().hasTrack
}

@_cdecl("music_state_track_name")
public func music_state_track_name(_ ptr: UnsafeRawPointer) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().trackName)
}

@_cdecl("music_state_track_artist")
public func music_state_track_artist(_ ptr: UnsafeRawPointer) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().trackArtist)
}

@_cdecl("music_state_track_album")
public func music_state_track_album(_ ptr: UnsafeRawPointer) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().trackAlbum)
}

@_cdecl("music_state_track_duration")
public func music_state_track_duration(_ ptr: UnsafeRawPointer) -> Double {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().trackDuration
}

@_cdecl("music_state_track_position")
public func music_state_track_position(_ ptr: UnsafeRawPointer) -> Double {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().trackPosition
}

@_cdecl("music_state_track_favorited")
public func music_state_track_favorited(_ ptr: UnsafeRawPointer) -> Bool {
    Unmanaged<PlayerStateBox>.fromOpaque(ptr).takeUnretainedValue().trackFavorited
}

// MARK: - Playlists (opaque pointer pattern)

@_cdecl("music_get_playlists")
public func music_get_playlists() -> UnsafeMutableRawPointer {
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
    guard let result = runAppleScript(script) else {
        return Unmanaged.passRetained(StringArrayBox([])).toOpaque()
    }
    let names = result.components(separatedBy: "||").filter { !$0.isEmpty }
    return Unmanaged.passRetained(StringArrayBox(names)).toOpaque()
}

@_cdecl("music_string_array_count")
public func music_string_array_count(_ ptr: UnsafeRawPointer) -> Int32 {
    Int32(Unmanaged<StringArrayBox>.fromOpaque(ptr).takeUnretainedValue().items.count)
}

@_cdecl("music_string_array_get")
public func music_string_array_get(_ ptr: UnsafeRawPointer, _ index: Int32) -> UnsafeMutablePointer<CChar> {
    let arr = Unmanaged<StringArrayBox>.fromOpaque(ptr).takeUnretainedValue()
    return makeCString(arr.items[Int(index)])
}

@_cdecl("music_string_array_free")
public func music_string_array_free(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<StringArrayBox>.fromOpaque(ptr).release()
}

@_cdecl("music_play_playlist")
public func music_play_playlist(_ name: UnsafePointer<CChar>) {
    let name = String(cString: name)
    let escaped = name.replacingOccurrences(of: "\"", with: "\\\"")
    let script = """
    tell application "Music"
        play playlist "\(escaped)"
    end tell
    """
    _ = runAppleScript(script)
}

// MARK: - Playlist Tracks (opaque pointer pattern)

@_cdecl("music_get_playlist_tracks_bulk")
public func music_get_playlist_tracks_bulk(_ name: UnsafePointer<CChar>) -> UnsafeMutableRawPointer {
    let playlistName = String(cString: name)
    let escaped = playlistName.replacingOccurrences(of: "\"", with: "\\\"")
    // Bulk property access: each `name of every track` is a single Apple Event,
    // orders of magnitude faster than iterating track-by-track.
    // Results are joined using text item delimiters (fast, built-in).
    let script = """
    tell application "Music"
        set pl to playlist "\(escaped)"
        set allNames to name of every track of pl
        set allArtists to artist of every track of pl
        set allAlbums to album of every track of pl
        set allDurations to duration of every track of pl
        set tid to AppleScript's text item delimiters
        set AppleScript's text item delimiters to "||"
        set nameStr to allNames as text
        set artistStr to allArtists as text
        set albumStr to allAlbums as text
        -- Build duration string without a slow repeat loop:
        -- coerce the list to text directly via a handler
        set AppleScript's text item delimiters to ", "
        set durFlat to allDurations as text
        set AppleScript's text item delimiters to ", "
        set durItems to text items of durFlat
        set AppleScript's text item delimiters to "||"
        set durStr to durItems as text
        set AppleScript's text item delimiters to tid
        return nameStr & "\\n" & artistStr & "\\n" & albumStr & "\\n" & durStr
    end tell
    """
    guard let result = runAppleScript(script) else {
        return Unmanaged.passRetained(PlaylistTrackArrayBox([])).toOpaque()
    }
    let lines = result.components(separatedBy: "\n")
    guard lines.count >= 4 else {
        return Unmanaged.passRetained(PlaylistTrackArrayBox([])).toOpaque()
    }
    let names = lines[0].components(separatedBy: "||")
    let artists = lines[1].components(separatedBy: "||")
    let albums = lines[2].components(separatedBy: "||")
    let durations = lines[3].components(separatedBy: "||")
    let count = names.count
    var tracks: [(String, String, String, Double)] = []
    tracks.reserveCapacity(count)
    for i in 0..<count {
        let name = names[i].trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { continue }
        let artist = i < artists.count ? artists[i] : ""
        let album = i < albums.count ? albums[i] : ""
        let dur = i < durations.count ? (Double(durations[i].trimmingCharacters(in: .whitespaces)) ?? 0) : 0
        tracks.append((name, artist, album, dur))
    }
    return Unmanaged.passRetained(PlaylistTrackArrayBox(tracks)).toOpaque()
}

@_cdecl("music_playlist_tracks_count")
public func music_playlist_tracks_count(_ ptr: UnsafeRawPointer) -> Int32 {
    Int32(Unmanaged<PlaylistTrackArrayBox>.fromOpaque(ptr).takeUnretainedValue().items.count)
}

@_cdecl("music_playlist_tracks_name")
public func music_playlist_tracks_name(_ ptr: UnsafeRawPointer, _ index: Int32) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<PlaylistTrackArrayBox>.fromOpaque(ptr).takeUnretainedValue().items[Int(index)].name)
}

@_cdecl("music_playlist_tracks_artist")
public func music_playlist_tracks_artist(_ ptr: UnsafeRawPointer, _ index: Int32) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<PlaylistTrackArrayBox>.fromOpaque(ptr).takeUnretainedValue().items[Int(index)].artist)
}

@_cdecl("music_playlist_tracks_album")
public func music_playlist_tracks_album(_ ptr: UnsafeRawPointer, _ index: Int32) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<PlaylistTrackArrayBox>.fromOpaque(ptr).takeUnretainedValue().items[Int(index)].album)
}

@_cdecl("music_playlist_tracks_duration")
public func music_playlist_tracks_duration(_ ptr: UnsafeRawPointer, _ index: Int32) -> Double {
    Unmanaged<PlaylistTrackArrayBox>.fromOpaque(ptr).takeUnretainedValue().items[Int(index)].duration
}

@_cdecl("music_playlist_tracks_free")
public func music_playlist_tracks_free(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<PlaylistTrackArrayBox>.fromOpaque(ptr).release()
}

@_cdecl("music_play_track_in_playlist")
public func music_play_track_in_playlist(_ name: UnsafePointer<CChar>, _ index: Int32) {
    let playlistName = String(cString: name)
    let escaped = playlistName.replacingOccurrences(of: "\"", with: "\\\"")
    let script = """
    tell application "Music"
        play track \(index + 1) of playlist "\(escaped)"
    end tell
    """
    _ = runAppleScript(script)
}

// MARK: - Search (opaque pointer pattern)

@_cdecl("music_search")
public func music_search(_ query: UnsafePointer<CChar>) -> UnsafeMutableRawPointer {
    let queryStr = String(cString: query)
    let escaped = queryStr.replacingOccurrences(of: "\"", with: "\\\"")
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
    guard let result = runAppleScript(script) else {
        return Unmanaged.passRetained(SearchResultArrayBox([])).toOpaque()
    }
    let items = result.components(separatedBy: "\n").compactMap { line -> (String, String, String)? in
        let parts = line.components(separatedBy: "||")
        guard parts.count >= 3 else { return nil }
        let name = parts[0].trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return nil }
        return (name, parts[1], parts[2])
    }
    return Unmanaged.passRetained(SearchResultArrayBox(items)).toOpaque()
}

@_cdecl("music_search_count")
public func music_search_count(_ ptr: UnsafeRawPointer) -> Int32 {
    Int32(Unmanaged<SearchResultArrayBox>.fromOpaque(ptr).takeUnretainedValue().items.count)
}

@_cdecl("music_search_name")
public func music_search_name(_ ptr: UnsafeRawPointer, _ index: Int32) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<SearchResultArrayBox>.fromOpaque(ptr).takeUnretainedValue().items[Int(index)].name)
}

@_cdecl("music_search_artist")
public func music_search_artist(_ ptr: UnsafeRawPointer, _ index: Int32) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<SearchResultArrayBox>.fromOpaque(ptr).takeUnretainedValue().items[Int(index)].artist)
}

@_cdecl("music_search_album")
public func music_search_album(_ ptr: UnsafeRawPointer, _ index: Int32) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<SearchResultArrayBox>.fromOpaque(ptr).takeUnretainedValue().items[Int(index)].album)
}

@_cdecl("music_search_free")
public func music_search_free(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<SearchResultArrayBox>.fromOpaque(ptr).release()
}

@_cdecl("music_play_track")
public func music_play_track(_ name: UnsafePointer<CChar>, _ artist: UnsafePointer<CChar>) {
    let nameStr = String(cString: name)
    let artistStr = String(cString: artist)
    let escapedName = nameStr.replacingOccurrences(of: "\"", with: "\\\"")
    let escapedArtist = artistStr.replacingOccurrences(of: "\"", with: "\\\"")
    let script = """
    tell application "Music"
        set results to (search playlist "Library" for "\(escapedName)")
        repeat with t in results
            if name of t is "\(escapedName)" and artist of t is "\(escapedArtist)" then
                play t
                return
            end if
        end repeat
        -- Fallback: exact name match (any artist)
        repeat with t in results
            if name of t is "\(escapedName)" then
                play t
                return
            end if
        end repeat
        -- Last resort: play first search hit
        if (count of results) > 0 then
            play item 1 of results
        end if
    end tell
    """
    _ = runAppleScript(script)
}

// MARK: - Lyrics (opaque pointer pattern)

@_cdecl("music_get_lyrics")
public func music_get_lyrics(_ trackName: UnsafePointer<CChar>, _ artist: UnsafePointer<CChar>) -> UnsafeMutableRawPointer? {
    let name = String(cString: trackName)
    let artistStr = String(cString: artist)

    let box_ = LyricsBox()

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
        box_.lines = embedded.components(separatedBy: "\n").map { (text: $0, time: nil, hasTime: false) }
        box_.synced = false
        return Unmanaged.passRetained(box_).toOpaque()
    }

    // Fall back to LRCLIB API
    guard !name.isEmpty, !artistStr.isEmpty else { return nil }
    var components = URLComponents(string: "https://lrclib.net/api/get")!
    components.queryItems = [
        URLQueryItem(name: "track_name", value: name),
        URLQueryItem(name: "artist_name", value: artistStr),
    ]
    guard let url = components.url else { return nil }
    var request = URLRequest(url: url)
    request.timeoutInterval = 5
    request.setValue("muse-tui/1.0", forHTTPHeaderField: "User-Agent")

    let semaphore = DispatchSemaphore(value: 0)
    var responseData: Data?
    URLSession.shared.dataTask(with: request) { data, _, _ in
        responseData = data
        semaphore.signal()
    }.resume()
    semaphore.wait()

    guard let data = responseData,
          let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }

    if let synced = json["syncedLyrics"] as? String, !synced.isEmpty {
        box_.lines = parseSyncedLyrics(synced)
        box_.synced = true
        return Unmanaged.passRetained(box_).toOpaque()
    } else if let plain = json["plainLyrics"] as? String, !plain.isEmpty {
        box_.lines = plain.components(separatedBy: "\n").map { (text: $0, time: nil, hasTime: false) }
        box_.synced = false
        return Unmanaged.passRetained(box_).toOpaque()
    }
    return nil
}

@_cdecl("music_lyrics_synced")
public func music_lyrics_synced(_ ptr: UnsafeRawPointer) -> Bool {
    Unmanaged<LyricsBox>.fromOpaque(ptr).takeUnretainedValue().synced
}

@_cdecl("music_lyrics_count")
public func music_lyrics_count(_ ptr: UnsafeRawPointer) -> Int32 {
    Int32(Unmanaged<LyricsBox>.fromOpaque(ptr).takeUnretainedValue().lines.count)
}

@_cdecl("music_lyrics_text")
public func music_lyrics_text(_ ptr: UnsafeRawPointer, _ index: Int32) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<LyricsBox>.fromOpaque(ptr).takeUnretainedValue().lines[Int(index)].text)
}

@_cdecl("music_lyrics_has_time")
public func music_lyrics_has_time(_ ptr: UnsafeRawPointer, _ index: Int32) -> Bool {
    Unmanaged<LyricsBox>.fromOpaque(ptr).takeUnretainedValue().lines[Int(index)].hasTime
}

@_cdecl("music_lyrics_time")
public func music_lyrics_time(_ ptr: UnsafeRawPointer, _ index: Int32) -> Double {
    Unmanaged<LyricsBox>.fromOpaque(ptr).takeUnretainedValue().lines[Int(index)].time ?? 0
}

@_cdecl("music_lyrics_free")
public func music_lyrics_free(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<LyricsBox>.fromOpaque(ptr).release()
}

private func parseSyncedLyrics(_ raw: String) -> [(text: String, time: Double?, hasTime: Bool)] {
    return raw.components(separatedBy: "\n").map { line in
        guard line.hasPrefix("["), let bracket = line.firstIndex(of: "]") else {
            return (text: line, time: nil, hasTime: false)
        }
        let timeStr = String(line[line.index(after: line.startIndex)..<bracket])
        let after = line.index(after: bracket)
        let text = String(line[after...]).trimmingCharacters(in: .whitespaces)
        let parts = timeStr.split(separator: ":")
        guard parts.count == 2,
              let minutes = Double(parts[0]),
              let seconds = Double(parts[1]) else {
            return (text: text, time: nil, hasTime: false)
        }
        return (text: text, time: minutes * 60.0 + seconds, hasTime: true)
    }
}

// MARK: - Open in Music.app

@_cdecl("music_reveal_artist")
public func music_reveal_artist(_ artist: UnsafePointer<CChar>) {
    let artistStr = String(cString: artist)
    if let url = iTunesSearchURL(term: artistStr, entity: "musicArtist") {
        NSWorkspace.shared.open(url)
    } else {
        revealCurrentTrack()
    }
}

@_cdecl("music_reveal_album")
public func music_reveal_album(_ album: UnsafePointer<CChar>, _ artist: UnsafePointer<CChar>) {
    let albumStr = String(cString: album)
    let artistStr = String(cString: artist)
    if let url = iTunesSearchURL(term: "\(albumStr) \(artistStr)", entity: "album") {
        NSWorkspace.shared.open(url)
    } else {
        revealCurrentTrack()
    }
}

@_cdecl("music_add_to_playlist")
public func music_add_to_playlist(_ name: UnsafePointer<CChar>) {
    let nameStr = String(cString: name)
    let escaped = nameStr.replacingOccurrences(of: "\"", with: "\\\"")
    let script = """
    tell application "Music"
        if player state is not stopped then
            duplicate current track to playlist "\(escaped)"
        end if
    end tell
    """
    _ = runAppleScript(script)
}

@_cdecl("music_remove_from_playlist")
public func music_remove_from_playlist(_ name: UnsafePointer<CChar>, _ index: Int32) {
    let nameStr = String(cString: name)
    let escaped = nameStr.replacingOccurrences(of: "\"", with: "\\\"")
    // AppleScript uses 1-based indexing
    let asIndex = index + 1
    let script = """
    tell application "Music"
        delete track \(asIndex) of playlist "\(escaped)"
    end tell
    """
    _ = runAppleScript(script)
}

private func iTunesSearchURL(term: String, entity: String) -> URL? {
    guard let encoded = term.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed),
          let searchURL = URL(string: "https://itunes.apple.com/search?term=\(encoded)&entity=\(entity)&limit=1") else { return nil }

    let semaphore = DispatchSemaphore(value: 0)
    var responseData: Data?
    URLSession.shared.dataTask(with: URLRequest(url: searchURL)) { data, _, _ in
        responseData = data
        semaphore.signal()
    }.resume()
    semaphore.wait()

    guard let data = responseData,
          let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let results = json["results"] as? [[String: Any]],
          let first = results.first else { return nil }
    let urlKey = entity == "musicArtist" ? "artistLinkUrl" : "collectionViewUrl"
    guard let urlString = first[urlKey] as? String,
          var components = URLComponents(string: urlString) else { return nil }
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

// MARK: - Notifications (simple callback with opaque pointer)

private var notificationCallback: (@convention(c) (UnsafeMutableRawPointer) -> Void)?
private var notificationObservers: [NSObjectProtocol] = []

@_cdecl("music_register_notification_callback")
public func music_register_notification_callback(_ cb: @convention(c) (UnsafeMutableRawPointer) -> Void) {
    notificationCallback = cb

    let nc = DistributedNotificationCenter.default()

    let handler = { (notification: Notification) in
        guard let info = notification.userInfo, let cb = notificationCallback else { return }

        let box_ = NotificationInfoBox()
        box_.playerState = info["Player State"] as? String ?? ""
        box_.name = info["Name"] as? String ?? ""
        box_.artist = info["Artist"] as? String ?? ""
        box_.album = info["Album"] as? String ?? ""
        box_.totalTimeMs = info["Total Time"] as? Double ?? 0

        let ptr = Unmanaged.passRetained(box_).toOpaque()
        cb(ptr)
    }

    let obs1 = nc.addObserver(forName: Notification.Name("com.apple.Music.playerInfo"),
                               object: nil, queue: nil, using: handler)
    let obs2 = nc.addObserver(forName: Notification.Name("com.apple.iTunes.playerInfo"),
                               object: nil, queue: nil, using: handler)
    notificationObservers = [obs1, obs2]
}

@_cdecl("music_notification_player_state")
public func music_notification_player_state(_ ptr: UnsafeRawPointer) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<NotificationInfoBox>.fromOpaque(ptr).takeUnretainedValue().playerState)
}

@_cdecl("music_notification_name")
public func music_notification_name(_ ptr: UnsafeRawPointer) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<NotificationInfoBox>.fromOpaque(ptr).takeUnretainedValue().name)
}

@_cdecl("music_notification_artist")
public func music_notification_artist(_ ptr: UnsafeRawPointer) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<NotificationInfoBox>.fromOpaque(ptr).takeUnretainedValue().artist)
}

@_cdecl("music_notification_album")
public func music_notification_album(_ ptr: UnsafeRawPointer) -> UnsafeMutablePointer<CChar> {
    makeCString(Unmanaged<NotificationInfoBox>.fromOpaque(ptr).takeUnretainedValue().album)
}

@_cdecl("music_notification_total_time_ms")
public func music_notification_total_time_ms(_ ptr: UnsafeRawPointer) -> Double {
    Unmanaged<NotificationInfoBox>.fromOpaque(ptr).takeUnretainedValue().totalTimeMs
}

@_cdecl("music_notification_free")
public func music_notification_free(_ ptr: UnsafeMutableRawPointer) {
    Unmanaged<NotificationInfoBox>.fromOpaque(ptr).release()
}

/// Pump the current thread's RunLoop briefly so DistributedNotificationCenter can deliver.
@_cdecl("music_pump_runloop")
public func music_pump_runloop() {
    RunLoop.current.run(mode: .default, before: Date())
}

// MARK: - Artwork

@_cdecl("music_get_artwork_data")
public func music_get_artwork_data(_ out_len: UnsafeMutablePointer<Int32>) -> UnsafeMutablePointer<UInt8>? {
    // Write artwork to a temp file via AppleScript, then read it back.
    // Using `raw data of artwork` directly through NSAppleEventDescriptor.data
    // can fail when called from a static library / background thread context.
    let tmpPath = NSTemporaryDirectory() + "muse-artwork.dat"
    let script = """
    tell application "Music"
        if player state is not stopped then
            try
                set artData to raw data of artwork 1 of current track
                set tmpFile to POSIX file "\(tmpPath)"
                set fRef to open for access tmpFile with write permission
                set eof fRef to 0
                write artData to fRef
                close access fRef
                return "ok"
            on error
                try
                    close access tmpFile
                end try
                return ""
            end try
        end if
        return ""
    end tell
    """
    guard let result = runAppleScript(script), result == "ok" else {
        out_len.pointee = 0
        return nil
    }
    guard let data = try? Data(contentsOf: URL(fileURLWithPath: tmpPath)), !data.isEmpty else {
        out_len.pointee = 0
        return nil
    }
    // Clean up temp file
    try? FileManager.default.removeItem(atPath: tmpPath)

    let count = data.count
    let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: count)
    data.copyBytes(to: buf, count: count)
    out_len.pointee = Int32(count)
    return buf
}

@_cdecl("music_free_artwork_data")
public func music_free_artwork_data(_ ptr: UnsafeMutablePointer<UInt8>?) {
    ptr?.deallocate()
}
