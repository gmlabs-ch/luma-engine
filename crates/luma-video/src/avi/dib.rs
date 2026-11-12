//! Uncompressed AVI frames: device-independent bitmaps (`BI_RGB`,
//! `BI_BITFIELDS`) of 1, 4, 8, 16, 24 or 32 bits per pixel, as
//! Microsoft's bitmap documentation describes them.

use crate::{Error, Result};

/// One colour channel of a pixel given by a mask.
#[derive(Debug, Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    bits: u32,
}

impl Channel {
    fn new(mask: u32) -> Channel {
        let shift = if mask == 0 { 0 } else { mask.trailing_zeros() };
        Channel {
            mask,
            shift,
            bits: (mask >> shift).trailing_ones(),
        }
    }

    /// The channel of a pixel as 8 bits, the high bits repeated in the
    /// low ones (31 of 5 bits is 255).
    fn get(self, pixel: u32) -> u8 {
        let value = (pixel & self.mask) >> self.shift;
        match self.bits {
            0 => 0,
            bits if bits >= 8 => (value >> (bits - 8)) as u8,
            bits => {
                let mut out = (value << (8 - bits)) & 0xff;
                let mut filled = bits;
                while filled < 8 {
                    out |= out >> filled;
                    filled *= 2;
                }
                out as u8
            }
        }
    }
}

/// How the pixels of an uncompressed frame are laid out.
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    width: usize,
    height: usize,
    bits: u16,
    top_down: bool,
    channels: [Channel; 3],
}

impl Layout {
    /// The layout of a format; `None` for a bit count bitmaps do not
    /// have.
    pub(crate) fn new(
        width: usize,
        height: usize,
        bits: u16,
        top_down: bool,
        masks: Option<[u32; 3]>,
    ) -> Option<Layout> {
        let masks = match (bits, masks) {
            (1 | 4 | 8 | 24, _) => [0; 3],
            (16 | 32, Some(masks)) => masks,
            // Five bits per channel without masks.
            (16, None) => [0x7c00, 0x03e0, 0x001f],
            (32, None) => [0x00ff_0000, 0x0000_ff00, 0x0000_00ff],
            _ => return None,
        };
        Some(Layout {
            width,
            height,
            bits,
            top_down,
            channels: masks.map(Channel::new),
        })
    }

    /// Decodes a frame to RGBA. The rows are padded to four bytes and
    /// run from the bottom unless the format says otherwise.
    pub(crate) fn decode(
        &self,
        palette: &[[u8; 4]; 256],
        data: &[u8],
        rgba: &mut [u8],
    ) -> Result<()> {
        let bits = usize::from(self.bits);
        let row_bytes = (self.width * bits).div_ceil(8);
        let stride = (self.width * bits).div_ceil(32) * 4;
        let needed = stride * (self.height - 1) + row_bytes;
        if data.len() < needed {
            return Err(Error::Decode(format!(
                "an uncompressed frame of {} bytes where {needed} are needed",
                data.len()
            )));
        }
        for y in 0..self.height {
            let source = if self.top_down {
                y
            } else {
                self.height - 1 - y
            };
            let row = &data[source * stride..source * stride + row_bytes];
            let out = &mut rgba[y * self.width * 4..(y + 1) * self.width * 4];
            for (x, pixel) in out.chunks_exact_mut(4).enumerate() {
                let color = match self.bits {
                    1 => palette[usize::from(row[x / 8] >> (7 - x % 8) & 1)],
                    4 => palette[usize::from(row[x / 2] >> (4 - x % 2 * 4) & 0xf)],
                    8 => palette[usize::from(row[x])],
                    24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 0xff],
                    _ => {
                        let value = if self.bits == 16 {
                            u32::from(u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]))
                        } else {
                            u32::from_le_bytes([
                                row[x * 4],
                                row[x * 4 + 1],
                                row[x * 4 + 2],
                                row[x * 4 + 3],
                            ])
                        };
                        [
                            self.channels[0].get(value),
                            self.channels[1].get(value),
                            self.channels[2].get(value),
                            0xff,
                        ]
                    }
                };
                pixel.copy_from_slice(&color);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_expand_to_eight_bits() {
        let five = Channel::new(0x7c00);
        assert_eq!(five.get(0x7c00), 255);
        assert_eq!(five.get(0x0400), 8);
        assert_eq!(five.get(0), 0);
        let six = Channel::new(0x07e0);
        assert_eq!(six.get(0x07e0), 255);
        assert_eq!(Channel::new(0x00ff_0000).get(0x0012_3456), 0x12);
        assert_eq!(Channel::new(0).get(0xffff_ffff), 0);
        // Ten bits keep their high eight.
        assert_eq!(Channel::new(0x3ff).get(0x3ff), 255);
        let one = Channel::new(0x8000);
        assert_eq!((one.get(0x8000), one.get(0)), (255, 0));
    }

    #[test]
    fn rows_run_from_the_bottom_and_are_padded() {
        let mut palette = [[0, 0, 0, 0xff]; 256];
        palette[1] = [10, 20, 30, 0xff];
        palette[2] = [40, 50, 60, 0xff];
        // 3x2, 8 bits: rows of three bytes padded to four, bottom first.
        let layout = Layout::new(3, 2, 8, false, None).unwrap();
        let mut rgba = vec![0; 3 * 2 * 4];
        layout
            .decode(&palette, &[2, 2, 2, 0, 1, 0, 1], &mut rgba)
            .unwrap();
        assert_eq!(&rgba[..4], &[10, 20, 30, 0xff]);
        assert_eq!(&rgba[4..8], &[0, 0, 0, 0xff]);
        assert_eq!(&rgba[12..16], &[40, 50, 60, 0xff]);
        // One byte short.
        assert!(
            layout
                .decode(&palette, &[2, 2, 2, 0, 1, 0], &mut rgba)
                .is_err()
        );
        // 24 bits are blue, green, red; 16 bits five each; top-down.
        let layout = Layout::new(1, 2, 24, true, None).unwrap();
        let mut rgba = vec![0; 8];
        layout
            .decode(&palette, &[1, 2, 3, 0, 4, 5, 6], &mut rgba)
            .unwrap();
        assert_eq!(rgba, [3, 2, 1, 0xff, 6, 5, 4, 0xff]);
        let layout = Layout::new(2, 1, 16, false, None).unwrap();
        let mut rgba = vec![0; 8];
        layout
            .decode(&palette, &[0x00, 0x7c, 0x1f, 0x00], &mut rgba)
            .unwrap();
        assert_eq!(rgba, [255, 0, 0, 0xff, 0, 0, 255, 0xff]);
        // Four bits and one bit: the high bits come first.
        let layout = Layout::new(3, 1, 4, false, None).unwrap();
        let mut rgba = vec![0; 12];
        layout.decode(&palette, &[0x12, 0x10], &mut rgba).unwrap();
        assert_eq!((rgba[0], rgba[4], rgba[8]), (10, 40, 10));
        let layout = Layout::new(9, 1, 1, false, None).unwrap();
        let mut rgba = vec![0; 36];
        layout.decode(&palette, &[0x80, 0x80], &mut rgba).unwrap();
        assert_eq!((rgba[0], rgba[4], rgba[32]), (10, 0, 10));
        assert!(Layout::new(1, 1, 12, false, None).is_none());
    }
}
