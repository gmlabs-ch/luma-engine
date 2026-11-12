//! AVI files: the RIFF container read into its streams, and the first
//! video stream decoded to RGBA frames.
//!
//! The container follows Microsoft's public description of the format
//! ("AVI RIFF File Reference", and the OpenDML extension's `AVIX`
//! segments); what is read is listed in `docs/formats/avi.md`. The file
//! is untrusted: every size and offset is checked against the bytes at
//! hand, and nothing here panics on a damaged file.

mod build;
mod cinepak;
mod dib;
mod mjpeg;
mod msvideo1;
mod rle;
mod sound;

use std::collections::HashSet;

pub use build::{AviSound, AviSpec, build_avi, synthetic_avi};

use crate::{Error, Frame, FrameSource, Result, VideoInfo};

/// The largest picture side accepted, in pixels.
const MAX_SIDE: u32 = 8192;
/// The largest picture accepted, in pixels (64 MB of RGBA).
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
/// The longest video accepted, in milliseconds (a day): a damaged header
/// must not freeze a game for good.
const MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1000;
/// How deep `LIST` chunks are followed inside `movi` (`rec ` groups).
const MAX_DEPTH: u32 = 4;

/// `AVIIF_KEYFRAME` of an `idx1` entry.
const AVIIF_KEYFRAME: u32 = 0x10;

/// A little-endian `u16` at an offset, if the bytes hold it.
pub(crate) fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let b = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

/// A little-endian `u32` at an offset, if the bytes hold it.
pub(crate) fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// A FourCC as text for a log line: its printable characters, or its
/// value in hexadecimal when it is a number (`BI_RLE8` is 1).
pub fn fourcc_name(code: u32) -> String {
    let bytes = code.to_le_bytes();
    if bytes.iter().all(|b| (0x20..0x7f).contains(b)) {
        bytes.iter().map(|b| char::from(*b)).collect()
    } else {
        format!("0x{code:08X}")
    }
}

/// Whether the bytes start as an AVI file does (`RIFF` .... `AVI `).
pub fn is_avi(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"AVI "
}

/// The stream header of a stream (`strh`, `AVISTREAMHEADER`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamHeader {
    /// `vids`, `auds`, ... as a little-endian number.
    pub kind: u32,
    /// The codec the stream names (`fccHandler`).
    pub handler: u32,
    /// With `rate`: `rate / scale` samples per second.
    pub scale: u32,
    /// See `scale`.
    pub rate: u32,
    /// The time of the first sample, in samples (`dwStart`).
    pub start: u32,
    /// The number of samples the header states (`dwLength`).
    pub length: u32,
    /// Bytes per sample, 0 when the chunks vary (`dwSampleSize`).
    pub sample_size: u32,
}

/// The picture format of a video stream (`strf`, `BITMAPINFOHEADER`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitmapFormat {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Whether the rows run from the top (a negative `biHeight`).
    pub top_down: bool,
    /// Bits per pixel.
    pub bit_count: u16,
    /// The codec (`biCompression`): 0 is uncompressed.
    pub compression: u32,
    /// The colour masks that follow a `BI_BITFIELDS` header.
    pub masks: Option<[u32; 3]>,
    /// The colour table: opaque RGBA entries.
    pub palette: Vec<[u8; 4]>,
}

/// The sound format of an audio stream (`strf`, `WAVEFORMATEX`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WaveFormat {
    /// The codec (`wFormatTag`): 1 is PCM.
    pub tag: u16,
    /// Channels.
    pub channels: u16,
    /// Samples per second.
    pub sample_rate: u32,
    /// Bytes per block of samples (`nBlockAlign`).
    pub block_align: u16,
    /// Bits per sample.
    pub bits: u16,
    /// What follows the fixed fields (`cbSize` bytes).
    pub extra: Vec<u8>,
}

/// One chunk of the video stream in `movi`.
#[derive(Debug, Clone, Copy)]
struct VideoChunk {
    start: usize,
    len: usize,
    /// A palette change (`##pc`), not a frame.
    palette_change: bool,
    key: bool,
}

/// An AVI file read into its first video stream and first audio stream
/// (the two `CVidPlayer::Initialize` opens).
#[derive(Debug, Clone)]
pub struct AviFile {
    bytes: Vec<u8>,
    /// `dwMicroSecPerFrame` of the main header.
    pub micro_sec_per_frame: u32,
    /// The video stream's header.
    pub video_header: StreamHeader,
    /// The video stream's picture format.
    pub video_format: BitmapFormat,
    video_chunks: Vec<VideoChunk>,
    frames: u64,
    /// The audio stream's header and format, when the file has one.
    pub audio: Option<(StreamHeader, WaveFormat)>,
    audio_chunks: Vec<(usize, usize)>,
}

/// What is known of a stream while the headers are read.
#[derive(Default)]
struct StreamDraft {
    header: StreamHeader,
    format: Vec<u8>,
}

fn container(reason: &str) -> Error {
    Error::Container(reason.to_string())
}

/// The chunks of a list, between two offsets: `(id, body start, body
/// length)`, the length cut to what the list holds.
fn chunks(bytes: &[u8], mut at: usize, end: usize) -> impl Iterator<Item = (u32, usize, usize)> {
    let end = end.min(bytes.len());
    std::iter::from_fn(move || {
        let id = u32_at(bytes, at)?;
        let size = u32_at(bytes, at.checked_add(4)?)? as usize;
        let body = at + 8;
        if body > end {
            return None;
        }
        let len = size.min(end - body);
        // Chunks are aligned on two bytes.
        at = body.saturating_add(size).saturating_add(size & 1);
        Some((id, body, len))
    })
}

const fn fourcc(text: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*text)
}

const RIFF: u32 = fourcc(b"RIFF");
const LIST: u32 = fourcc(b"LIST");
const VIDS: u32 = fourcc(b"vids");
const AUDS: u32 = fourcc(b"auds");

/// The stream number a `movi` chunk's id starts with (two hexadecimal
/// digits), and its two-letter type.
fn stream_chunk(id: u32) -> Option<(u32, [u8; 2])> {
    let b = id.to_le_bytes();
    let hi = char::from(b[0]).to_digit(16)?;
    let lo = char::from(b[1]).to_digit(16)?;
    Some((hi * 16 + lo, [b[2], b[3]]))
}

impl AviFile {
    /// Reads the headers and the list of chunks of an AVI file.
    pub fn parse(bytes: Vec<u8>) -> Result<AviFile> {
        if !is_avi(&bytes) {
            return Err(container("not an AVI file"));
        }
        let mut micro_sec_per_frame = 0;
        let mut streams: Vec<StreamDraft> = Vec::new();
        // The `movi` lists: where their `movi` FourCC is, and their end.
        let mut movies: Vec<(usize, usize)> = Vec::new();
        let mut index: Option<(usize, usize)> = None;
        for (id, body, len) in chunks(&bytes, 0, bytes.len()) {
            if id != RIFF {
                // Whatever follows the RIFF segments is not ours.
                break;
            }
            let form = bytes.get(body..body + 4);
            if !form.is_some_and(|form| form == b"AVI " || form == b"AVIX") {
                continue;
            }
            for (id, body, len) in chunks(&bytes, body + 4, body + len) {
                if id == fourcc(b"idx1") {
                    index.get_or_insert((body, len));
                    continue;
                }
                if id != LIST || len < 4 {
                    continue;
                }
                match &bytes[body..body + 4] {
                    b"hdrl" if streams.is_empty() => {
                        for (id, body, len) in chunks(&bytes, body + 4, body + len) {
                            if id == fourcc(b"avih") {
                                micro_sec_per_frame = u32_at(&bytes, body).unwrap_or(0);
                            } else if id == LIST
                                && bytes.get(body..body + 4).is_some_and(|f| f == b"strl")
                            {
                                streams.push(read_stream(&bytes, body + 4, body + len));
                            }
                        }
                    }
                    b"movi" => movies.push((body, body + len)),
                    _ => {}
                }
            }
        }
        let video_number = streams
            .iter()
            .position(|s| s.header.kind == VIDS)
            .ok_or(Error::NoVideoStream)?;
        let audio_number = streams.iter().position(|s| s.header.kind == AUDS);
        let video_header = streams[video_number].header.clone();
        let video_format = read_bitmap_format(&streams[video_number].format)?;
        let audio = audio_number.and_then(|n| {
            let format = read_wave_format(&streams[n].format)?;
            Some((streams[n].header.clone(), format))
        });

        // The key frames the index names. Its offsets count from the
        // `movi` FourCC, or from the start of the file in some files:
        // the first entry tells which.
        let mut keys: HashSet<usize> = HashSet::new();
        let mut indexed = false;
        if let (Some((start, len)), Some(&(movi, _))) = (index, movies.first()) {
            let entry = |n: usize| {
                let at = start + n * 16;
                Some((
                    u32_at(&bytes, at)?,
                    u32_at(&bytes, at + 4)?,
                    u32_at(&bytes, at + 8)? as usize,
                ))
            };
            let base = match entry(0) {
                Some((id, _, offset))
                    if u32_at(&bytes, movi.saturating_add(offset)) == Some(id) =>
                {
                    movi
                }
                _ => 0,
            };
            for n in 0..len / 16 {
                let Some((_, flags, offset)) = entry(n) else {
                    break;
                };
                indexed = true;
                if flags & AVIIF_KEYFRAME != 0 {
                    keys.insert(base.saturating_add(offset));
                }
            }
        }

        let mut video_chunks = Vec::new();
        let mut audio_chunks = Vec::new();
        let mut frames = 0u64;
        for &(movi, end) in &movies {
            let mut visit = |id: u32, body: usize, len: usize| {
                let Some((number, kind)) = stream_chunk(id) else {
                    return;
                };
                if number as usize == video_number {
                    let palette_change = &kind == b"pc";
                    if !palette_change {
                        frames += 1;
                    }
                    video_chunks.push(VideoChunk {
                        start: body,
                        len,
                        palette_change,
                        key: !indexed || keys.contains(&(body - 8)),
                    });
                } else if Some(number as usize) == audio_number && audio.is_some() {
                    audio_chunks.push((body, len));
                }
            };
            walk_movie(&bytes, movi + 4, end, 0, &mut visit);
        }

        let mut file = AviFile {
            bytes,
            micro_sec_per_frame,
            video_header,
            video_format,
            video_chunks,
            frames,
            audio,
            audio_chunks,
        };
        // The frame rate: the stream's `dwRate / dwScale`, else the main
        // header's time per frame.
        if file.video_header.rate == 0 || file.video_header.scale == 0 {
            if micro_sec_per_frame == 0 {
                return Err(container("the video stream has no frame rate"));
            }
            file.video_header.rate = 1_000_000;
            file.video_header.scale = micro_sec_per_frame;
        }
        if file.duration_ms() > MAX_DURATION_MS {
            return Err(container("the video is longer than a day"));
        }
        Ok(file)
    }

    /// The number of frames of the video stream: its chunks in `movi`.
    pub fn frame_count(&self) -> u64 {
        self.frames
    }

    /// When a sample of the video stream starts, in milliseconds
    /// (`AVIStreamSampleToTime`).
    pub fn sample_time_ms(&self, sample: u64) -> u64 {
        let header = &self.video_header;
        let ms =
            u128::from(sample) * 1000 * u128::from(header.scale) / u128::from(header.rate.max(1));
        u64::try_from(ms).unwrap_or(u64::MAX)
    }

    /// When the video stream ends, in milliseconds
    /// (`AVIStreamEndTime`): the time of the sample after the last.
    pub fn duration_ms(&self) -> u64 {
        self.sample_time_ms(u64::from(self.video_header.start) + self.frames)
    }

    /// The bytes of the audio stream, chunk after chunk.
    pub fn audio_data(&self) -> Vec<u8> {
        let total = self.audio_chunks.iter().map(|(_, len)| len).sum();
        let mut out = Vec::with_capacity(total);
        for &(start, len) in &self.audio_chunks {
            out.extend_from_slice(&self.bytes[start..start + len]);
        }
        out
    }

    /// The sound track as a WAV file of PCM samples for
    /// `luma_audio::decode`: `None` without an audio stream (or with an
    /// empty one), an error naming the format tag when the codec is not
    /// one Luma decodes.
    pub fn sound_track(&self) -> Result<Option<Vec<u8>>> {
        let Some((_, format)) = &self.audio else {
            return Ok(None);
        };
        let data = self.audio_data();
        if data.is_empty() {
            return Ok(None);
        }
        sound::to_wav(format, &data).map(Some)
    }
}

/// Visits the stream chunks of a `movi` list, `rec ` groups included.
fn walk_movie(
    bytes: &[u8],
    start: usize,
    end: usize,
    depth: u32,
    visit: &mut impl FnMut(u32, usize, usize),
) {
    for (id, body, len) in chunks(bytes, start, end) {
        if id == LIST {
            if depth < MAX_DEPTH && len >= 4 {
                walk_movie(bytes, body + 4, body + len, depth + 1, visit);
            }
        } else {
            visit(id, body, len);
        }
    }
}

/// Reads a stream list (`strl`): its header and its format.
fn read_stream(bytes: &[u8], start: usize, end: usize) -> StreamDraft {
    let mut draft = StreamDraft::default();
    for (id, body, len) in chunks(bytes, start, end) {
        if id == fourcc(b"strh") {
            let field = |n: usize| {
                if n * 4 + 4 <= len {
                    u32_at(bytes, body + n * 4).unwrap_or(0)
                } else {
                    0
                }
            };
            draft.header = StreamHeader {
                kind: field(0),
                handler: field(1),
                scale: field(5),
                rate: field(6),
                start: field(7),
                length: field(8),
                sample_size: field(11),
            };
        } else if id == fourcc(b"strf") {
            draft.format = bytes[body..body + len].to_vec();
        }
    }
    draft
}

/// Reads a `BITMAPINFOHEADER`, its masks and its colour table.
fn read_bitmap_format(format: &[u8]) -> Result<BitmapFormat> {
    let field = |at: usize| u32_at(format, at).ok_or_else(|| container("short video format"));
    let size = field(0)? as usize;
    if size < 40 {
        return Err(container("short video format"));
    }
    let width = field(4)? as i32;
    let raw_height = field(8)? as i32;
    let bit_count = u16_at(format, 14).ok_or_else(|| container("short video format"))?;
    let compression = field(16)?;
    let colors_used = field(32)? as usize;
    let height = raw_height.unsigned_abs();
    if width <= 0 || height == 0 || width as u32 > MAX_SIDE || height > MAX_SIDE {
        return Err(container("absurd picture size"));
    }
    if u64::from(width as u32) * u64::from(height) > MAX_PIXELS {
        return Err(container("absurd picture size"));
    }
    let mut after = size.min(format.len());
    let masks = if compression == 3 && size == 40 {
        let masks = [
            u32_at(format, after),
            u32_at(format, after + 4),
            u32_at(format, after + 8),
        ];
        after += 12;
        match masks {
            [Some(r), Some(g), Some(b)] => Some([r, g, b]),
            _ => None,
        }
    } else if compression == 3 {
        // A longer header holds its masks itself (`BITMAPV4HEADER`).
        match [u32_at(format, 40), u32_at(format, 44), u32_at(format, 48)] {
            [Some(r), Some(g), Some(b)] => Some([r, g, b]),
            _ => None,
        }
    } else {
        None
    };
    let mut palette = Vec::new();
    if bit_count <= 8 {
        let count = match colors_used {
            0 => 1usize << bit_count,
            n => n.min(256),
        };
        for n in 0..count {
            let Some(entry) = format.get(after + n * 4..after + n * 4 + 4) else {
                break;
            };
            palette.push([entry[2], entry[1], entry[0], 0xff]);
        }
    }
    Ok(BitmapFormat {
        width: width as u32,
        height,
        top_down: raw_height < 0,
        bit_count,
        compression,
        masks,
        palette,
    })
}

/// Reads a `WAVEFORMATEX` (or the shorter `WAVEFORMAT`).
fn read_wave_format(format: &[u8]) -> Option<WaveFormat> {
    let extra_len = u16_at(format, 16).unwrap_or(0) as usize;
    let extra = format
        .get(18..)
        .map(|rest| rest[..extra_len.min(rest.len())].to_vec())
        .unwrap_or_default();
    Some(WaveFormat {
        tag: u16_at(format, 0)?,
        channels: u16_at(format, 2)?,
        sample_rate: u32_at(format, 4)?,
        block_align: u16_at(format, 12)?,
        bits: u16_at(format, 14).unwrap_or(8),
        extra,
    })
}

/// The decoder of a video stream, with what it keeps between frames.
enum Decoder {
    /// Uncompressed pictures (`BI_RGB`, `BI_BITFIELDS`).
    Dib(dib::Layout),
    /// Microsoft RLE, 4 or 8 bits: palette indices, the unchanged pixels
    /// kept from frame to frame.
    Rle { bits: u8, indices: Vec<u8> },
    /// Motion JPEG.
    Mjpeg,
    /// Microsoft Video 1.
    Video1(msvideo1::State),
    /// Cinepak.
    Cinepak(Box<cinepak::State>),
}

impl Decoder {
    /// Whether a frame builds on the one before it, so that none may be
    /// passed over.
    fn is_delta(&self) -> bool {
        !matches!(self, Decoder::Dib(_) | Decoder::Mjpeg)
    }
}

/// The codecs Luma decodes, as `docs/video.md` lists them.
fn decoder_for(format: &BitmapFormat, handler: u32) -> Result<Decoder> {
    let (width, height) = (format.width as usize, format.height as usize);
    let code = format.compression;
    let mut name = code.to_le_bytes();
    name.make_ascii_uppercase();
    let unsupported = || {
        let mut text = format!("video codec '{}'", fourcc_name(code));
        if handler != 0 && handler != code {
            text.push_str(&format!(" (handler '{}')", fourcc_name(handler)));
        }
        Error::UnsupportedCodec(text)
    };
    match (code, &name) {
        (0 | 3, _) | (_, b"DIB " | b"RGB " | b"RAW ") => {
            let masks = if code == 3 { format.masks } else { None };
            dib::Layout::new(width, height, format.bit_count, format.top_down, masks)
                .map(Decoder::Dib)
                .ok_or_else(|| {
                    Error::UnsupportedCodec(format!(
                        "uncompressed video with {} bits per pixel",
                        format.bit_count
                    ))
                })
        }
        (1, _) | (_, b"RLE " | b"MRLE") if format.bit_count == 8 => Ok(Decoder::Rle {
            bits: 8,
            indices: vec![0; width * height],
        }),
        (2, _) | (_, b"RLE " | b"MRLE") if format.bit_count == 4 => Ok(Decoder::Rle {
            bits: 4,
            indices: vec![0; width * height],
        }),
        (_, b"MJPG" | b"JPEG") => Ok(Decoder::Mjpeg),
        (_, b"CRAM" | b"MSVC" | b"WHAM") => msvideo1::State::new(width, height, format.bit_count)
            .map(Decoder::Video1)
            .ok_or_else(unsupported),
        (_, b"CVID") => Ok(Decoder::Cinepak(Box::new(cinepak::State::new(
            width, height,
        )))),
        _ => Err(unsupported()),
    }
}

/// The video stream of an AVI file being decoded, frame after frame
/// (what `AVIStreamGetFrame` gives the original player).
pub struct AviVideo {
    file: AviFile,
    info: VideoInfo,
    decoder: Decoder,
    palette: [[u8; 4]; 256],
    /// The next chunk of the video stream to read.
    chunk: usize,
    next_index: u64,
    /// The last picture decoded and the index of its frame.
    last: Option<(u64, Vec<u8>)>,
}

impl std::fmt::Debug for AviVideo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AviVideo")
            .field("info", &self.info)
            .field("format", &self.file.video_format)
            .field("next_index", &self.next_index)
            .finish_non_exhaustive()
    }
}

/// An opaque black picture of that many pixels.
fn black(pixels: usize) -> Vec<u8> {
    let mut rgba = vec![0; pixels * 4];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = 0xff;
    }
    rgba
}

fn start_palette(format: &BitmapFormat) -> [[u8; 4]; 256] {
    let mut palette = [[0, 0, 0, 0xff]; 256];
    for (slot, entry) in palette.iter_mut().zip(&format.palette) {
        *slot = *entry;
    }
    palette
}

impl AviVideo {
    /// Opens the video stream of an AVI file from the file's bytes. An
    /// [`Error::UnsupportedCodec`] names the codec Luma cannot decode.
    pub fn open(bytes: Vec<u8>) -> Result<AviVideo> {
        AviVideo::from_file(AviFile::parse(bytes)?)
    }

    /// Opens the video stream of a file already read.
    pub fn from_file(file: AviFile) -> Result<AviVideo> {
        let decoder = decoder_for(&file.video_format, file.video_header.handler)?;
        let format = &file.video_format;
        let info = VideoInfo {
            width: format.width,
            height: format.height,
            coded_width: format.width,
            coded_height: format.height,
            pic_x: 0,
            pic_y: 0,
            before_3_2_1: false,
            fps_num: file.video_header.rate,
            fps_den: file.video_header.scale,
            has_audio: file.audio.is_some(),
        };
        let palette = start_palette(format);
        Ok(AviVideo {
            file,
            info,
            decoder,
            palette,
            chunk: 0,
            next_index: 0,
            last: None,
        })
    }

    /// The file the stream is read from.
    pub fn file(&self) -> &AviFile {
        &self.file
    }

    /// The stream sample the first frame stands for (`dwStart`): frame
    /// `n` is sample `first_sample() + n`.
    pub fn first_sample(&self) -> u64 {
        u64::from(self.file.video_header.start)
    }

    /// The number of frames.
    pub fn frame_count(&self) -> u64 {
        self.file.frame_count()
    }

    /// When the stream ends, in milliseconds (`AVIStreamEndTime`).
    pub fn duration_ms(&self) -> u64 {
        self.file.duration_ms()
    }

    /// The sound track as a WAV file; see [`AviFile::sound_track`].
    pub fn sound_track(&self) -> Result<Option<Vec<u8>>> {
        self.file.sound_track()
    }

    /// A palette change (`##pc`, `AVIPALCHANGE`): the first entry, the
    /// number of entries (0 for 256), two bytes of flags, then the
    /// entries as red, green, blue, flags.
    fn change_palette(&mut self, data: &[u8]) {
        let (Some(&first), Some(&count)) = (data.first(), data.get(1)) else {
            return;
        };
        let count = if count == 0 { 256 } else { usize::from(count) };
        for (n, entry) in data[4.min(data.len())..]
            .chunks_exact(4)
            .take(count)
            .enumerate()
        {
            if let Some(slot) = self.palette.get_mut(usize::from(first) + n) {
                *slot = [entry[0], entry[1], entry[2], 0xff];
            }
        }
    }

    /// The next frame chunk, the palette changes before it applied.
    fn next_chunk(&mut self) -> Option<VideoChunk> {
        loop {
            let chunk = *self.file.video_chunks.get(self.chunk)?;
            self.chunk += 1;
            if chunk.palette_change {
                let data = self.file.bytes[chunk.start..chunk.start + chunk.len].to_vec();
                self.change_palette(&data);
                continue;
            }
            return Some(chunk);
        }
    }

    /// Decodes a frame chunk to RGBA.
    fn decode(&mut self, chunk: VideoChunk) -> Result<Vec<u8>> {
        let data = &self.file.bytes[chunk.start..chunk.start + chunk.len];
        let (width, height) = (self.info.width as usize, self.info.height as usize);
        let mut rgba = black(width * height);
        match &mut self.decoder {
            Decoder::Dib(layout) => layout.decode(&self.palette, data, &mut rgba)?,
            Decoder::Rle { bits, indices } => {
                rle::decode(*bits, width, height, data, indices);
                // The rows are stored from the bottom.
                for (y, row) in indices.chunks_exact(width).enumerate() {
                    let out = &mut rgba[(height - 1 - y) * width * 4..][..width * 4];
                    for (pixel, index) in out.chunks_exact_mut(4).zip(row) {
                        pixel.copy_from_slice(&self.palette[usize::from(*index)]);
                    }
                }
            }
            Decoder::Mjpeg => mjpeg::decode(width, height, data, &mut rgba)?,
            Decoder::Video1(state) => state.decode(&self.palette, data, &mut rgba),
            Decoder::Cinepak(state) => state.decode(data, &mut rgba)?,
        }
        Ok(rgba)
    }

    /// The picture an empty chunk repeats when the frames before it
    /// were passed over: the last one with data before `chunk`.
    fn picture_before(&mut self, chunk: usize) -> Result<Option<Vec<u8>>> {
        let earlier = self.file.video_chunks[..chunk.min(self.file.video_chunks.len())]
            .iter()
            .rev()
            .find(|c| !c.palette_change && c.len != 0)
            .copied();
        match earlier {
            Some(chunk) => self.decode(chunk).map(Some),
            None => Ok(None),
        }
    }
}

impl FrameSource for AviVideo {
    fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn next_index(&self) -> u64 {
        self.next_index
    }

    /// Decodes the next frame; `None` after the last. A chunk without
    /// data repeats the picture before it (a frame the encoder dropped).
    /// After an error the stream stands on the frame after the bad one.
    fn next_frame(&mut self) -> Result<Option<Frame>> {
        let Some(chunk) = self.next_chunk() else {
            return Ok(None);
        };
        let index = self.next_index;
        self.next_index += 1;
        let rgba = if chunk.len == 0 {
            match self.last.take() {
                Some((at, rgba)) if at + 1 == index || self.decoder.is_delta() => rgba,
                _ => match self.picture_before(self.chunk - 1)? {
                    Some(rgba) => rgba,
                    None => black(self.info.width as usize * self.info.height as usize),
                },
            }
        } else {
            self.decode(chunk)?
        };
        self.last = Some((index, rgba.clone()));
        Ok(Some(Frame {
            index,
            time_ms: self
                .file
                .sample_time_ms(self.first_sample().saturating_add(index)),
            keyframe: chunk.key,
            width: self.info.width,
            height: self.info.height,
            rgba,
        }))
    }

    /// Passes over the frames before `index`. The frames of a codec
    /// whose pictures stand alone are not decoded; the others are, since
    /// each builds on the one before.
    fn skip_to(&mut self, index: u64) -> Result<()> {
        while self.next_index < index {
            if self.decoder.is_delta() {
                // A bad frame leaves the picture as it was.
                match self.next_frame() {
                    Ok(Some(_)) | Err(_) => {}
                    Ok(None) => break,
                }
            } else {
                if self.next_chunk().is_none() {
                    break;
                }
                self.next_index += 1;
            }
        }
        Ok(())
    }

    fn rewind(&mut self) -> Result<()> {
        self.decoder = decoder_for(&self.file.video_format, self.file.video_header.handler)?;
        self.palette = start_palette(&self.file.video_format);
        self.chunk = 0;
        self.next_index = 0;
        self.last = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn code(text: &[u8; 4]) -> u32 {
        u32::from_le_bytes(*text)
    }

    /// A 2x2 uncompressed 24-bit frame of one colour (blue, green, red).
    fn plain(b: u8, g: u8, r: u8) -> Vec<u8> {
        [b, g, r, b, g, r, 0, 0].repeat(2)
    }

    fn spec(frames: Vec<Vec<u8>>) -> AviSpec {
        AviSpec {
            width: 2,
            height: 2,
            bit_count: 24,
            rate: 15,
            scale: 1,
            frames,
            index: true,
            ..AviSpec::default()
        }
    }

    fn pcm(ms: u32) -> AviSound {
        AviSound {
            tag: 1,
            channels: 1,
            sample_rate: 8000,
            block_align: 2,
            bits: 16,
            extra: Vec::new(),
            chunks: vec![vec![0; ms as usize * 4]; 4],
        }
    }

    /// The first pixel of each frame, and the frames' times.
    fn play(video: &mut AviVideo) -> Vec<([u8; 3], u64)> {
        let mut out = Vec::new();
        while let Some(frame) = video.next_frame().unwrap() {
            assert_eq!(frame.index as usize, out.len());
            assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
            out.push(([frame.rgba[0], frame.rgba[1], frame.rgba[2]], frame.time_ms));
        }
        out
    }

    #[test]
    fn frames_times_and_sound_of_an_uncompressed_file() {
        let mut file = spec(vec![plain(1, 2, 3), plain(4, 5, 6), plain(7, 8, 9)]);
        file.sound = Some(pcm(200));
        file.delta_frames = vec![1];
        let bytes = build_avi(&file);
        assert!(is_avi(&bytes));
        let mut video = AviVideo::open(bytes).unwrap();
        let info = video.info().clone();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!((info.coded_width, info.coded_height), (2, 2));
        assert_eq!((info.fps_num, info.fps_den, info.has_audio), (15, 1, true));
        assert_eq!(video.frame_count(), 3);
        // 15 frames per second: 0, 66 and 133 ms; the end at 200 ms.
        assert_eq!(video.duration_ms(), 200);
        assert_eq!(info.frame_at_ms(67), 1);
        assert_eq!(info.frame_at_ms(66), 0);
        let first = video.next_frame().unwrap().unwrap();
        assert!(first.keyframe);
        let second = video.next_frame().unwrap().unwrap();
        assert!(!second.keyframe, "the index does not flag it");
        video.rewind().unwrap();
        assert_eq!(
            play(&mut video),
            [([3, 2, 1], 0), ([6, 5, 4], 66), ([9, 8, 7], 133)]
        );
        assert!(video.next_frame().unwrap().is_none());
        // The sound: four chunks, 200 ms in all, as a WAV file.
        let wav = video.sound_track().unwrap().unwrap();
        let clip = luma_audio::decode(&wav).unwrap();
        assert_eq!((clip.sample_rate(), clip.duration_ms()), (8000, 200));
    }

    #[test]
    fn the_layout_of_the_file_does_not_matter() {
        let frames = vec![plain(1, 2, 3), plain(4, 5, 6)];
        let expected = [([3, 2, 1], 0), ([6, 5, 4], 66)];
        // Without an index: every frame is a key frame.
        let mut file = spec(frames.clone());
        file.index = false;
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        assert!(video.next_frame().unwrap().unwrap().keyframe);
        video.rewind().unwrap();
        assert_eq!(play(&mut video), expected);
        // Frames grouped in `rec ` lists, the sound in stream 0.
        let mut file = spec(frames.clone());
        file.records = true;
        file.sound = Some(pcm(10));
        file.sound_first = true;
        file.delta_frames = vec![1];
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        assert_eq!(video.file().audio_data().len(), 160);
        assert!(video.next_frame().unwrap().unwrap().keyframe);
        assert!(!video.next_frame().unwrap().unwrap().keyframe);
        // An index whose offsets count from the start of the file.
        let mut file = spec(frames.clone());
        file.delta_frames = vec![0];
        let mut bytes = build_avi(&file);
        let movi = bytes.windows(4).position(|w| w == b"movi").unwrap() as u32;
        let index = bytes.windows(4).position(|w| w == b"idx1").unwrap() + 8;
        for entry in 0..2 {
            let at = index + entry * 16 + 8;
            let offset = u32_at(&bytes, at).unwrap() + movi;
            bytes[at..at + 4].copy_from_slice(&offset.to_le_bytes());
        }
        let mut video = AviVideo::open(bytes).unwrap();
        assert!(!video.next_frame().unwrap().unwrap().keyframe);
        assert!(video.next_frame().unwrap().unwrap().keyframe);
        // A stream that starts late, and a frame rate given by the main
        // header only (66666 microseconds per frame).
        let mut file = spec(frames.clone());
        file.start = 3;
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        assert_eq!(video.first_sample(), 3);
        assert_eq!(video.duration_ms(), 333);
        assert_eq!(play(&mut video), [([3, 2, 1], 200), ([6, 5, 4], 266)]);
        let mut bytes = build_avi(&spec(frames));
        let header = bytes.windows(4).position(|w| w == b"vids").unwrap();
        bytes[header + 20..header + 28].fill(0);
        let video = AviVideo::open(bytes.clone()).unwrap();
        assert_eq!(
            (video.info().fps_num, video.info().fps_den),
            (1_000_000, 66_666)
        );
        assert_eq!(video.duration_ms(), 133);
        // Without either there is no clock to play on.
        let main = bytes.windows(4).position(|w| w == b"avih").unwrap();
        bytes[main + 8..main + 12].fill(0);
        assert!(matches!(AviVideo::open(bytes), Err(Error::Container(_))));
    }

    #[test]
    fn palettes_change_between_frames() {
        // 4x1, 8 bits; the palette is blue, green, red, zero.
        let mut file = spec(vec![vec![0, 1, 1, 0], vec![0, 1, 1, 0]]);
        file.width = 4;
        file.height = 1;
        file.bit_count = 8;
        file.palette = vec![[1, 2, 3, 0], [4, 5, 6, 0]];
        // Before the second frame, entry 1 becomes red 9, green 8,
        // blue 7.
        file.palette_changes = vec![(1, vec![1, 1, 0, 0, 9, 8, 7, 0])];
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        assert_eq!(video.frame_count(), 2);
        let first = video.next_frame().unwrap().unwrap();
        assert_eq!(first.rgba[..8], [3, 2, 1, 0xff, 6, 5, 4, 0xff]);
        let second = video.next_frame().unwrap().unwrap();
        assert_eq!(second.rgba[..8], [3, 2, 1, 0xff, 9, 8, 7, 0xff]);
        // The change is applied when the frames before it are passed
        // over, and undone by a rewind.
        video.rewind().unwrap();
        video.skip_to(1).unwrap();
        assert_eq!(video.next_frame().unwrap().unwrap().rgba[4..7], [9, 8, 7]);
        video.rewind().unwrap();
        assert_eq!(video.next_frame().unwrap().unwrap().rgba[4..7], [6, 5, 4]);
    }

    #[test]
    fn compressed_streams_decode() {
        // Motion JPEG without Huffman tables.
        let mut rgb = Vec::new();
        for y in 0..16u32 {
            for x in 0..16u32 {
                rgb.extend_from_slice(&[(x * 16) as u8, (y * 16) as u8, 128]);
            }
        }
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode(&rgb, 16, 16, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut file = spec(vec![mjpeg::without_tables(&jpeg)]);
        (file.width, file.height) = (16, 16);
        file.compression = code(b"MJPG");
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        let frame = video.next_frame().unwrap().unwrap();
        for (ours, theirs) in frame.rgba.chunks_exact(4).zip(rgb.chunks_exact(3)) {
            for channel in 0..3 {
                assert!(
                    ours[channel].abs_diff(theirs[channel]) < 24,
                    "{ours:?} {theirs:?}"
                );
            }
        }
        // Microsoft RLE: the second frame changes one pixel.
        let mut file = spec(vec![vec![4, 1, 0, 1], vec![0, 2, 2, 0, 1, 0, 0, 1]]);
        (file.width, file.height) = (4, 1);
        file.bit_count = 8;
        file.compression = 1;
        file.palette = vec![[0, 0, 10, 0], [0, 0, 20, 0]];
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        let reds = |frame: Frame| -> Vec<u8> { frame.rgba.chunks_exact(4).map(|p| p[0]).collect() };
        assert_eq!(reds(video.next_frame().unwrap().unwrap()), [20, 20, 20, 20]);
        assert_eq!(reds(video.next_frame().unwrap().unwrap()), [20, 20, 10, 20]);
        // Passing over a frame of such a stream still decodes it.
        video.rewind().unwrap();
        video.skip_to(1).unwrap();
        assert_eq!(reds(video.next_frame().unwrap().unwrap()), [20, 20, 10, 20]);
        // Microsoft Video 1 and Cinepak: a block of one colour.
        let mut file = spec(vec![vec![0x1f, 0x80]]);
        (file.width, file.height) = (4, 4);
        file.bit_count = 16;
        file.compression = code(b"CRAM");
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        assert_eq!(
            video.next_frame().unwrap().unwrap().rgba[..4],
            [0, 0, 255, 0xff]
        );
        let cinepak = [
            &[0, 0, 0, 40, 0, 4, 0, 4, 0, 1][..],
            &[0x10, 0, 0, 30, 0, 0, 0, 0, 0, 4, 0, 4],
            &[0x26, 0, 0, 8, 9, 9, 9, 9],
            &[0x32, 0, 0, 5, 0],
        ]
        .concat();
        let mut file = spec(vec![cinepak, Vec::new()]);
        (file.width, file.height) = (4, 4);
        file.compression = code(b"cvid");
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        assert_eq!(
            video.next_frame().unwrap().unwrap().rgba[..4],
            [9, 9, 9, 0xff]
        );
        // A chunk without data repeats the picture.
        assert_eq!(
            video.next_frame().unwrap().unwrap().rgba[..4],
            [9, 9, 9, 0xff]
        );
    }

    #[test]
    fn an_empty_chunk_repeats_the_frame_before_it() {
        let file = spec(vec![
            Vec::new(),
            plain(1, 2, 3),
            Vec::new(),
            plain(4, 5, 6),
            Vec::new(),
        ]);
        let bytes = build_avi(&file);
        let mut video = AviVideo::open(bytes).unwrap();
        let colours: Vec<[u8; 3]> = play(&mut video).into_iter().map(|(c, _)| c).collect();
        assert_eq!(
            colours,
            [[0, 0, 0], [3, 2, 1], [3, 2, 1], [6, 5, 4], [6, 5, 4]],
            "black before the first picture"
        );
        // The same when the frames before were passed over undecoded.
        video.rewind().unwrap();
        video.skip_to(4).unwrap();
        assert_eq!(video.next_index(), 4);
        assert_eq!(video.next_frame().unwrap().unwrap().rgba[..3], [6, 5, 4]);
        video.rewind().unwrap();
        video.skip_to(2).unwrap();
        assert_eq!(video.next_frame().unwrap().unwrap().rgba[..3], [3, 2, 1]);
        // Past the end.
        video.skip_to(99).unwrap();
        assert!(video.next_frame().unwrap().is_none());
    }

    #[test]
    fn a_bad_frame_is_an_error_and_the_stream_goes_on() {
        let file = spec(vec![plain(1, 2, 3), vec![1, 2, 3], plain(4, 5, 6)]);
        let mut video = AviVideo::open(build_avi(&file)).unwrap();
        assert!(video.next_frame().unwrap().is_some());
        assert!(matches!(video.next_frame(), Err(Error::Decode(_))));
        assert_eq!(video.next_frame().unwrap().unwrap().index, 2);
    }

    #[test]
    fn codecs_luma_does_not_decode_are_named() {
        let name = |compression: u32, bits: u16| {
            let mut file = spec(vec![vec![0; 16]]);
            file.compression = compression;
            file.bit_count = bits;
            match AviVideo::open(build_avi(&file)) {
                Err(Error::UnsupportedCodec(name)) => name,
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(name(code(b"DIVX"), 24), "video codec 'DIVX'");
        assert_eq!(name(code(b"IV50"), 24), "video codec 'IV50'");
        assert_eq!(name(code(b"MP42"), 24), "video codec 'MP42'");
        assert_eq!(name(0x1000_0001, 24), "video codec '0x10000001'");
        assert_eq!(name(0, 12), "uncompressed video with 12 bits per pixel");
        assert_eq!(name(code(b"CRAM"), 24), "video codec 'CRAM'");
        // The sound: the video still opens, the track names its format.
        let mut file = spec(vec![plain(1, 2, 3)]);
        let mut sound = pcm(10);
        sound.tag = 0x55;
        file.sound = Some(sound);
        let video = AviVideo::open(build_avi(&file)).unwrap();
        assert_eq!(
            video.sound_track(),
            Err(Error::UnsupportedCodec(
                "sound format tag 0x0055 (MP3)".into()
            ))
        );
        // The handler is named too when it differs.
        let mut bytes = build_avi(&{
            let mut file = spec(vec![vec![0; 16]]);
            file.compression = code(b"DX50");
            file
        });
        let header = bytes.windows(4).position(|w| w == b"vids").unwrap();
        bytes[header + 4..header + 8].copy_from_slice(b"divx");
        assert_eq!(
            AviVideo::open(bytes).unwrap_err(),
            Error::UnsupportedCodec("video codec 'DX50' (handler 'divx')".into())
        );
    }

    #[test]
    fn files_that_are_not_videos_are_refused() {
        let refused = |bytes: Vec<u8>| matches!(AviVideo::open(bytes), Err(Error::Container(_)));
        assert!(refused(Vec::new()));
        assert!(refused(b"RIFF\x04\0\0\0WAVE".to_vec()));
        assert!(matches!(
            AviVideo::open(b"RIFF\x04\0\0\0AVI ".to_vec()),
            Err(Error::NoVideoStream)
        ));
        // Absurd sizes: a picture too large, a video longer than a day.
        let mut file = spec(vec![plain(1, 2, 3)]);
        file.width = 100_000;
        assert!(refused(build_avi(&file)));
        let mut file = spec(vec![plain(1, 2, 3)]);
        file.width = 8192;
        file.height = 8192;
        assert!(refused(build_avi(&file)));
        let mut file = spec(vec![plain(1, 2, 3)]);
        file.start = u32::MAX;
        assert!(refused(build_avi(&file)));
        let mut file = spec(vec![plain(1, 2, 3)]);
        (file.rate, file.scale) = (1, 100_000);
        assert!(refused(build_avi(&file)));
        assert_eq!(fourcc_name(code(b"cvid")), "cvid");
        assert_eq!(fourcc_name(1), "0x00000001");
    }

    /// A small file of each codec, to damage.
    fn samples() -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut file = spec(vec![plain(1, 2, 3), Vec::new(), plain(4, 5, 6)]);
        file.sound = Some(pcm(10));
        file.palette_changes = vec![(1, vec![0, 0, 0, 0, 1, 2, 3, 0])];
        out.push(build_avi(&file));
        file.records = true;
        file.sound_first = true;
        out.push(build_avi(&file));
        let mut file = spec(vec![vec![4, 1, 0, 0, 0, 3, 1, 0, 1, 0, 0, 1]; 2]);
        (file.width, file.height, file.bit_count, file.compression) = (4, 4, 8, 1);
        file.palette = vec![[1, 2, 3, 0]; 2];
        out.push(build_avi(&file));
        file.bit_count = 4;
        file.compression = 2;
        out.push(build_avi(&file));
        file.bit_count = 8;
        file.compression = code(b"MSVC");
        out.push(build_avi(&file));
        file.bit_count = 16;
        out.push(build_avi(&file));
        file.compression = code(b"cvid");
        file.frames = vec![
            [
                &[0, 0, 0, 40, 0, 4, 0, 4, 0, 1][..],
                &[0x10, 0, 0, 30, 0, 0, 0, 0, 0, 4, 0, 4],
                &[0x20, 0, 0, 10, 9, 9, 9, 9, 1, 2],
                &[0x30, 0, 0, 12, 0x80, 0, 0, 0, 0, 0, 0, 0],
            ]
            .concat();
            2
        ];
        out.push(build_avi(&file));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[90; 4 * 4 * 3], 4, 4, image::ExtendedColorType::Rgb8)
            .unwrap();
        file.compression = code(b"MJPG");
        file.frames = vec![mjpeg::without_tables(&jpeg), jpeg];
        out.push(build_avi(&file));
        let mut file = spec(vec![plain(1, 2, 3)]);
        let mut sound = pcm(10);
        (sound.tag, sound.block_align, sound.bits) = (2, 16, 4);
        file.sound = Some(sound.clone());
        out.push(build_avi(&file));
        sound.tag = 0x11;
        sound.channels = 2;
        file.sound = Some(sound);
        out.push(build_avi(&file));
        out
    }

    /// Opens a file and reads all of it; nothing may panic.
    fn exercise(bytes: Vec<u8>) {
        let Ok(mut video) = AviVideo::open(bytes) else {
            return;
        };
        let _ = video.sound_track();
        for _ in 0..16 {
            if let Ok(None) = video.next_frame() {
                break;
            }
        }
        let _ = video.rewind();
        let _ = video.skip_to(2);
        let _ = video.next_frame();
    }

    #[test]
    fn damaged_files_do_not_panic() {
        for sample in samples() {
            exercise(sample.clone());
            // Cut anywhere.
            for cut in 0..sample.len() {
                exercise(sample[..cut].to_vec());
            }
            // Any byte changed, to values that make sizes and counts
            // lie.
            for at in 0..sample.len() {
                for value in [0x00, 0x7f, 0x80, 0xff] {
                    let mut damaged = sample.clone();
                    damaged[at] = value;
                    exercise(damaged);
                }
            }
        }
    }
}
