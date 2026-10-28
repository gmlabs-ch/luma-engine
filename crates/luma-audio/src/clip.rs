//! Decoded sounds: WAV and Ogg Vorbis files read by Symphonia into
//! Kira's static sound data.

use std::io::Cursor;
use std::sync::Arc;

use kira::Frame;
use kira::sound::static_sound::StaticSoundData;

use crate::error::{Error, Result};

/// A decoded sound, shared by the plays of it.
#[derive(Debug, Clone)]
pub struct Clip {
    pub(crate) data: StaticSoundData,
}

impl Clip {
    /// Frames per second.
    pub fn sample_rate(&self) -> u32 {
        self.data.sample_rate
    }

    /// The number of frames.
    pub fn frames(&self) -> usize {
        self.data.num_frames()
    }

    /// The length in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.data.duration().as_millis() as u64
    }

    /// A silent clip of a duration, for tests and stand-ins.
    pub fn silence(sample_rate: u32, ms: u64) -> Clip {
        let frames = (ms * u64::from(sample_rate) / 1000) as usize;
        Clip {
            data: StaticSoundData {
                sample_rate,
                frames: Arc::from(vec![Frame::ZERO; frames]),
                settings: Default::default(),
                slice: None,
            },
        }
    }
}

/// Decodes a WAV or Ogg Vorbis file. Other containers and codecs
/// Symphonia knows are rejected unless the crate is built with them.
pub fn decode(bytes: &[u8]) -> Result<Clip> {
    if bytes.is_empty() {
        return Err(Error::Truncated);
    }
    let data = StaticSoundData::from_cursor(Cursor::new(bytes.to_vec()))
        .map_err(|e| Error::Decode(e.to_string()))?;
    Ok(Clip { data })
}

/// A 16-bit PCM WAV file of silence, for tests and tools.
pub fn silent_wav(sample_rate: u32, channels: u16, ms: u64) -> Vec<u8> {
    let frames = (ms * u64::from(sample_rate) / 1000) as u32;
    let data_len = frames * u32::from(channels) * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    let block = u32::from(channels) * 2;
    out.extend_from_slice(&(sample_rate * block).to_le_bytes());
    out.extend_from_slice(&(block as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(out.len() + data_len as usize, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_wav() {
        let clip = decode(&silent_wav(8000, 2, 300)).unwrap();
        assert_eq!(clip.sample_rate(), 8000);
        assert_eq!(clip.frames(), 2400);
        assert_eq!(clip.duration_ms(), 300);
        assert_eq!(Clip::silence(1000, 250).frames(), 250);
        assert!(decode(b"").is_err());
        assert!(decode(b"nope").is_err());
        assert!(decode(b"OggSjunk").is_err());
    }
}
