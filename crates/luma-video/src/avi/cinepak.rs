//! Cinepak (`cvid`): vector quantisation. A frame is cut into strips;
//! each strip has two codebooks of 256 entries (four luminance values
//! and two chrominance values each) and gives every 4x4 block either
//! one entry of the first codebook, stretched over the block, or four
//! entries of the second, one per quarter. Written from the public
//! description of the stream format (Tim Ferguson, "Cinepak (CVID)
//! stream format for AVI and QT", and the MultimediaWiki page
//! "Cinepak"); see `docs/formats/avi.md`.

use crate::{Error, Result};

/// The most strips a frame may have.
const MAX_STRIPS: usize = 32;

/// A codebook entry: its four pixels (top left, top right, bottom left,
/// bottom right) as RGB.
type Entry = [[u8; 3]; 4];

/// The two codebooks of a strip.
#[derive(Clone)]
struct Codebooks {
    /// Entries stretched over a whole block (`V1`).
    v1: [Entry; 256],
    /// Entries for a quarter of a block (`V4`).
    v4: [Entry; 256],
}

/// What is kept between frames: the picture and each strip's codebooks.
pub(crate) struct State {
    width: usize,
    height: usize,
    rgb: Vec<[u8; 3]>,
    strips: Vec<Codebooks>,
}

fn be16(data: &[u8], at: usize) -> Option<usize> {
    let b = data.get(at..at + 2)?;
    Some(usize::from(u16::from_be_bytes([b[0], b[1]])))
}

/// The bits of a chunk's flag words: 32 at a time, the high bit first.
struct Flags {
    word: u32,
    left: u32,
}

impl Flags {
    fn new() -> Flags {
        Flags { word: 0, left: 0 }
    }

    /// The next flag; a new word is read from `data` when the last one
    /// is used up. `None` when the data ends.
    fn next(&mut self, data: &mut &[u8]) -> Option<bool> {
        if self.left == 0 {
            let (word, rest) = data.split_first_chunk::<4>()?;
            self.word = u32::from_be_bytes(*word);
            self.left = 32;
            *data = rest;
        }
        self.left -= 1;
        Some(self.word >> self.left & 1 != 0)
    }
}

/// Reads one codebook entry: four luminance bytes and, unless the
/// stream is grey, two signed chrominance bytes.
fn read_entry(data: &mut &[u8], grey: bool) -> Option<Entry> {
    let (luma, rest) = data.split_first_chunk::<4>()?;
    let (u, v, rest) = if grey {
        (0, 0, rest)
    } else {
        let (chroma, rest) = rest.split_first_chunk::<2>()?;
        (i32::from(chroma[0] as i8), i32::from(chroma[1] as i8), rest)
    };
    *data = rest;
    Some(luma.map(|y| {
        let y = i32::from(y);
        [
            (y + 2 * v).clamp(0, 255) as u8,
            (y - (u >> 1) - v).clamp(0, 255) as u8,
            (y + 2 * u).clamp(0, 255) as u8,
        ]
    }))
}

/// Loads a codebook chunk: every entry in turn, or, for an update,
/// only the entries whose flag is set.
fn load_codebook(book: &mut [Entry; 256], mut data: &[u8], update: bool, grey: bool) {
    let mut flags = Flags::new();
    for slot in book.iter_mut() {
        if update {
            match flags.next(&mut data) {
                Some(true) => {}
                Some(false) => continue,
                None => return,
            }
        }
        match read_entry(&mut data, grey) {
            Some(entry) => *slot = entry,
            None => return,
        }
    }
}

impl State {
    pub(crate) fn new(width: usize, height: usize) -> State {
        State {
            width,
            height,
            rgb: vec![[0; 3]; width * height],
            strips: Vec::new(),
        }
    }

    fn put(&mut self, x: usize, y: usize, color: [u8; 3]) {
        if x < self.width && y < self.height {
            self.rgb[y * self.width + x] = color;
        }
    }

    /// Draws the vectors of a strip (`y1..y2`): for each block, row by
    /// row from the top, a flag says whether it has one `V1` entry or
    /// four `V4` entries; in a frame built on the last, a flag before
    /// it says whether the block changes at all.
    fn draw(
        &mut self,
        strip: usize,
        (y1, y2): (usize, usize),
        mut data: &[u8],
        inter: bool,
        only_v1: bool,
    ) {
        let mut flags = Flags::new();
        let mut y = y1;
        while y < y2.min(self.height) {
            let mut x = 0;
            while x < self.width {
                if inter {
                    match flags.next(&mut data) {
                        Some(true) => {}
                        Some(false) => {
                            x += 4;
                            continue;
                        }
                        None => return,
                    }
                }
                let v4 = if only_v1 {
                    false
                } else {
                    match flags.next(&mut data) {
                        Some(flag) => flag,
                        None => return,
                    }
                };
                if v4 {
                    let Some((indices, rest)) = data.split_first_chunk::<4>() else {
                        return;
                    };
                    data = rest;
                    for (quarter, index) in indices.iter().enumerate() {
                        let entry = self.strips[strip].v4[usize::from(*index)];
                        let (qx, qy) = (x + quarter % 2 * 2, y + quarter / 2 * 2);
                        for (n, color) in entry.iter().enumerate() {
                            self.put(qx + n % 2, qy + n / 2, *color);
                        }
                    }
                } else {
                    let Some((index, rest)) = data.split_first() else {
                        return;
                    };
                    data = rest;
                    let entry = self.strips[strip].v1[usize::from(*index)];
                    for n in 0..16 {
                        let (px, py) = (n % 4, n / 4);
                        self.put(x + px, y + py, entry[py / 2 * 2 + px / 2]);
                    }
                }
                x += 4;
            }
            y += 4;
        }
    }

    /// Decodes a frame over the kept picture and writes it to `rgba`.
    ///
    /// The frame starts with a byte of flags, its length on three
    /// bytes, the width, the height and the number of strips (big
    /// endian). Each strip has an id, its size, and its top, left,
    /// bottom and right edges; a strip whose top is 0 follows the one
    /// before it and its bottom is its height. Unless bit 0 of the
    /// frame's flags is set, a strip starts with the codebooks of the
    /// strip before it. Then come chunks (an id and a size, both on two
    /// bytes): `0x20` to `0x27` load a codebook (bit 0 of the low byte:
    /// only the flagged entries; bit 1: the `V1` codebook; bit 2: grey
    /// entries of four bytes), `0x30` draws a key strip, `0x31` a strip
    /// built on the last frame, `0x32` a strip of `V1` blocks only.
    pub(crate) fn decode(&mut self, data: &[u8], rgba: &mut [u8]) -> Result<()> {
        let short = || Error::Decode("a Cinepak frame ends in its header".into());
        let frame_flags = *data.first().ok_or_else(short)?;
        let strips = be16(data, 8).ok_or_else(short)?;
        if strips > MAX_STRIPS {
            return Err(Error::Decode(format!("a Cinepak frame of {strips} strips")));
        }
        let mut at = 10;
        let mut y0 = 0;
        for strip in 0..strips {
            let (Some(size), Some(top), Some(bottom)) =
                (be16(data, at + 2), be16(data, at + 4), be16(data, at + 8))
            else {
                break;
            };
            if strip >= self.strips.len() {
                self.strips.push(Codebooks {
                    v1: [[[0; 3]; 4]; 256],
                    v4: [[[0; 3]; 4]; 256],
                });
            }
            if strip > 0 && frame_flags & 1 == 0 {
                self.strips[strip] = self.strips[strip - 1].clone();
            }
            let (y1, y2) = if top == 0 {
                (y0, y0 + bottom)
            } else {
                (top, bottom)
            };
            let end = (at + size.max(12)).min(data.len());
            let mut chunk = at + 12;
            while let (Some(id), Some(len)) = (be16(data, chunk), be16(data, chunk + 2)) {
                if chunk + 4 > end || len < 4 {
                    break;
                }
                let body = &data[chunk + 4..(chunk + len).min(end)];
                match id >> 8 {
                    0x20..=0x27 => {
                        let kind = id >> 8;
                        let book = if kind & 2 != 0 {
                            &mut self.strips[strip].v1
                        } else {
                            &mut self.strips[strip].v4
                        };
                        load_codebook(book, body, kind & 1 != 0, kind & 4 != 0);
                    }
                    0x30 => self.draw(strip, (y1, y2), body, false, false),
                    0x31 => self.draw(strip, (y1, y2), body, true, false),
                    0x32 => self.draw(strip, (y1, y2), body, false, true),
                    _ => {}
                }
                chunk += len;
            }
            at += size.max(12);
            y0 = y2;
        }
        for (pixel, color) in rgba.chunks_exact_mut(4).zip(&self.rgb) {
            pixel[..3].copy_from_slice(color);
            pixel[3] = 0xff;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
        let mut out = id.to_be_bytes().to_vec();
        out.extend_from_slice(&((body.len() + 4) as u16).to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    fn strip(id: u16, top: u16, bottom: u16, width: u16, chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut out = id.to_be_bytes().to_vec();
        out.extend_from_slice(&((body.len() + 12) as u16).to_be_bytes());
        for value in [top, 0, bottom, width] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&body);
        out
    }

    fn frame(flags: u8, width: u16, height: u16, strips: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = strips.concat();
        let mut out = vec![flags];
        out.extend_from_slice(&((body.len() + 10) as u32).to_be_bytes()[1..]);
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&(strips.len() as u16).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// The red channel of each pixel.
    fn reds(rgba: &[u8]) -> Vec<u8> {
        rgba.chunks_exact(4).map(|p| p[0]).collect()
    }

    #[test]
    fn colours_come_from_luminance_and_two_signed_offsets() {
        let mut data: &[u8] = &[100, 0, 255, 50, 10, 0xf6];
        let entry = read_entry(&mut data, false).unwrap();
        assert!(data.is_empty());
        // u = 10, v = -10: red y - 20, green y - 5 + 10, blue y + 20.
        assert_eq!(entry[0], [80, 105, 120]);
        assert_eq!(entry[1], [0, 5, 20]);
        assert_eq!(entry[2], [235, 255, 255]);
        let mut data: &[u8] = &[1, 2, 3, 4, 9];
        assert_eq!(read_entry(&mut data, true).unwrap()[3], [4, 4, 4]);
        assert_eq!(data, [9]);
        assert!(read_entry(&mut data, false).is_none());
    }

    #[test]
    fn blocks_take_one_entry_or_four() {
        // 8x4: a grey V1 codebook of two entries and a grey V4
        // codebook of four; the first block is V1 (entry 1), the second
        // V4 (entries 0, 1, 2, 3).
        let v1 = chunk(0x2600, &[1, 1, 1, 1, 10, 20, 30, 40]);
        let v4 = chunk(
            0x2400,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );
        let vectors = chunk(0x3000, &[0x40, 0, 0, 0, 1, 0, 1, 2, 3]);
        let key = frame(0, 8, 4, &[strip(0x1000, 0, 4, 8, &[v4, v1, vectors])]);
        let mut state = State::new(8, 4);
        let mut rgba = vec![0; 8 * 4 * 4];
        state.decode(&key, &mut rgba).unwrap();
        assert_eq!(
            reds(&rgba),
            [
                10, 10, 20, 20, 1, 2, 5, 6, //
                10, 10, 20, 20, 3, 4, 7, 8, //
                30, 30, 40, 40, 9, 10, 13, 14, //
                30, 30, 40, 40, 11, 12, 15, 16,
            ]
        );
        // The next frame updates entry 1 of the V1 codebook, skips the
        // first block and redraws the second with it.
        let update = chunk(0x2700, &[0x40, 0, 0, 0, 50, 60, 70, 80]);
        let vectors = chunk(0x3100, &[0b0100_0000, 0, 0, 0, 1]);
        let inter = frame(0, 8, 4, &[strip(0x1100, 0, 4, 8, &[update, vectors])]);
        state.decode(&inter, &mut rgba).unwrap();
        assert_eq!(
            reds(&rgba),
            [
                10, 10, 20, 20, 50, 50, 60, 60, //
                10, 10, 20, 20, 50, 50, 60, 60, //
                30, 30, 40, 40, 70, 70, 80, 80, //
                30, 30, 40, 40, 70, 70, 80, 80,
            ]
        );
    }

    #[test]
    fn strips_follow_each_other_and_share_codebooks() {
        // 4x8 in two strips of V1 blocks; the second strip loads no
        // codebook: it starts with the first strip's.
        let v1 = chunk(0x2600, &[7, 7, 7, 7, 9, 9, 9, 9]);
        let first = strip(0x1000, 0, 4, 4, &[v1, chunk(0x3200, &[0])]);
        let second = strip(0x1000, 0, 4, 4, &[chunk(0x3200, &[1])]);
        let mut state = State::new(4, 8);
        let mut rgba = vec![0; 4 * 8 * 4];
        state
            .decode(&frame(0, 4, 8, &[first.clone(), second.clone()]), &mut rgba)
            .unwrap();
        assert_eq!(reds(&rgba)[..16], [7; 16]);
        assert_eq!(reds(&rgba)[16..], [9; 16]);
        // With bit 0 of the frame's flags, each strip keeps its own
        // codebooks: the second's are still empty in a new stream.
        let mut state = State::new(4, 8);
        state
            .decode(&frame(1, 4, 8, &[first.clone(), second]), &mut rgba)
            .unwrap();
        assert_eq!(reds(&rgba)[16..], [0; 16]);
        // Absolute edges place a strip too.
        let low = strip(0x1000, 4, 8, 4, &[chunk(0x3200, &[1])]);
        state
            .decode(&frame(0, 4, 8, &[first, low]), &mut rgba)
            .unwrap();
        assert_eq!(reds(&rgba)[16..], [9; 16]);
    }

    #[test]
    fn damaged_frames_do_not_panic() {
        let mut state = State::new(8, 8);
        let mut rgba = vec![0; 8 * 8 * 4];
        assert!(state.decode(&[], &mut rgba).is_err());
        assert!(state.decode(&[0; 9], &mut rgba).is_err());
        let mut many = vec![0; 10];
        many[8] = 0xff;
        assert!(state.decode(&many, &mut rgba).is_err());
        let v1 = chunk(0x2600, &[7, 7, 7, 7]);
        let good = frame(
            0,
            8,
            8,
            &[strip(0x1000, 0, 8, 8, &[v1, chunk(0x3000, &[0; 8])])],
        );
        for cut in 0..good.len() {
            let _ = state.decode(&good[..cut], &mut rgba);
        }
        // Sizes that lie: a strip and a chunk longer than the data, a
        // chunk shorter than its own header, edges outside the picture.
        let mut lying = good.clone();
        lying[12] = 0xff;
        lying[13] = 0xff;
        state.decode(&lying, &mut rgba).unwrap();
        let mut lying = good.clone();
        lying[24] = 0xff;
        state.decode(&lying, &mut rgba).unwrap();
        let mut lying = good.clone();
        lying[25] = 0;
        state.decode(&lying, &mut rgba).unwrap();
        let far = strip(0x1000, 4000, 9000, 8, &[chunk(0x3200, &[0; 64])]);
        state.decode(&frame(0, 8, 8, &[far]), &mut rgba).unwrap();
    }
}
