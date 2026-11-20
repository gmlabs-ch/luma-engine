//! MPEG audio (layers 1, 2 and 3, format tags 0x0050 and 0x0055) in
//! an AVI file, decoded with the `symphonia` crates (MPL-2.0, used
//! unmodified). The chunks of the audio stream, joined, are an MPEG
//! audio elementary stream: a run of frames, each starting with a
//! sync word and a header that gives its layer, bit rate, sample
//! rate and channel mode. The decoder follows the headers, so the
//! `WAVEFORMAT` fields of the file are not trusted for the output.

use std::io::Cursor;

use symphonia::core::audio::sample::Sample;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::{Error, Result};

/// The most samples (all channels together) a track may decode to,
/// 128 million: 256 MiB of 16-bit PCM. A hostile stream of tiny
/// frames cannot grow past it.
const MAX_SAMPLES: usize = 128 * 1024 * 1024;

/// The decoded track: interleaved 16-bit samples, the channel count
/// and the sample rate.
pub(crate) struct Decoded {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

fn refuse(what: &str) -> Error {
    Error::UnsupportedCodec(format!("MPEG audio: {what}"))
}

/// Decodes an MPEG audio elementary stream to 16-bit PCM. Frames that
/// do not decode are skipped (the reader resynchronises on the next
/// header, as a player does); a stream with no decodable frame is an
/// error, and one whose format changes in the middle stops there.
pub(crate) fn decode(data: &[u8]) -> Result<Decoded> {
    let source = MediaSourceStream::new(Box::new(Cursor::new(data.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| refuse("no frame found"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| refuse("no audio track"))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| refuse("no audio parameters"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|_| refuse("no decoder for the stream"))?;
    let track_id = track.id;

    let mut shape: Option<(u16, u32)> = None;
    let mut samples: Vec<i16> = Vec::new();
    let mut frame: Vec<i16> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = buffer.spec();
        let this = match (u16::try_from(spec.channels().count()), spec.rate()) {
            (Ok(c @ 1..=2), rate @ 1..=384_000) => (c, rate),
            _ => return Err(refuse("a layout other than mono or stereo")),
        };
        match shape {
            None => shape = Some(this),
            // A stream that changes format cannot be one WAV file:
            // keep what came before.
            Some(first) if first != this => break,
            Some(_) => {}
        }
        frame.resize(buffer.samples_interleaved(), i16::MID);
        buffer.copy_to_slice_interleaved(&mut frame);
        if samples.len() + frame.len() > MAX_SAMPLES {
            break;
        }
        samples.extend_from_slice(&frame);
    }
    match shape {
        Some((channels, sample_rate)) if !samples.is_empty() => Ok(Decoded {
            channels,
            sample_rate,
            samples,
        }),
        _ => Err(refuse("no frame could be decoded")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One MPEG-1 layer 3 frame of silence: 128 kbit/s, 44.1 kHz,
    /// 417 bytes, a header and zeroed side information and main data,
    /// which decode to 1152 samples per channel of silence.
    fn silent_frame(stereo: bool) -> Vec<u8> {
        let mut frame = vec![0u8; 417];
        // Sync, MPEG-1, layer 3, no CRC; 128 kbit/s, 44.1 kHz, no padding.
        frame[..3].copy_from_slice(&[0xff, 0xfb, 0x90]);
        frame[3] = if stereo { 0x00 } else { 0xc0 };
        frame
    }

    #[test]
    fn silent_layer_3_frames_decode_to_silence() {
        let stream = silent_frame(false).repeat(6);
        let decoded = decode(&stream).unwrap();
        assert_eq!((decoded.channels, decoded.sample_rate), (1, 44_100));
        assert!(
            decoded.samples.len() >= 1152 * 3,
            "{}",
            decoded.samples.len()
        );
        assert_eq!(decoded.samples.len() % 1152, 0);
        assert!(decoded.samples.iter().all(|&s| s == 0));

        let decoded = decode(&silent_frame(true).repeat(6)).unwrap();
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.samples.len() % 2304, 0);
    }

    #[test]
    fn hostile_streams_do_not_panic() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[0; 4096]).is_err());
        assert!(decode(&[0xff; 4096]).is_err());
        // Frames cut short, and garbage after good frames.
        let mut stream = silent_frame(false).repeat(4);
        stream.truncate(stream.len() - 100);
        let _ = decode(&stream);
        let mut stream = silent_frame(false).repeat(4);
        stream.extend((0..2000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8));
        let _ = decode(&stream);
        // Every second header byte with a short body.
        for b1 in 0..=255u8 {
            let _ = decode(&[0xff, b1, 0x90, 0x00, 1, 2, 3, 4, 5, 6, 7, 8]);
        }
    }
}
