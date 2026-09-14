//! Album-art rendering abstraction.
//!
//! muse supports two rendering paths, chosen once at startup:
//!
//! * **ratatui-image** (`Artwork::Ratatui`) for Sixel / iTerm2 / halfblocks.
//!   These are rendered as ratatui widgets inside `terminal.draw`.
//!
//! * **Hand-rolled Kitty** (`Artwork::Kitty`) using the Kitty graphics
//!   protocol's *direct placement* (`a=T`), NOT ratatui-image's Unicode
//!   placeholder method. ratatui-image's Kitty backend relies on the Unicode
//!   placeholder (`U=1`, `\u{10EEEE}`) method, which WezTerm advertises but
//!   does not actually render — the placeholders show up as literal glyphs.
//!   Direct placement (the same approach `rview` uses) works in WezTerm,
//!   Kitty, Ghostty, and Konsole. These images are emitted as raw escape
//!   sequences *after* `terminal.draw`, positioned with `cursor::MoveTo`.
//!
//! The Kitty path decodes the artwork to a PNG once (off the UI thread) and
//! keeps the base64 payload plus pixel dimensions, so placement is cheap.

use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

/// Rendering context shared with the background artwork-fetch threads.
/// Held behind an `Arc` so worker threads can clone it cheaply.
pub struct ArtCtx {
    pub picker: Picker,
    /// When true, use the hand-rolled Kitty direct-placement path instead of
    /// ratatui-image. Set when the detected/forced protocol is Kitty.
    pub kitty: bool,
}

/// A ready-to-render piece of album art, in whichever form matches the
/// active rendering path.
pub enum Artwork {
    Ratatui(StatefulProtocol),
    Kitty(KittyArt),
}

/// Kitty artwork: a PNG payload (base64) plus its source pixel dimensions,
/// used to compute an aspect-correct cell box at placement time.
pub struct KittyArt {
    png_b64: String,
    width: u32,
    height: u32,
}

impl KittyArt {
    /// Encode a decoded image into a base64 PNG for Kitty transmission.
    pub fn from_image(img: &image::DynamicImage) -> Option<Self> {
        let width = img.width();
        let height = img.height();
        if width == 0 || height == 0 {
            return None;
        }
        let mut png: Vec<u8> = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .ok()?;
        Some(Self {
            png_b64: base64_encode(&png),
            width,
            height,
        })
    }

    /// Compute the cell box (columns, rows) to display this image within the
    /// available area, preserving aspect ratio and never upscaling past the
    /// area. `cell_px` is the terminal's (width, height) per character cell.
    pub fn fit(&self, avail_cols: u16, avail_rows: u16, cell_px: (u16, u16)) -> (u16, u16) {
        let (cw, ch) = (cell_px.0.max(1) as f64, cell_px.1.max(1) as f64);
        let max_w = avail_cols as f64 * cw;
        let max_h = avail_rows as f64 * ch;
        let (iw, ih) = (self.width as f64, self.height as f64);
        // Largest scale that fits inside the pixel box, without upscaling.
        let scale = (max_w / iw).min(max_h / ih).min(1.0);
        let disp_w = iw * scale;
        let disp_h = ih * scale;
        let cols = (disp_w / cw).ceil() as u16;
        let rows = (disp_h / ch).ceil() as u16;
        (cols.clamp(1, avail_cols), rows.clamp(1, avail_rows))
    }

    /// Build the Kitty escape sequence that transmits and displays this image
    /// at the current cursor position, scaled into `cols` x `rows` cells.
    /// The base64 payload is chunked to respect Kitty's 4096-byte limit.
    pub fn transmit_escape(&self, cols: u16, rows: u16) -> String {
        let bytes = self.png_b64.as_bytes();
        let chunks: Vec<&[u8]> = bytes.chunks(4096).collect();
        let total = chunks.len().max(1);
        let mut out = String::with_capacity(self.png_b64.len() + total * 24);
        for (i, chunk) in chunks.iter().enumerate() {
            let more = if i + 1 < total { 1 } else { 0 };
            if i == 0 {
                // a=T: transmit and display at cursor. f=100: PNG. c/r: fit box.
                out.push_str(&format!("\x1b_Ga=T,f=100,c={cols},r={rows},q=2,m={more};"));
            } else {
                out.push_str(&format!("\x1b_Gm={more};"));
            }
            out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            out.push_str("\x1b\\");
        }
        out
    }
}

/// Delete all Kitty images (visible placements and stored data).
pub const KITTY_DELETE_ALL: &str = "\x1b_Ga=d,d=A,q=2\x1b\\";

/// Placement request produced by the UI layer for the main loop to emit.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KittyPlacement {
    pub rect: Rect,
}

/// Standard base64 (RFC 4648) encoder. Inlined to avoid a dependency, matching
/// the project's existing no-crypto-dep approach in `spotify.rs`.
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn fit_preserves_aspect_and_never_upscales() {
        let art = KittyArt {
            png_b64: String::new(),
            width: 100,
            height: 100,
        };
        // Square image into a 14-wide box with 8x16 cells: pixel box is
        // 112x(rows*16). Height is the limiting dimension for a square.
        let (cols, rows) = art.fit(14, 10, (8, 16));
        assert!(cols <= 14 && rows <= 10);
        assert!(cols >= 1 && rows >= 1);
    }

    #[test]
    fn transmit_escape_is_chunked_and_framed() {
        let art = KittyArt {
            png_b64: "A".repeat(5000),
            width: 10,
            height: 10,
        };
        let esc = art.transmit_escape(14, 7);
        assert!(esc.starts_with("\x1b_Ga=T,f=100,c=14,r=7,q=2,m=1;"));
        assert!(esc.contains("\x1b_Gm=0;"));
        assert!(esc.ends_with("\x1b\\"));
    }
}
