//! Decoding of sound files to PCM: WAV (PCM and float) and Ogg Vorbis.

use std::io::Cursor;

use crate::error::{Error, Result};

/// A decoded sound: interleaved 16-bit samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels per frame, 1 or 2.
    pub channels: u16,
    /// `frames * channels` samples, interleaved.
    pub samples: Vec<i16>,
}

impl Clip {
    /// The number of frames.
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// The length in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        self.frames() as u64 * 1000 / u64::from(self.sample_rate)
    }

    /// The frame of a time in milliseconds.
    pub fn frame_at(&self, ms: u64) -> usize {
        (ms * u64::from(self.sample_rate) / 1000) as usize
    }

    /// The left and right samples of a frame, as floats in `-1..1`; a
    /// mono clip gives the same on both sides.
    pub fn frame(&self, index: usize) -> (f32, f32) {
        let channels = usize::from(self.channels.max(1));
        let base = index * channels;
        let left = self.samples.get(base).copied().unwrap_or(0);
        let right = if channels > 1 {
            self.samples.get(base + 1).copied().unwrap_or(0)
        } else {
            left
        };
        (f32::from(left) / 32768.0, f32::from(right) / 32768.0)
    }

    /// A silent clip of a duration, for tests and stand-ins.
    pub fn silence(sample_rate: u32, channels: u16, ms: u64) -> Clip {
        let frames = (ms * u64::from(sample_rate) / 1000) as usize;
        Clip {
            sample_rate,
            channels,
            samples: vec![0; frames * usize::from(channels)],
        }
    }
}

/// Decodes a WAV or Ogg Vorbis file.
pub fn decode(bytes: &[u8]) -> Result<Clip> {
    if bytes.starts_with(b"RIFF") {
        decode_wav(bytes)
    } else if bytes.starts_with(b"OggS") {
        decode_vorbis(bytes)
    } else {
        Err(Error::Unsupported(
            "not a WAV or Ogg Vorbis file".to_string(),
        ))
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Error::Truncated)
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Error::Truncated)
}

/// Decodes a RIFF WAVE file: PCM of 8, 16, 24 or 32 bits, or 32-bit
/// float, in any channel count (more than two are mixed down to two).
pub fn decode_wav(bytes: &[u8]) -> Result<Clip> {
    if bytes.len() < 12 || &bytes[8..12] != b"WAVE" {
        return Err(Error::Unsupported("not a WAVE file".to_string()));
    }
    let mut at = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32_at(bytes, at + 4)? as usize;
        let body_start = at + 8;
        let body_end = (body_start + size).min(bytes.len());
        let body = &bytes[body_start..body_end];
        match id {
            b"fmt " => {
                let mut tag = u16_at(body, 0)?;
                let channels = u16_at(body, 2)?;
                let rate = u32_at(body, 4)?;
                let bits = u16_at(body, 14)?;
                if tag == 0xFFFE && body.len() >= 26 {
                    tag = u16_at(body, 24)?;
                }
                format = Some((tag, channels, rate, bits));
            }
            b"data" => {
                data = Some(body);
            }
            _ => {}
        }
        at = body_start + size + (size & 1);
    }
    let (tag, channels, rate, bits) = format.ok_or(Error::Truncated)?;
    let data = data.ok_or(Error::Truncated)?;
    if channels == 0 || rate == 0 {
        return Err(Error::Unsupported("empty WAVE format".to_string()));
    }
    let bytes_per_sample = usize::from(bits / 8);
    if bytes_per_sample == 0 {
        return Err(Error::Unsupported(format!("{bits}-bit WAVE")));
    }
    let count = data.len() / bytes_per_sample;
    let mut raw: Vec<i16> = Vec::with_capacity(count);
    for i in 0..count {
        let s = &data[i * bytes_per_sample..(i + 1) * bytes_per_sample];
        let sample = match (tag, bits) {
            (1, 8) => (i16::from(s[0]) - 128) << 8,
            (1, 16) => i16::from_le_bytes([s[0], s[1]]),
            (1, 24) => i16::from_le_bytes([s[1], s[2]]),
            (1, 32) => i16::from_le_bytes([s[2], s[3]]),
            (3, 32) => {
                let f = f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                (f.clamp(-1.0, 1.0) * 32767.0) as i16
            }
            _ => {
                return Err(Error::Unsupported(format!(
                    "WAVE format {tag} with {bits} bits"
                )));
            }
        };
        raw.push(sample);
    }
    Ok(mix_down(raw, channels, rate))
}

/// Keeps one or two channels, mixing more down to two.
fn mix_down(raw: Vec<i16>, channels: u16, rate: u32) -> Clip {
    if channels <= 2 {
        return Clip {
            sample_rate: rate,
            channels,
            samples: raw,
        };
    }
    let n = usize::from(channels);
    let samples = raw
        .chunks_exact(n)
        .flat_map(|frame| [frame[0], frame[1]])
        .collect();
    Clip {
        sample_rate: rate,
        channels: 2,
        samples,
    }
}

/// Decodes an Ogg Vorbis file.
pub fn decode_vorbis(bytes: &[u8]) -> Result<Clip> {
    let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))
        .map_err(|e| Error::Decode(e.to_string()))?;
    let channels = u16::from(reader.ident_hdr.audio_channels);
    let rate = reader.ident_hdr.audio_sample_rate;
    let mut raw: Vec<i16> = Vec::new();
    while let Some(packet) = reader
        .read_dec_packet_itl()
        .map_err(|e| Error::Decode(e.to_string()))?
    {
        raw.extend_from_slice(&packet);
    }
    if channels == 0 || rate == 0 {
        return Err(Error::Unsupported("empty Vorbis stream".to_string()));
    }
    Ok(mix_down(raw, channels, rate))
}

/// Encodes a clip as a 16-bit PCM WAV file (for tests and tools).
pub fn encode_wav(clip: &Clip) -> Vec<u8> {
    let data_len = (clip.samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&clip.channels.to_le_bytes());
    out.extend_from_slice(&clip.sample_rate.to_le_bytes());
    let block = u32::from(clip.channels) * 2;
    out.extend_from_slice(&(clip.sample_rate * block).to_le_bytes());
    out.extend_from_slice(&(block as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in &clip.samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip() {
        let clip = Clip {
            sample_rate: 8000,
            channels: 2,
            samples: vec![0, 1000, -1000, 32767, -32768, 5],
        };
        let bytes = encode_wav(&clip);
        assert_eq!(decode(&bytes).unwrap(), clip);
        assert_eq!(clip.frames(), 3);
        assert_eq!(Clip::silence(1000, 1, 250).frames(), 250);
        assert_eq!(Clip::silence(1000, 2, 250).duration_ms(), 250);
        assert_eq!(clip.frame(1), (-1000.0 / 32768.0, 32767.0 / 32768.0));
        assert_eq!(clip.frame_at(1), 8);
    }

    #[test]
    fn wav_formats() {
        // 8-bit mono.
        let mut bytes = encode_wav(&Clip {
            sample_rate: 1000,
            channels: 1,
            samples: vec![],
        });
        bytes[34] = 8;
        bytes[32] = 1;
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[128, 255]);
        let clip = decode(&bytes).unwrap();
        assert_eq!(clip.samples, vec![0, 127 << 8]);
        assert!(decode(b"RIFFxxxxWAVE").is_err());
        assert!(decode(b"nope").is_err());
        assert!(decode(b"OggSjunk").is_err());
    }
}
