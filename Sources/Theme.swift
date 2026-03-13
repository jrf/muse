struct Theme: Sendable {
    var border: Int
    var accent: Int
    var text: Int
    var textBright: Int
    var textDim: Int
    var textMuted: Int
    var timeText: Int
    var error: Int

    static let defaultTheme = Theme(
        border: 75, accent: 213, text: 252, textBright: 255,
        textDim: 245, textMuted: 240, timeText: 249, error: 196
    )

    static let allThemes: [(name: String, theme: Theme)] = [
        ("synthwave", defaultTheme),
        ("monochrome", Theme(
            border: 245, accent: 255, text: 250, textBright: 255,
            textDim: 242, textMuted: 238, timeText: 247, error: 196
        )),
        ("ocean", Theme(
            border: 32, accent: 39, text: 153, textBright: 195,
            textDim: 67, textMuted: 60, timeText: 117, error: 196
        )),
        ("sunset", Theme(
            border: 208, accent: 203, text: 223, textBright: 230,
            textDim: 180, textMuted: 137, timeText: 216, error: 196
        )),
        ("forest", Theme(
            border: 65, accent: 114, text: 151, textBright: 194,
            textDim: 108, textMuted: 59, timeText: 150, error: 196
        )),
        ("tokyo night", Theme(
            border: 61, accent: 141, text: 189, textBright: 195,
            textDim: 103, textMuted: 60, timeText: 117, error: 210
        )),
    ]
}
