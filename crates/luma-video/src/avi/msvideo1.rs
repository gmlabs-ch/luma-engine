//! Microsoft Video 1 (`CRAM`, `MSVC`, `WHAM`): blocks of 4x4 pixels
//! given one, two or eight colours each, or left as the frame before
//! had them. Written from the public description of the format (the
//! MultimediaWiki page "Microsoft Video 1"); see `docs/formats/avi.md`.

/// The picture kept from frame to frame: palette indices for the 8-bit
/// variant, 15-bit colours for the 16-bit one, rows from the top.
pub(crate) struct State {
    width: usize,
    height: usize,
    /// Whether the pixels are palette indices.
    indexed: bool,
    pixels: Vec<u16>,
}

/// The eight bits of a five-bit channel.
fn five(value: u16) -> u8 {
    let value = (value & 0x1f) as u8;
    value << 3 | value >> 2
}

impl State {
    /// The state of a stream; `None` for a depth the codec does not
    /// have (it has 8 and 16 bits per pixel).
    pub(crate) fn new(width: usize, height: usize, bits: u16) -> Option<State> {
        let indexed = match bits {
            8 => true,
            16 => false,
            _ => return None,
        };
        Some(State {
            width,
            height,
            indexed,
            pixels: vec![0; width * height],
        })
    }

    /// Decodes a frame over the kept picture and writes it to `rgba`.
    ///
    /// The blocks come row by row from the **bottom** of the picture,
    /// left to right, and so do the four rows of a block. Each starts
    /// with two bytes `a`, `b`:
    ///
    /// - `b` from 0x84 to 0x87: `(b - 0x84) * 256 + a` blocks, this one
    ///   included, stay as they were;
    /// - 8 bits, `b < 0x80`: two colours follow, and the 16 bits of
    ///   `b:a` pick one per pixel, the low bit first (a set bit takes
    ///   the first colour);
    /// - 8 bits, `b >= 0x90`: eight colours follow, two per quarter of
    ///   the block (bottom left, bottom right, top left, top right),
    ///   picked by the same 16 bits;
    /// - 8 bits, otherwise: the block is filled with colour `a`;
    /// - 16 bits, `b < 0x80`: two colours follow; when the first has
    ///   its top bit set, six more follow and the block has eight
    ///   colours as above, else two;
    /// - 16 bits, otherwise: the block is filled with the colour `b:a`.
    ///
    /// The data may end at any point: the blocks not reached stay.
    pub(crate) fn decode(&mut self, palette: &[[u8; 4]; 256], data: &[u8], rgba: &mut [u8]) {
        self.decode_blocks(data);
        for (pixel, value) in rgba.chunks_exact_mut(4).zip(&self.pixels) {
            if self.indexed {
                pixel.copy_from_slice(&palette[usize::from(*value & 0xff)]);
            } else {
                pixel.copy_from_slice(&[five(value >> 10), five(value >> 5), five(*value), 0xff]);
            }
        }
    }

    fn decode_blocks(&mut self, data: &[u8]) {
        let mut bytes = data.iter().copied();
        let (width, height) = (self.width, self.height);
        let indexed = self.indexed;
        let mut skip = 0u32;
        for block_y in 0..height.div_ceil(4) {
            for block_x in 0..width.div_ceil(4) {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let (Some(a), Some(b)) = (bytes.next(), bytes.next()) else {
                    return;
                };
                if (0x84..=0x87).contains(&b) {
                    skip = ((u32::from(b) - 0x84) << 8 | u32::from(a)).saturating_sub(1);
                    continue;
                }
                let flags = u16::from(b) << 8 | u16::from(a);
                let mut colors = [0u16; 8];
                let mut read = |count: usize, colors: &mut [u16; 8]| -> bool {
                    for color in colors.iter_mut().take(count) {
                        let value = if indexed {
                            bytes.next().map(u16::from)
                        } else {
                            match (bytes.next(), bytes.next()) {
                                (Some(lo), Some(hi)) => Some(u16::from(hi) << 8 | u16::from(lo)),
                                _ => None,
                            }
                        };
                        match value {
                            Some(value) => *color = value,
                            None => return false,
                        }
                    }
                    true
                };
                // How many colours the block has: 1, 2 or 8.
                let count = if b < 0x80 {
                    if !read(2, &mut colors) {
                        return;
                    }
                    if !indexed && colors[0] & 0x8000 != 0 {
                        let mut rest = [0u16; 8];
                        if !read(6, &mut rest) {
                            return;
                        }
                        colors[2..].copy_from_slice(&rest[..6]);
                        8
                    } else {
                        2
                    }
                } else if indexed && b >= 0x90 {
                    if !read(8, &mut colors) {
                        return;
                    }
                    8
                } else {
                    colors[0] = if indexed { u16::from(a) } else { flags };
                    1
                };
                for n in 0..16usize {
                    let (px, py) = (n % 4, n / 4);
                    let other = usize::from(flags >> n & 1 == 0);
                    let color = match count {
                        1 => colors[0],
                        2 => colors[other],
                        _ => colors[((py & 2) << 1) + (px & 2) + other],
                    };
                    let x = block_x * 4 + px;
                    // Rows count from the bottom.
                    let from_bottom = block_y * 4 + py;
                    if x < width && from_bottom < height {
                        self.pixels[(height - 1 - from_bottom) * width + x] = color;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> [[u8; 4]; 256] {
        let mut palette = [[0, 0, 0, 0xff]; 256];
        for (n, entry) in palette.iter_mut().enumerate() {
            *entry = [n as u8, n as u8, n as u8, 0xff];
        }
        palette
    }

    /// The red channel of each pixel, row by row from the top.
    fn reds(rgba: &[u8]) -> Vec<u8> {
        rgba.chunks_exact(4).map(|p| p[0]).collect()
    }

    #[test]
    fn eight_bit_blocks() {
        // 8x4: two blocks.
        let mut state = State::new(8, 4, 8).unwrap();
        let mut rgba = vec![0; 8 * 4 * 4];
        state.decode(
            &palette(),
            &[
                // Two colours, 7 and 9: bit 0 set (bottom left takes 7),
                // the top row all set.
                0x01, 0x70, 7, 9, //
                // One colour, 5.
                5, 0x80,
            ],
            &mut rgba,
        );
        assert_eq!(
            reds(&rgba),
            [
                7, 7, 7, 9, 5, 5, 5, 5, //
                9, 9, 9, 9, 5, 5, 5, 5, //
                9, 9, 9, 9, 5, 5, 5, 5, //
                7, 9, 9, 9, 5, 5, 5, 5,
            ],
            "0x7001: bit 0 and bits 12 to 14"
        );
        // The next frame skips the first block and gives the second
        // eight colours: bits 12 and 15 (of 0x9000) take the first of
        // their pair, the others the second.
        state.decode(
            &palette(),
            &[1, 0x84, 0x00, 0x90, 1, 2, 3, 4, 5, 6, 7, 8],
            &mut rgba,
        );
        assert_eq!(
            reds(&rgba),
            [
                7, 7, 7, 9, 5, 6, 8, 7, //
                9, 9, 9, 9, 6, 6, 8, 8, //
                9, 9, 9, 9, 2, 2, 4, 4, //
                7, 9, 9, 9, 2, 2, 4, 4,
            ]
        );
        // A skip of two leaves both blocks; cut data leaves the rest.
        let before = rgba.clone();
        state.decode(&palette(), &[2, 0x84], &mut rgba);
        assert_eq!(rgba, before);
        state.decode(&palette(), &[0x00, 0x10, 7], &mut rgba);
        assert_eq!(rgba, before);
        state.decode(&palette(), &[], &mut rgba);
        assert_eq!(rgba, before);
    }

    #[test]
    fn sixteen_bit_blocks() {
        let mut state = State::new(4, 4, 16).unwrap();
        let mut rgba = vec![0; 4 * 4 * 4];
        // One colour: red (0x7c00) with the top bit set.
        state.decode(&palette(), &[0x00, 0xfc], &mut rgba);
        assert!(rgba.chunks_exact(4).all(|p| p == [255, 0, 0, 0xff]));
        // Two colours: green where the bits are set, blue elsewhere.
        state.decode(&palette(), &[0x0f, 0x00, 0xe0, 0x03, 0x1f, 0x00], &mut rgba);
        assert_eq!(&rgba[12 * 4..12 * 4 + 4], &[0, 255, 0, 0xff], "bottom row");
        assert_eq!(&rgba[..4], &[0, 0, 255, 0xff], "top row");
        // Eight colours: the first has its top bit set.
        let mut data = vec![0xff, 0x7f];
        for color in [0x8001u16, 2, 3, 4, 5, 6, 7, 8] {
            data.extend_from_slice(&color.to_le_bytes());
        }
        state.decode(&palette(), &data, &mut rgba);
        // Blue channel: five bits spread over eight.
        let blues: Vec<u8> = rgba.chunks_exact(4).map(|p| p[2]).collect();
        let e = |v: u16| five(v);
        assert_eq!(
            blues,
            [
                // The top row's last bit (15) is clear: the second colour.
                e(5),
                e(5),
                e(7),
                e(8), //
                e(5),
                e(5),
                e(7),
                e(7), //
                e(1),
                e(1),
                e(3),
                e(3), //
                e(1),
                e(1),
                e(3),
                e(3),
            ]
        );
        assert!(State::new(4, 4, 24).is_none());
        // A picture whose sides are not multiples of four is clipped.
        let mut state = State::new(5, 3, 8).unwrap();
        let mut rgba = vec![0; 5 * 3 * 4];
        state.decode(&palette(), &[4, 0x80, 6, 0x80], &mut rgba);
        assert_eq!(reds(&rgba), [4, 4, 4, 4, 6, 4, 4, 4, 4, 6, 4, 4, 4, 4, 6]);
    }
}
