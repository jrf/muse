import Darwin

enum Key {
    case character(Character)
    case up, down, left, right
    case enter, escape, backspace, delete, tab
    case space
}

struct TerminalSize {
    var width: Int
    var height: Int
}

final class Terminal {
    private var originalTermios = termios()
    private var isRaw = false

    func enableRawMode() {
        tcgetattr(STDIN_FILENO, &originalTermios)
        var raw = originalTermios
        raw.c_lflag &= ~UInt(ECHO | ICANON | ISIG | IEXTEN)
        raw.c_iflag &= ~UInt(IXON | ICRNL | BRKINT | INPCK | ISTRIP)
        raw.c_oflag &= ~UInt(OPOST)
        raw.c_cflag |= UInt(CS8)
        raw.c_cc.16 = 0  // VMIN
        raw.c_cc.17 = 1  // VTIME = 100ms
        tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw)
        isRaw = true
        // Hide cursor
        let hide = "\u{1B}[?25l"
        _ = Darwin.write(STDOUT_FILENO, hide, hide.utf8.count)
    }

    func restoreMode() {
        guard isRaw else { return }
        tcsetattr(STDIN_FILENO, TCSAFLUSH, &originalTermios)
        isRaw = false
        // Show cursor, reset style
        let seq = "\u{1B}[?25h\u{1B}[0m"
        _ = Darwin.write(STDOUT_FILENO, seq, seq.utf8.count)
    }

    func getSize() -> TerminalSize {
        var ws = winsize()
        if ioctl(STDOUT_FILENO, UInt(TIOCGWINSZ), &ws) == 0 {
            return TerminalSize(width: Int(ws.ws_col), height: Int(ws.ws_row))
        }
        return TerminalSize(width: 80, height: 24)
    }

    func readKey() -> Key? {
        var c: UInt8 = 0
        let n = read(STDIN_FILENO, &c, 1)
        guard n == 1 else { return nil }

        switch c {
        case 27: // Escape sequence
            var seq: [UInt8] = [0, 0]
            let n1 = read(STDIN_FILENO, &seq[0], 1)
            guard n1 == 1 else { return .escape }
            let n2 = read(STDIN_FILENO, &seq[1], 1)
            guard n2 == 1 else { return .escape }
            if seq[0] == 91 { // '['
                switch seq[1] {
                case 65: return .up
                case 66: return .down
                case 67: return .right
                case 68: return .left
                default: return .escape
                }
            }
            return .escape
        case 13: return .enter
        case 32: return .space
        case 127: return .backspace
        case 9: return .tab
        case 3: return .character("q") // Ctrl-C → quit
        default:
            if c >= 32 && c < 127 {
                return .character(Character(UnicodeScalar(c)))
            }
            return nil
        }
    }

    func write(_ content: String) {
        var data = Array(content.utf8)
        Darwin.write(STDOUT_FILENO, &data, data.count)
    }

    func clearScreen() {
        self.write("\u{1B}[2J\u{1B}[H")
    }
}
