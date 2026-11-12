//! The sound of an AVI file as a WAV file of PCM samples: PCM is
//! passed on as it is; Microsoft ADPCM and IMA ADPCM (both described in
//! Microsoft's "Multimedia Data Standards Update" and the IMA's
//! recommended practices) are decoded to 16 bits.

use super::WaveFormat;
use crate::{Error, Result};

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_ADPCM: u16 = 0x0002;
const WAVE_FORMAT_IMA_ADPCM: u16 = 0x0011;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;

/// The names of the format tags a game's AVI may carry, for the log.
fn tag_name(tag: u16) -> &'static str {
    match tag {
        0x0003 => " (IEEE float)",
        0x0006 => " (A-law)",
        0x0007 => " (mu-law)",
        0x0031 => " (GSM 6.10)",
        0x0050 => " (MPEG audio)",
        0x0055 => " (MP3)",
        0x0092 | 0x2000 => " (AC-3)",
        0x00ff | 0x706d => " (AAC)",
        0x0160..=0x0163 => " (Windows Media Audio)",
        0x674f..=0x6771 => " (Ogg Vorbis)",
        _ => "",
    }
}

/// A WAV file of PCM samples.
fn wav(channels: u16, sample_rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
    let block = channels * (bits / 8);
    let len = (data.len() - data.len() % usize::from(block.max(1))).min(u32::MAX as usize - 36);
    let mut out = Vec::with_capacity(44 + len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate.saturating_mul(u32::from(block))).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(len as u32).to_le_bytes());
    out.extend_from_slice(&data[..len]);
    out
}

/// The audio stream's data as a WAV file, or an error naming the
/// format when Luma does not decode it.
pub(crate) fn to_wav(format: &WaveFormat, data: &[u8]) -> Result<Vec<u8>> {
    let unsupported = |what: String| Err(Error::UnsupportedCodec(what));
    if !(1..=2).contains(&format.channels) {
        return unsupported(format!("sound with {} channels", format.channels));
    }
    if format.sample_rate == 0 || format.sample_rate > 384_000 {
        return unsupported(format!(
            "sound at {} samples per second",
            format.sample_rate
        ));
    }
    let tag = match format.tag {
        // The real format is the first two bytes of the sub-format.
        WAVE_FORMAT_EXTENSIBLE => match format.extra.get(6..8) {
            Some(b) => u16::from_le_bytes([b[0], b[1]]),
            None => WAVE_FORMAT_EXTENSIBLE,
        },
        tag => tag,
    };
    let pcm = match tag {
        WAVE_FORMAT_PCM => {
            if !matches!(format.bits, 8 | 16 | 24 | 32) {
                return unsupported(format!("PCM sound of {} bits", format.bits));
            }
            return Ok(wav(format.channels, format.sample_rate, format.bits, data));
        }
        WAVE_FORMAT_ADPCM => ms_adpcm(format, data),
        WAVE_FORMAT_IMA_ADPCM => ima_adpcm(format, data),
        tag => {
            return unsupported(format!("sound format tag 0x{tag:04X}{}", tag_name(tag)));
        }
    };
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    Ok(wav(format.channels, format.sample_rate, 16, &bytes))
}

/// How the step of Microsoft ADPCM follows each code.
const MS_ADAPTATION: [i32; 16] = [
    230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
];

/// The seven standard predictor pairs of Microsoft ADPCM.
const MS_COEFFICIENTS: [(i32, i32); 7] = [
    (256, 0),
    (512, -256),
    (0, 0),
    (192, 64),
    (240, 0),
    (460, -208),
    (392, -232),
];

/// Decodes Microsoft ADPCM to interleaved 16-bit samples. Each block
/// starts, per channel, with the number of a predictor, the step, and
/// the two samples before the codes (the later one first); then come
/// the codes, four bits each, the high half of a byte first, the
/// channels in turn.
fn ms_adpcm(format: &WaveFormat, data: &[u8]) -> Vec<i16> {
    let channels = usize::from(format.channels);
    let block = usize::from(format.block_align);
    let header = 7 * channels;
    let mut out = Vec::new();
    if block <= header {
        return out;
    }
    // The file's own predictors, after the samples per block and their
    // count; else the standard ones.
    let mut coefficients: Vec<(i32, i32)> = format
        .extra
        .get(4..)
        .unwrap_or_default()
        .chunks_exact(4)
        .take(usize::from(super::u16_at(&format.extra, 2).unwrap_or(0)))
        .map(|c| {
            (
                i32::from(i16::from_le_bytes([c[0], c[1]])),
                i32::from(i16::from_le_bytes([c[2], c[3]])),
            )
        })
        .collect();
    if coefficients.is_empty() {
        coefficients = MS_COEFFICIENTS.to_vec();
    }
    for chunk in data.chunks(block) {
        if chunk.len() <= header {
            break;
        }
        let i16_at = |at: usize| i32::from(i16::from_le_bytes([chunk[at], chunk[at + 1]]));
        let mut coefficient = [(0, 0); 2];
        let mut delta = [0i32; 2];
        let mut first = [0i32; 2];
        let mut second = [0i32; 2];
        for c in 0..channels {
            coefficient[c] = coefficients
                .get(usize::from(chunk[c]))
                .copied()
                .unwrap_or((0, 0));
            delta[c] = i16_at(channels + c * 2);
            first[c] = i16_at(channels * 3 + c * 2);
            second[c] = i16_at(channels * 5 + c * 2);
        }
        out.extend(second[..channels].iter().map(|s| *s as i16));
        out.extend(first[..channels].iter().map(|s| *s as i16));
        let codes = chunk[header..]
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 0xf]);
        for (n, code) in codes.enumerate() {
            let c = n % channels;
            let signed = if code & 8 != 0 {
                i32::from(code) - 16
            } else {
                i32::from(code)
            };
            let predicted = (first[c] * coefficient[c].0 + second[c] * coefficient[c].1) / 256;
            let sample = (predicted + signed * delta[c]).clamp(-32768, 32767);
            second[c] = first[c];
            first[c] = sample;
            delta[c] = (MS_ADAPTATION[usize::from(code)] * delta[c] / 256).max(16);
            out.push(sample as i16);
        }
    }
    out
}

/// How the step index of IMA ADPCM follows each code.
const IMA_INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// The steps of IMA ADPCM.
const IMA_STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// Decodes IMA ADPCM (the WAV variant) to interleaved 16-bit samples.
/// Each block starts, per channel, with a sample and a step index
/// (four bytes); then come groups of four bytes per channel, each
/// holding eight codes, the low half of a byte first.
fn ima_adpcm(format: &WaveFormat, data: &[u8]) -> Vec<i16> {
    let channels = usize::from(format.channels);
    let block = usize::from(format.block_align);
    let header = 4 * channels;
    let mut out = Vec::new();
    if block <= header {
        return out;
    }
    for chunk in data.chunks(block) {
        if chunk.len() < header {
            break;
        }
        let mut sample = [0i32; 2];
        let mut index = [0i32; 2];
        for c in 0..channels {
            sample[c] = i32::from(i16::from_le_bytes([chunk[c * 4], chunk[c * 4 + 1]]));
            index[c] = i32::from(chunk[c * 4 + 2]).min(88);
            out.push(sample[c] as i16);
        }
        // Whole groups only: eight samples of each channel.
        for group in chunk[header..].chunks_exact(4 * channels) {
            let at = out.len();
            out.resize(at + 8 * channels, 0);
            for c in 0..channels {
                for n in 0..8 {
                    let byte = group[c * 4 + n / 2];
                    let code = if n % 2 == 0 { byte & 0xf } else { byte >> 4 };
                    let step = IMA_STEPS[index[c] as usize];
                    let mut diff = step >> 3;
                    if code & 4 != 0 {
                        diff += step;
                    }
                    if code & 2 != 0 {
                        diff += step >> 1;
                    }
                    if code & 1 != 0 {
                        diff += step >> 2;
                    }
                    if code & 8 != 0 {
                        diff = -diff;
                    }
                    sample[c] = (sample[c] + diff).clamp(-32768, 32767);
                    index[c] = (index[c] + IMA_INDEX[usize::from(code & 7)]).clamp(0, 88);
                    out[at + n * channels + c] = sample[c] as i16;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(tag: u16, channels: u16, block_align: u16, bits: u16) -> WaveFormat {
        WaveFormat {
            tag,
            channels,
            sample_rate: 8000,
            block_align,
            bits,
            extra: Vec::new(),
        }
    }

    #[test]
    fn pcm_is_wrapped_as_it_is() {
        let wav = to_wav(&format(1, 2, 4, 16), &[1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[20..24], &[1, 0, 2, 0], "PCM, two channels");
        assert_eq!(&wav[32..36], &[4, 0, 16, 0]);
        // Whole blocks only.
        assert_eq!(&wav[40..], &[8, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
        // The extensible format with a PCM sub-format is PCM.
        let mut extensible = format(0xfffe, 1, 1, 8);
        extensible.extra = vec![8, 0, 4, 0, 0, 0, 1, 0, 0, 0, 0, 0, 16, 0];
        assert_eq!(to_wav(&extensible, &[128, 129]).unwrap().len(), 46);
    }

    #[test]
    fn unsupported_formats_are_named() {
        let name = |f: &WaveFormat| match to_wav(f, &[0; 16]) {
            Err(Error::UnsupportedCodec(name)) => name,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            name(&format(0x55, 2, 1, 0)),
            "sound format tag 0x0055 (MP3)"
        );
        assert_eq!(name(&format(0x1234, 2, 1, 0)), "sound format tag 0x1234");
        assert_eq!(name(&format(1, 6, 12, 16)), "sound with 6 channels");
        assert_eq!(name(&format(1, 1, 1, 12)), "PCM sound of 12 bits");
        let mut silent = format(1, 1, 1, 8);
        silent.sample_rate = 0;
        assert_eq!(name(&silent), "sound at 0 samples per second");
    }

    #[test]
    fn ms_adpcm_follows_its_predictor() {
        // One mono block: predictor 1 (2 * first - second), step 16,
        // first sample 100, second 90, then codes 1, 15 (-1), 0, 0.
        let block = [1, 16, 0, 100, 0, 90, 0, 0x1f, 0x00];
        let samples = ms_adpcm(&format(2, 1, 9, 4), &block);
        // 90, 100, then 110 + 16 = 126, then 152 - 16 = 136, 146, 156.
        assert_eq!(samples, [90, 100, 126, 136, 146, 156]);
        // Stereo: the codes alternate between the channels.
        let block = [0, 0, 16, 0, 16, 0, 10, 0, 20, 0, 0, 0, 0, 0, 0x12];
        let samples = ms_adpcm(&format(2, 2, 15, 4), &block);
        assert_eq!(samples, [0, 0, 10, 20, 26, 52]);
        // A block too small for its header, a cut block, a predictor
        // the stream does not have: nothing, and no panic.
        assert!(ms_adpcm(&format(2, 1, 7, 4), &block).is_empty());
        assert!(ms_adpcm(&format(2, 1, 0, 4), &block).is_empty());
        assert!(ms_adpcm(&format(2, 1, 9, 4), &[1, 16, 0, 100]).is_empty());
        let odd = ms_adpcm(&format(2, 1, 9, 4), &[200, 16, 0, 5, 0, 5, 0, 0x10]);
        assert_eq!(odd, [5, 5, 16, 0]);
    }

    #[test]
    fn ima_adpcm_steps_up_and_down() {
        // Mono: sample 0, index 0 (step 7), then eight codes.
        let block = [0, 0, 0, 0, 0x07, 0x8f, 0x00, 0x00];
        let samples = ima_adpcm(&format(0x11, 1, 8, 4), &block);
        // Code 7: 0 + 7/8 + 7 + 3 + 1 = 11, index 8 (step 16);
        // code 0: + 2, index 7 (step 14); code 15: -(1 + 14 + 7 + 3).
        assert_eq!(samples[..4], [0, 11, 13, -12]);
        assert_eq!(samples.len(), 9);
        // Stereo: four bytes for the left channel, then four for the
        // right.
        let block = [
            10, 0, 0, 0, 20, 0, 0, 0, //
            0x07, 0, 0, 0, 0x0f, 0, 0, 0,
        ];
        let samples = ima_adpcm(&format(0x11, 2, 16, 4), &block);
        assert_eq!(samples[..4], [10, 20, 21, 9]);
        assert_eq!(samples.len(), 18);
        assert!(ima_adpcm(&format(0x11, 2, 8, 4), &block).is_empty());
        // An index past the table is held to it.
        let wild = ima_adpcm(
            &format(0x11, 1, 8, 4),
            &[0, 0, 200, 0, 0x77, 0x77, 0x77, 0x77],
        );
        assert_eq!(wild.len(), 9);
    }
}
