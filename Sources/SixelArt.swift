import Foundation
import Darwin
import CoreGraphics
import ImageIO

struct SixelArt {

    // MARK: - Sixel Detection

    static func detectSixelSupport() -> Bool {
        // MUSE_SIXEL env var overrides detection
        if let env = ProcessInfo.processInfo.environment["MUSE_SIXEL"] {
            return env == "1"
        }
        // DA1 query: ask terminal what it supports
        var oldTermios = termios()
        tcgetattr(STDIN_FILENO, &oldTermios)

        var raw = oldTermios
        raw.c_lflag &= ~UInt(ECHO | ICANON)
        raw.c_cc.16 = 0  // VMIN
        raw.c_cc.17 = 5  // VTIME = 500ms
        tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw)

        defer { tcsetattr(STDIN_FILENO, TCSAFLUSH, &oldTermios) }

        let query = "\u{1B}[c"
        var queryBytes = Array(query.utf8)
        Darwin.write(STDOUT_FILENO, &queryBytes, queryBytes.count)

        var response = ""
        var buf: UInt8 = 0
        let deadline = Date().addingTimeInterval(0.5)
        while Date() < deadline {
            let n = read(STDIN_FILENO, &buf, 1)
            if n == 1 {
                response.append(Character(UnicodeScalar(buf)))
                if buf == 0x63 { break }
            } else {
                break
            }
        }

        guard let qMark = response.firstIndex(of: "?"),
              let cEnd = response.lastIndex(of: "c") else { return false }
        let paramStr = response[response.index(after: qMark)..<cEnd]
        let params = paramStr.split(separator: ";").compactMap { Int($0) }
        return params.contains(4)
    }

    // MARK: - Cell Pixel Size

    struct CellSize {
        var width: Int
        var height: Int
    }

    static func getCellPixelSize() -> CellSize {
        var ws = winsize()
        if ioctl(STDOUT_FILENO, UInt(TIOCGWINSZ), &ws) == 0,
           ws.ws_xpixel > 0, ws.ws_ypixel > 0, ws.ws_col > 0, ws.ws_row > 0 {
            return CellSize(
                width: Int(ws.ws_xpixel) / Int(ws.ws_col),
                height: Int(ws.ws_ypixel) / Int(ws.ws_row)
            )
        }
        return CellSize(width: 8, height: 16)
    }

    // MARK: - Artwork Fetch

    static func fetchArtworkData() -> Data? {
        let script = """
        tell application "Music"
            if player state is not stopped then
                try
                    set artData to raw data of artwork 1 of current track
                    return artData
                end try
            end if
        end tell
        """
        let appleScript = NSAppleScript(source: script)
        var error: NSDictionary?
        guard let result = appleScript?.executeAndReturnError(&error) else { return nil }
        let data = result.data
        return data.isEmpty ? nil : data
    }

    // MARK: - Image Resize

    static func loadAndResize(data: Data, targetWidth: Int, targetHeight: Int) -> CGImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { return nil }

        let colorSpace = CGColorSpaceCreateDeviceRGB()
        guard let ctx = CGContext(
            data: nil,
            width: targetWidth,
            height: targetHeight,
            bitsPerComponent: 8,
            bytesPerRow: targetWidth * 4,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else { return nil }

        ctx.interpolationQuality = .high
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: targetWidth, height: targetHeight))
        return ctx.makeImage()
    }

    // MARK: - Color Quantization (Median Cut)

    struct RGB: Hashable {
        var r: UInt8, g: UInt8, b: UInt8
    }

    static func extractPixels(from image: CGImage) -> [RGB] {
        let w = image.width, h = image.height
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        var pixelData = [UInt8](repeating: 0, count: w * h * 4)
        guard let ctx = CGContext(
            data: &pixelData, width: w, height: h,
            bitsPerComponent: 8, bytesPerRow: w * 4,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else { return [] }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))

        var pixels = [RGB]()
        pixels.reserveCapacity(w * h)
        for i in 0..<(w * h) {
            let offset = i * 4
            pixels.append(RGB(r: pixelData[offset], g: pixelData[offset+1], b: pixelData[offset+2]))
        }
        return pixels
    }

    static func medianCut(pixels: [RGB], maxColors: Int) -> [RGB] {
        guard !pixels.isEmpty else { return [] }

        struct Box {
            var pixels: [RGB]
            var rMin: UInt8, rMax: UInt8
            var gMin: UInt8, gMax: UInt8
            var bMin: UInt8, bMax: UInt8

            init(pixels: [RGB]) {
                self.pixels = pixels
                var rLo: UInt8 = 255, rHi: UInt8 = 0
                var gLo: UInt8 = 255, gHi: UInt8 = 0
                var bLo: UInt8 = 255, bHi: UInt8 = 0
                for p in pixels {
                    if p.r < rLo { rLo = p.r }; if p.r > rHi { rHi = p.r }
                    if p.g < gLo { gLo = p.g }; if p.g > gHi { gHi = p.g }
                    if p.b < bLo { bLo = p.b }; if p.b > bHi { bHi = p.b }
                }
                rMin = rLo; rMax = rHi; gMin = gLo; gMax = gHi; bMin = bLo; bMax = bHi
            }

            var range: Int { max(Int(rMax) - Int(rMin), Int(gMax) - Int(gMin), Int(bMax) - Int(bMin)) }

            var dominantChannel: Int {
                let rr = Int(rMax) - Int(rMin)
                let gr = Int(gMax) - Int(gMin)
                let br = Int(bMax) - Int(bMin)
                if rr >= gr && rr >= br { return 0 }
                if gr >= br { return 1 }
                return 2
            }

            var average: RGB {
                guard !pixels.isEmpty else { return RGB(r: 0, g: 0, b: 0) }
                var rSum = 0, gSum = 0, bSum = 0
                for p in pixels { rSum += Int(p.r); gSum += Int(p.g); bSum += Int(p.b) }
                let c = pixels.count
                return RGB(r: UInt8(rSum / c), g: UInt8(gSum / c), b: UInt8(bSum / c))
            }
        }

        var boxes = [Box(pixels: pixels)]

        while boxes.count < maxColors {
            // Find box with largest range
            var bestIdx = 0
            var bestRange = boxes[0].range
            for i in 1..<boxes.count {
                let r = boxes[i].range
                if r > bestRange { bestRange = r; bestIdx = i }
            }
            if bestRange == 0 || boxes[bestIdx].pixels.count < 2 { break }

            var pix = boxes[bestIdx].pixels
            let ch = boxes[bestIdx].dominantChannel
            switch ch {
            case 0: pix.sort { $0.r < $1.r }
            case 1: pix.sort { $0.g < $1.g }
            default: pix.sort { $0.b < $1.b }
            }
            let mid = pix.count / 2
            boxes[bestIdx] = Box(pixels: Array(pix[..<mid]))
            boxes.append(Box(pixels: Array(pix[mid...])))
        }

        return boxes.map(\.average)
    }

    static func nearestColorIndex(pixel: RGB, palette: [RGB]) -> Int {
        var bestIdx = 0
        var bestDist = Int.max
        for (i, c) in palette.enumerated() {
            let dr = Int(pixel.r) - Int(c.r)
            let dg = Int(pixel.g) - Int(c.g)
            let db = Int(pixel.b) - Int(c.b)
            let dist = dr*dr + dg*dg + db*db
            if dist < bestDist { bestDist = dist; bestIdx = i }
        }
        return bestIdx
    }

    // MARK: - Sixel Encoding

    static func encodeSixel(image: CGImage, palette: [RGB]) -> String {
        let w = image.width, h = image.height
        let pixels = extractPixels(from: image)
        guard !pixels.isEmpty else { return "" }

        // Build indexed image
        var indexed = [Int](repeating: 0, count: w * h)
        for i in 0..<pixels.count {
            indexed[i] = nearestColorIndex(pixel: pixels[i], palette: palette)
        }

        var out = ""
        out.reserveCapacity(w * h)

        // DCS q - start sixel
        out += "\u{1B}Pq"
        // Raster attributes: Pan=1;Pad=1;Ph=width;Pv=height
        out += "\"1;1;\(w);\(h)"

        // Palette definitions
        for (i, c) in palette.enumerated() {
            let r100 = Int(c.r) * 100 / 255
            let g100 = Int(c.g) * 100 / 255
            let b100 = Int(c.b) * 100 / 255
            out += "#\(i);2;\(r100);\(g100);\(b100)"
        }

        // Sixel data - process 6 rows at a time
        let sixelRows = (h + 5) / 6
        for sy in 0..<sixelRows {
            let baseY = sy * 6
            // For each color in palette
            var firstColor = true
            for ci in 0..<palette.count {
                // Build row data for this color
                var rowData = [UInt8](repeating: 0, count: w)
                var hasData = false
                for x in 0..<w {
                    var bits: UInt8 = 0
                    for bit in 0..<6 {
                        let y = baseY + bit
                        if y < h && indexed[y * w + x] == ci {
                            bits |= (1 << bit)
                        }
                    }
                    rowData[x] = bits
                    if bits != 0 { hasData = true }
                }
                guard hasData else { continue }

                if !firstColor {
                    out += "$" // carriage return (stay on same sixel row)
                }
                firstColor = false

                // Select color
                out += "#\(ci)"

                // RLE encode
                var x = 0
                while x < w {
                    let val = rowData[x]
                    var count = 1
                    while x + count < w && rowData[x + count] == val {
                        count += 1
                    }
                    let ch = Character(UnicodeScalar(val + 63))
                    if count >= 4 {
                        out += "!\(count)\(ch)"
                    } else {
                        for _ in 0..<count {
                            out += String(ch)
                        }
                    }
                    x += count
                }
            }
            if sy < sixelRows - 1 {
                out += "-" // newline (next sixel row)
            }
        }

        // String terminator
        out += "\u{1B}\\"
        return out
    }

    // MARK: - Cache

    struct ArtworkCache: Sendable {
        var key: String = ""
        var sixelString: String = ""
        var cellCols: Int = 0
        var cellRows: Int = 0
    }

    // MARK: - Convenience

    static func generateSixel(artRows: Int) -> ArtworkCache? {
        guard let data = fetchArtworkData() else { return nil }

        let cellSize = getCellPixelSize()
        let pixelHeight = artRows * cellSize.height
        let roundedHeight = ((pixelHeight + 5) / 6) * 6
        let targetWidth = roundedHeight // square aspect

        guard let resized = loadAndResize(data: data, targetWidth: targetWidth, targetHeight: roundedHeight) else {
            return nil
        }

        let pixels = extractPixels(from: resized)
        let palette = medianCut(pixels: pixels, maxColors: 128)
        guard !palette.isEmpty else { return nil }

        let sixel = encodeSixel(image: resized, palette: palette)
        guard !sixel.isEmpty else { return nil }

        let cellCols = (targetWidth + cellSize.width - 1) / cellSize.width
        let cellRows = (roundedHeight + cellSize.height - 1) / cellSize.height

        return ArtworkCache(
            sixelString: sixel,
            cellCols: cellCols,
            cellRows: cellRows
        )
    }
}
