//! Motion JPEG frames: each frame is a JPEG picture, decoded by the
//! `image` crate. Frames written for AVI (the `AVI1` variant) usually
//! leave out the Huffman tables: the decoder is then given the default
//! tables of the JPEG standard (ITU-T T.81, annex K.3), as the Motion
//! JPEG DIB format asks.

use std::io::Cursor;

use image::ImageDecoder;
use image::codecs::jpeg::JpegDecoder;

use crate::{Error, Result};

/// A table of annex K.3: its class and number, the count of codes of
/// each length, the values.
struct Table {
    id: u8,
    counts: [u8; 16],
    values: &'static [u8],
}

const DC_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

const AC_LUMINANCE: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

const AC_CHROMINANCE: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

const TABLES: [Table; 4] = [
    Table {
        id: 0x00,
        counts: [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        values: &DC_VALUES,
    },
    Table {
        id: 0x01,
        counts: [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
        values: &DC_VALUES,
    },
    Table {
        id: 0x10,
        counts: [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d],
        values: &AC_LUMINANCE,
    },
    Table {
        id: 0x11,
        counts: [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77],
        values: &AC_CHROMINANCE,
    },
];

/// The default tables as one `DHT` segment.
fn default_tables() -> Vec<u8> {
    let mut body = Vec::new();
    for table in &TABLES {
        body.push(table.id);
        body.extend_from_slice(&table.counts);
        body.extend_from_slice(table.values);
    }
    let mut segment = vec![0xff, 0xc4];
    segment.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    segment.extend_from_slice(&body);
    segment
}

/// The frame with the default Huffman tables put before its first scan
/// when it defines none; `None` when it already has tables or is not
/// laid out as a JPEG picture.
pub(crate) fn with_default_tables(data: &[u8]) -> Option<Vec<u8>> {
    if data.get(..2) != Some(&[0xff, 0xd8]) {
        return None;
    }
    let mut at = 2;
    loop {
        let marker = data.get(at..at + 2)?;
        if marker[0] != 0xff {
            return None;
        }
        match marker[1] {
            // Fill bytes before a marker.
            0xff => at += 1,
            // A table: nothing to add.
            0xc4 => return None,
            // The first scan.
            0xda => {
                let mut out = Vec::with_capacity(data.len() + 432);
                out.extend_from_slice(&data[..at]);
                out.extend_from_slice(&default_tables());
                out.extend_from_slice(&data[at..]);
                return Some(out);
            }
            // Markers without a length.
            0x01 | 0xd0..=0xd8 => at += 2,
            0xd9 => return None,
            _ => {
                let len = data.get(at + 2..at + 4)?;
                at += 2 + usize::from(u16::from_be_bytes([len[0], len[1]])).max(2);
            }
        }
    }
}

/// Decodes a frame into `rgba` (`width * height` pixels). A picture of
/// half the height is one field of an interlaced frame: each of its
/// rows is shown twice. A picture of another size is copied where it
/// overlaps.
pub(crate) fn decode(width: usize, height: usize, data: &[u8], rgba: &mut [u8]) -> Result<()> {
    let patched = with_default_tables(data);
    let data = patched.as_deref().unwrap_or(data);
    let failed = |e: image::ImageError| Error::Decode(format!("Motion JPEG frame: {e}"));
    let decoder = JpegDecoder::new(Cursor::new(data)).map_err(failed)?;
    let (w, h) = decoder.dimensions();
    let (w, h) = (w as usize, h as usize);
    // Nothing larger than the stream's own picture is decoded.
    if w == 0 || h == 0 || w > width * 2 || h > height * 2 {
        return Err(Error::Decode(format!(
            "Motion JPEG frame of {w}x{h} in a stream of {width}x{height}"
        )));
    }
    let picture = image::DynamicImage::from_decoder(decoder)
        .map_err(failed)?
        .into_rgba8()
        .into_raw();
    let field = h * 2 == height;
    for y in 0..height {
        let source = if field { y / 2 } else { y };
        if source >= h {
            break;
        }
        let count = w.min(width) * 4;
        rgba[y * width * 4..y * width * 4 + count]
            .copy_from_slice(&picture[source * w * 4..source * w * 4 + count]);
    }
    Ok(())
}

/// A JPEG picture without its `DHT` segments, as an AVI holds it.
#[cfg(test)]
pub(crate) fn without_tables(data: &[u8]) -> Vec<u8> {
    let mut out = data[..2].to_vec();
    let mut at = 2;
    while data[at + 1] != 0xda {
        let len = usize::from(u16::from_be_bytes([data[at + 2], data[at + 3]])) + 2;
        if data[at + 1] != 0xc4 {
            out.extend_from_slice(&data[at..at + len]);
        }
        at += len;
    }
    out.extend_from_slice(&data[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG picture, as the `image` crate's encoder writes it: with
    /// the standard's tables.
    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut rgb = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&[(x * 8) as u8, (y * 8) as u8, ((x + y) * 4) as u8]);
            }
        }
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    #[test]
    fn a_frame_without_tables_gets_the_default_ones() {
        let full = jpeg(32, 16);
        assert!(with_default_tables(&full).is_none(), "it has its tables");
        let bare = without_tables(&full);
        assert!(bare.len() < full.len());
        let mut expected = vec![0; 32 * 16 * 4];
        decode(32, 16, &full, &mut expected).unwrap();
        let mut rgba = vec![0; 32 * 16 * 4];
        decode(32, 16, &bare, &mut rgba).unwrap();
        assert_eq!(rgba, expected, "the default tables are the standard's");
        // Close to what was encoded.
        assert!(rgba[4 * 10].abs_diff(80) < 12, "{}", rgba[40]);
        assert_eq!(default_tables().len(), 4 + 2 * 29 + 2 * 179);
    }

    #[test]
    fn a_field_is_shown_on_two_rows_and_damage_is_an_error() {
        let field = jpeg(16, 8);
        let mut rgba = vec![0; 16 * 16 * 4];
        decode(16, 16, &field, &mut rgba).unwrap();
        assert_eq!(rgba[..64], rgba[64..128]);
        assert_ne!(rgba[..64], rgba[128..192]);
        // Too large for the stream, cut short, not a picture at all.
        let mut small = vec![0; 4 * 4 * 4];
        assert!(decode(4, 4, &jpeg(32, 32), &mut small).is_err());
        assert!(decode(16, 8, &field[..field.len() / 2], &mut rgba).is_err());
        assert!(decode(16, 8, b"not a picture", &mut rgba).is_err());
        assert!(decode(16, 8, &[], &mut rgba).is_err());
        assert!(with_default_tables(&[0xff, 0xd8, 0xff]).is_none());
        assert!(with_default_tables(&[0xff, 0xd8, 0xff, 0xe0, 0xff]).is_none());
    }
}
