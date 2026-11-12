//! Microsoft RLE (`BI_RLE8`, `BI_RLE4`): run-length coded palette
//! indices, as Microsoft's bitmap documentation describes the two
//! encodings. In an AVI stream a frame only writes the pixels that
//! changed: the others keep the value the frame before gave them.

/// Decodes a frame over `indices` (`width * height` palette indices,
/// rows from the bottom, as the stream stores them). Pairs of bytes:
///
/// - `n, v` with `n > 0`: `n` pixels of `v` (for 4 bits, the two halves
///   of `v` in turn, the high one first);
/// - `0, 0`: the end of the row; `0, 1`: the end of the picture;
/// - `0, 2, dx, dy`: move right and up, leaving the pixels as they were;
/// - `0, n` with `n >= 3`: `n` pixels given as they are, padded to an
///   even number of bytes.
///
/// Pixels outside the picture are dropped, and the data may end at any
/// point: what was read stays.
pub(crate) fn decode(bits: u8, width: usize, height: usize, data: &[u8], indices: &mut [u8]) {
    let mut bytes = data.iter().copied();
    let (mut x, mut y) = (0usize, 0usize);
    let mut put = |x: usize, y: usize, value: u8| {
        if x < width && y < height {
            indices[y * width + x] = value;
        }
    };
    while y < height {
        let (Some(count), Some(value)) = (bytes.next(), bytes.next()) else {
            return;
        };
        let count = usize::from(count);
        if count > 0 {
            for n in 0..count {
                let pixel = match bits {
                    4 if n % 2 == 0 => value >> 4,
                    4 => value & 0xf,
                    _ => value,
                };
                put(x + n, y, pixel);
            }
            x += count;
            continue;
        }
        match value {
            0 => {
                x = 0;
                y += 1;
            }
            1 => return,
            2 => {
                let (Some(dx), Some(dy)) = (bytes.next(), bytes.next()) else {
                    return;
                };
                x += usize::from(dx);
                y += usize::from(dy);
            }
            literal => {
                let literal = usize::from(literal);
                let stored = if bits == 4 {
                    literal.div_ceil(2)
                } else {
                    literal
                };
                let mut byte = 0;
                for n in 0..literal {
                    let pixel = if bits == 4 {
                        if n % 2 == 0 {
                            let Some(next) = bytes.next() else {
                                return;
                            };
                            byte = next;
                            byte >> 4
                        } else {
                            byte & 0xf
                        }
                    } else {
                        let Some(next) = bytes.next() else {
                            return;
                        };
                        next
                    };
                    put(x + n, y, pixel);
                }
                x += literal;
                if stored % 2 == 1 {
                    bytes.next();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_literals_and_moves() {
        // 4x3, rows from the bottom.
        let mut indices = vec![9; 12];
        decode(
            8,
            4,
            3,
            &[
                3, 5, 0, 0, // row 0: three fives, the fourth pixel kept
                0, 3, 1, 2, 3, 0, 0, 0, // row 1: a literal run, padded
                0, 2, 2, 0, 1, 7, // row 2: skip two, one seven
                0, 1, 4, 4,
            ],
            &mut indices,
        );
        assert_eq!(indices, [5, 5, 5, 9, 1, 2, 3, 9, 9, 9, 7, 9]);
        // A second frame changes one pixel: a move, then the end.
        decode(8, 4, 3, &[0, 2, 1, 1, 1, 8, 0, 1], &mut indices);
        assert_eq!(indices, [5, 5, 5, 9, 1, 8, 3, 9, 9, 9, 7, 9]);
    }

    #[test]
    fn four_bits_alternate_the_halves() {
        let mut indices = vec![0; 8];
        decode(
            4,
            8,
            1,
            &[3, 0x12, 0, 5, 0x34, 0x56, 0x70, 0, 0, 1],
            &mut indices,
        );
        assert_eq!(indices, [1, 2, 1, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn damaged_data_stays_in_the_picture() {
        let mut indices = vec![0; 4];
        // A run past the row, a move past the picture, a cut literal.
        decode(8, 2, 2, &[255, 1, 0, 2, 200, 200, 9, 9], &mut indices);
        assert_eq!(indices, [1, 1, 0, 0]);
        decode(8, 2, 2, &[0, 200, 3], &mut indices);
        assert_eq!(indices, [3, 1, 0, 0]);
        decode(8, 2, 2, &[0], &mut indices);
        decode(4, 2, 2, &[0, 9, 0x12], &mut indices);
        assert_eq!(indices, [1, 2, 0, 0]);
    }
}
