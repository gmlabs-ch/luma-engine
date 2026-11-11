//! Video playback: Ogg Theora files decoded to RGBA frames, one at a
//! time, with the timing a player needs.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! The Ogg container is read with the `ogg` crate and the Theora stream
//! decoded with `oxideav-theora`, both pure Rust. The sound track (Vorbis)
//! is not decoded here: [`sound_track`] hands it out as an Ogg file of
//! its own for `luma_audio::decode`. The YUV to RGB conversion uses the fixed-point tables of the
//! original engine (`VidTheoraPlayer.cpp` of Wintermute Engine 1.x, MIT
//! License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`. How the
//! engine plays videos is documented in `docs/video.md`.
//!
//! ```
//! let bytes = luma_video::synthetic_clip(32, 32, 3, 10);
//! let mut video = luma_video::Video::open(bytes)?;
//! assert_eq!((video.info().width, video.info().height), (32, 32));
//! let frame = video.next_frame()?.unwrap();
//! assert_eq!(frame.rgba.len(), 32 * 32 * 4);
//! # Ok::<(), luma_video::Error>(())
//! ```

#![forbid(unsafe_code)]

mod error;

use std::io::Cursor;

use ogg::PacketReader;
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use oxideav_core::{CodecId, Decoder, Encoder, Frame as CoreFrame, Packet, TimeBase};
use oxideav_theora::{
    ColorSpace, PixelFormat, THEORA_CODEC_ID, TheoraDecoder, TheoraEncoder, TheoraIdentHeader,
    decode_identification_header,
};

pub use error::{Error, Result};

/// What a video is: its picture size, frame rate and whether it has a
/// sound track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoInfo {
    /// Width of the picture in pixels.
    pub width: u32,
    /// Height of the picture in pixels.
    pub height: u32,
    /// Width of the coded frame: a multiple of 16 that holds the
    /// picture. The original engine's texture has this size.
    pub coded_width: u32,
    /// Height of the coded frame.
    pub coded_height: u32,
    /// Where the picture's left edge is in the coded frame.
    pub pic_x: u32,
    /// Where the picture's top edge is in the coded frame, from the top
    /// (the stream states it from the bottom).
    pub pic_y: u32,
    /// Whether the stream is older than bitstream 3.2.1: its frames are
    /// then timed one period later (see [`VideoInfo::shown_at_ms`]).
    pub before_3_2_1: bool,
    /// Frame rate numerator.
    pub fps_num: u32,
    /// Frame rate denominator.
    pub fps_den: u32,
    /// Whether the file also carries a Vorbis sound track.
    pub has_audio: bool,
}

impl VideoInfo {
    /// The description of a stream from its identification header.
    fn from_ident(ident: &TheoraIdentHeader, has_audio: bool) -> VideoInfo {
        let coded_width = u32::from(ident.fmbw) * 16;
        let coded_height = u32::from(ident.fmbh) * 16;
        VideoInfo {
            width: ident.picw,
            height: ident.pich,
            coded_width,
            coded_height,
            pic_x: u32::from(ident.picx),
            pic_y: coded_height
                .saturating_sub(ident.pich)
                .saturating_sub(u32::from(ident.picy)),
            before_3_2_1: (ident.vmaj, ident.vmin, ident.vrev) < (3, 2, 1),
            fps_num: ident.frn,
            fps_den: ident.frd,
            has_audio,
        }
    }

    /// When the frame numbered `number` is shown, in milliseconds: the
    /// time the original engine's decoder library (libtheora 1.0,
    /// `theora_granule_time` of the decoder's own position) gives it.
    /// That is the start of the frame's interval, `number` periods, for a
    /// stream of bitstream 3.2.1 or later, and its end, one period
    /// later, for an older stream.
    pub fn shown_at_ms(&self, number: u64) -> u64 {
        self.frame_time_ms(number.saturating_add(u64::from(self.before_3_2_1)))
    }

    /// When the frame numbered `index` is shown, in milliseconds from
    /// the start.
    pub fn frame_time_ms(&self, index: u64) -> u64 {
        if self.fps_num == 0 {
            return 0;
        }
        let ms = u128::from(index) * 1000 * u128::from(self.fps_den) / u128::from(self.fps_num);
        u64::try_from(ms).unwrap_or(u64::MAX)
    }

    /// The frame shown at a time, in milliseconds from the start.
    pub fn frame_at_ms(&self, ms: u64) -> u64 {
        if self.fps_den == 0 {
            return 0;
        }
        ms * u64::from(self.fps_num) / (1000 * u64::from(self.fps_den))
    }
}

/// One decoded frame, as RGBA pixels of the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Index of the frame among the packets of the stream, from 0.
    pub index: u64,
    /// When the frame is shown, in milliseconds from the start: the time
    /// of the frame number the stream's granule positions give it, which
    /// runs ahead of `index` in a stream that leaves frames out.
    pub time_ms: u64,
    /// Whether the frame is a key frame.
    pub keyframe: bool,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes, opaque.
    pub rgba: Vec<u8>,
}

/// A Theora video being read.
pub struct Video {
    reader: PacketReader<Cursor<Vec<u8>>>,
    bytes_len: usize,
    serial: u32,
    headers: [Vec<u8>; 3],
    decoder: TheoraDecoder,
    ident: TheoraIdentHeader,
    info: VideoInfo,
    next_index: u64,
    /// The frame number of the last data packet, on the stream's clock.
    frame_number: Option<u64>,
    last: Option<Frame>,
    finished: bool,
}

impl std::fmt::Debug for Video {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Video")
            .field("info", &self.info)
            .field("next_index", &self.next_index)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

fn container(e: impl std::fmt::Display) -> Error {
    Error::Container(e.to_string())
}

fn decode_err(e: impl std::fmt::Display) -> Error {
    Error::Decode(e.to_string())
}

fn is_theora_header(data: &[u8]) -> bool {
    data.len() >= 7 && data[0] == 0x80 && &data[1..7] == b"theora"
}

fn is_vorbis_header(data: &[u8]) -> bool {
    data.len() >= 7 && data[0] == 0x01 && &data[1..7] == b"vorbis"
}

fn is_theora_comment_header(data: &[u8]) -> bool {
    data.len() >= 7 && data[0] == 0x81 && &data[1..7] == b"theora"
}

/// The comment header handed to the decoder in place of the stream's
/// own: a comment header packet with an empty vendor string and no user
/// comment (Theora specification, section 6.3.2).
///
/// The decoder validates the comment header and refuses the stream when
/// the vendor string or a comment is not UTF-8, or when a length runs
/// past the packet. Old encoders wrote Latin-1 text there, and nothing in
/// the comments is needed to decode, so the stream's own text is never
/// parsed. A packet that is not a comment header at all is passed on as
/// it is, for the decoder to report.
fn decoder_comment_header(packet: &[u8]) -> Vec<u8> {
    if !is_theora_comment_header(packet) {
        return packet.to_vec();
    }
    let mut out = Vec::with_capacity(15);
    out.push(0x81);
    out.extend_from_slice(b"theora");
    out.extend_from_slice(&0u32.to_le_bytes()); // vendor string length
    out.extend_from_slice(&0u32.to_le_bytes()); // number of user comments
    out
}

/// The frame number of a data packet on the stream's clock, from 0:
/// what times the frame, at `number * fps_den / fps_num` seconds.
///
/// `previous` is the number of the packet before, `page_granule` the
/// granule position of the page when the packet is the last one ending
/// on it (the only packet of a page whose position the container
/// states). Such a packet takes the frame number of that position; any
/// other packet comes one frame after the one before. This is Luma's
/// own rule: the original engine's decoder library (libtheora 1.0)
/// counts the packets and ignores the page positions. The two agree on
/// a regular stream; this one keeps a stream that leaves frames out
/// (its granule positions skip the missing frames, with no zero-length
/// packet in their place) on its real clock, where the original plays
/// it too fast. See "Frame times" in `docs/video.md`.
///
/// A position that marks no frame, one that would go back in time, or
/// one that jumps further ahead than [`MAX_FRAME_GAP`] (a damaged or
/// hostile file must not make the video wait for years) is ignored.
fn next_frame_number(
    ident: &TheoraIdentHeader,
    previous: Option<u64>,
    page_granule: Option<u64>,
) -> u64 {
    let following = previous.map_or(0, |n| n.saturating_add(1));
    let stated = page_granule
        // All ones: no packet ends on the page.
        .filter(|granule| *granule != u64::MAX)
        .and_then(|granule| ident.frame_index_from_granule_position(granule).ok());
    match stated {
        Some(number)
            if previous.is_none_or(|p| number >= p)
                && number <= following.saturating_add(MAX_FRAME_GAP) =>
        {
            number
        }
        _ => following,
    }
}

/// The most frames a stream may leave out between two pictures: two
/// minutes at 30 frames per second.
const MAX_FRAME_GAP: u64 = 3600;

/// The sound track of a video file as an Ogg file of its own: the
/// packets of the first Vorbis stream, on the same pages and with the
/// same granule positions, closed by an end-of-stream page. `None` when
/// the file has no Vorbis stream.
///
/// Audio decoders that read the whole container refuse a file whose
/// streams are not closed (a video cut short, or written by a muxer that
/// never marked the last pages), and would lose the whole track for it.
/// A damaged or truncated tail ends the track where it stops.
pub fn sound_track(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = PacketReader::new(Cursor::new(bytes));
    let mut writer = PacketWriter::new(Cursor::new(Vec::new()));
    let mut serial = None;
    let mut held: Option<ogg::Packet> = None;
    let mut write = |packet: ogg::Packet, last: bool| {
        let end = if last {
            PacketWriteEndInfo::EndStream
        } else if packet.last_in_page() {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        let (serial, granule) = (packet.stream_serial(), packet.absgp_page());
        writer.write_packet(packet.data, serial, end, granule).ok()
    };
    while let Ok(Some(packet)) = reader.read_packet() {
        match serial {
            None if is_vorbis_header(&packet.data) => serial = Some(packet.stream_serial()),
            Some(s) if s == packet.stream_serial() => {}
            _ => continue,
        }
        let ends = packet.last_in_stream();
        if let Some(previous) = held.replace(packet) {
            write(previous, false)?;
        }
        if ends {
            break;
        }
    }
    write(held?, true)?;
    Some(writer.into_inner().into_inner())
}

impl Video {
    /// Opens a video from the bytes of an Ogg file: reads the headers of
    /// its streams and gets ready to decode the first frame.
    pub fn open(bytes: Vec<u8>) -> Result<Video> {
        let bytes_len = bytes.len();
        let mut reader = PacketReader::new(Cursor::new(bytes));
        let mut serial = None;
        let mut headers: Vec<Vec<u8>> = Vec::new();
        let mut has_audio = false;
        while headers.len() < 3 {
            let packet = reader.read_packet().map_err(container)?.ok_or_else(|| {
                if serial.is_none() {
                    Error::NoVideoStream
                } else {
                    Error::Container("video headers end early".into())
                }
            })?;
            match serial {
                None => {
                    if is_theora_header(&packet.data) {
                        serial = Some(packet.stream_serial());
                        headers.push(packet.data);
                    } else if is_vorbis_header(&packet.data) {
                        has_audio = true;
                    }
                }
                Some(s) if packet.stream_serial() == s => headers.push(packet.data),
                Some(_) => {
                    if is_vorbis_header(&packet.data) {
                        has_audio = true;
                    }
                }
            }
        }
        let serial = serial.ok_or(Error::NoVideoStream)?;
        let headers: [Vec<u8>; 3] = [headers[0].clone(), headers[1].clone(), headers[2].clone()];
        let ident = decode_identification_header(&headers[0]).map_err(decode_err)?;
        let comment = decoder_comment_header(&headers[1]);
        let decoder = TheoraDecoder::with_headers(
            CodecId::new(THEORA_CODEC_ID),
            &[&headers[0], &comment, &headers[2]],
        )
        .map_err(decode_err)?;
        let info = VideoInfo::from_ident(&ident, has_audio);
        Ok(Video {
            reader,
            bytes_len,
            serial,
            headers,
            decoder,
            ident,
            info,
            next_index: 0,
            frame_number: None,
            last: None,
            finished: false,
        })
    }

    /// The picture size, frame rate and sound track.
    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    /// Index of the next frame to decode.
    pub fn next_index(&self) -> u64 {
        self.next_index
    }

    /// Whether every frame was decoded.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Size of the file, in bytes.
    pub fn len(&self) -> usize {
        self.bytes_len
    }

    /// Whether the file is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes_len == 0
    }

    /// Decodes the next frame; `None` once the stream ends. A zero-length
    /// packet repeats the previous frame, as the format specifies. The
    /// frame's time follows the granule positions of the stream's pages
    /// (see "Frame times" in `docs/video.md`).
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        if self.finished {
            return Ok(None);
        }
        loop {
            let packet = match self.reader.read_packet().map_err(container)? {
                Some(packet) => packet,
                None => {
                    self.finished = true;
                    return Ok(None);
                }
            };
            if packet.stream_serial() != self.serial {
                continue;
            }
            if packet.data.first().is_some_and(|b| b & 0x80 != 0) {
                // A header packet after the data (a chained stream): ignore.
                continue;
            }
            let index = self.next_index;
            self.next_index += 1;
            let granule = packet.last_in_page().then(|| packet.absgp_page());
            let number = next_frame_number(&self.ident, self.frame_number, granule);
            self.frame_number = Some(number);
            let time_ms = self.info.shown_at_ms(number);
            let keyframe = packet.data.first().is_some_and(|b| b & 0x40 == 0);
            let frame = if packet.data.is_empty() {
                match &self.last {
                    Some(last) => Frame {
                        index,
                        time_ms,
                        keyframe: false,
                        ..last.clone()
                    },
                    None => continue,
                }
            } else {
                self.decoder
                    .send_packet(&Packet::new(
                        0,
                        TimeBase::from_rate(self.info.fps_num.max(1)),
                        packet.data,
                    ))
                    .map_err(decode_err)?;
                let CoreFrame::Video(video) = self.decoder.receive_frame().map_err(decode_err)?
                else {
                    return Err(Error::Decode("the decoder gave no video frame".into()));
                };
                let (sub_x, sub_y) = match self.ident.pf {
                    PixelFormat::Yuv420 => (2, 2),
                    PixelFormat::Yuv422 => (2, 1),
                    _ => (1, 1),
                };
                let planes: Vec<(usize, &[u8])> = video
                    .planes
                    .iter()
                    .take(3)
                    .map(|p| (p.stride, p.data.as_slice()))
                    .collect();
                if planes.len() < 3 {
                    return Err(Error::Decode("fewer than three planes".into()));
                }
                let rgba = yuv_to_rgba(
                    self.info.width,
                    self.info.height,
                    planes[0],
                    planes[1],
                    planes[2],
                    sub_x,
                    sub_y,
                );
                Frame {
                    index,
                    time_ms,
                    keyframe,
                    width: self.info.width,
                    height: self.info.height,
                    rgba,
                }
            };
            self.last = Some(frame.clone());
            return Ok(Some(frame));
        }
    }

    /// Goes back to the first frame (looping).
    pub fn rewind(&mut self) -> Result<()> {
        let bytes = std::mem::take(self.reader.get_mut()).into_inner();
        let fresh = Video::open(bytes)?;
        *self = fresh;
        Ok(())
    }

    /// The three Theora header packets as the file has them (the comment
    /// header untouched), for callers that re-mux the stream.
    pub fn headers(&self) -> &[Vec<u8>; 3] {
        &self.headers
    }
}

/// The engine's fixed-point YUV to RGB tables (`GenLookupTables`): 13
/// bits of fraction, BT.601 studio range.
struct Tables {
    y: [i32; 256],
    rv: [i32; 256],
    gu: [i32; 256],
    gv: [i32; 256],
    bu: [i32; 256],
}

fn tables() -> &'static Tables {
    static TABLES: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let scale = f64::from(1i32 << 13);
        let mut t = Tables {
            y: [0; 256],
            rv: [0; 256],
            gu: [0; 256],
            gv: [0; 256],
            bu: [0; 256],
        };
        for i in 0..256usize {
            let c = i as i32 - 128;
            t.y[i] = ((1.164 * scale + 0.5) as i32) * (i as i32 - 16);
            t.rv[i] = ((1.596 * scale + 0.5) as i32) * c;
            t.gu[i] = ((0.391 * scale + 0.5) as i32) * c;
            t.gv[i] = ((0.813 * scale + 0.5) as i32) * c;
            t.bu[i] = ((2.018 * scale + 0.5) as i32) * c;
        }
        t
    })
}

/// Converts planar YUV (with chroma subsampled by `sub_x`, `sub_y`) to
/// opaque RGBA, as the engine's `RenderFrame` does.
pub fn yuv_to_rgba(
    width: u32,
    height: u32,
    y: (usize, &[u8]),
    u: (usize, &[u8]),
    v: (usize, &[u8]),
    sub_x: usize,
    sub_y: usize,
) -> Vec<u8> {
    let t = tables();
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![255u8; w * h * 4];
    let clip = |value: i32| (value >> 13).clamp(0, 255) as u8;
    for row in 0..h {
        let crow = row / sub_y.max(1);
        for col in 0..w {
            let ccol = col / sub_x.max(1);
            let yy = y.1.get(row * y.0 + col).copied().unwrap_or(16) as usize;
            let uu = u.1.get(crow * u.0 + ccol).copied().unwrap_or(128) as usize;
            let vv = v.1.get(crow * v.0 + ccol).copied().unwrap_or(128) as usize;
            let luma = t.y[yy];
            let o = (row * w + col) * 4;
            out[o] = clip(luma + t.rv[vv]);
            out[o + 1] = clip(luma - t.gu[uu] - t.gv[vv]);
            out[o + 2] = clip(luma + t.bu[uu]);
        }
    }
    out
}

/// A short Theora clip for tests and demos: `frames` frames of
/// `width` by `height` pixels (multiples of 16) at `fps` frames per
/// second, showing a bright square moving over a gradient, with a key
/// frame every eight frames.
pub fn synthetic_clip(width: u32, height: u32, frames: u32, fps: u32) -> Vec<u8> {
    let numbers: Vec<u64> = (0..u64::from(frames)).collect();
    synthetic_clip_at(width, height, fps, &numbers)
}

/// A clip like [`synthetic_clip`]'s whose frames carry the frame numbers
/// given, one picture per number: numbers that skip some make a stream
/// that leaves frames out, as encoders fed an irregular source write.
/// The numbers must not go back, nor run more than 63 past the last key
/// frame (every eighth picture).
pub fn synthetic_clip_at(width: u32, height: u32, fps: u32, numbers: &[u64]) -> Vec<u8> {
    let frames = numbers.len();
    let ident = TheoraIdentHeader {
        vmaj: 3,
        vmin: 2,
        vrev: 1,
        fmbw: (width / 16) as u16,
        fmbh: (height / 16) as u16,
        picw: width,
        pich: height,
        picx: 0,
        picy: 0,
        frn: fps,
        frd: 1,
        parn: 1,
        pard: 1,
        cs: ColorSpace::Undefined,
        nombr: 0,
        qual: 0,
        kfgshift: 6,
        pf: PixelFormat::Yuv420,
    };
    let mut encoder = TheoraEncoder::with_default_setup_keyframe_interval(
        CodecId::new(THEORA_CODEC_ID),
        ident.clone(),
        48,
        8,
    )
    .expect("a valid synthetic clip header");
    let (w, h) = (width as usize, height as usize);
    let mut packets = Vec::new();
    for i in 0..frames {
        let mut y = vec![0u8; w * h];
        for (row, line) in y.chunks_mut(w).enumerate() {
            for (col, px) in line.iter_mut().enumerate() {
                let dx = (col as i32 - (w as i32 / 4 + i as i32 * 2)).abs();
                let dy = (row as i32 - (h as i32 / 3 + i as i32)).abs();
                *px = if dx < 6 && dy < 6 {
                    235
                } else {
                    16 + ((col * 200 / w.max(1)) as u8) / 2
                };
            }
        }
        let (cw, ch) = (w / 2, h / 2);
        let cb: Vec<u8> = (0..cw * ch)
            .map(|k| (64 + (k % cw) * 2 + i) as u8)
            .collect();
        let cr: Vec<u8> = (0..cw * ch)
            .map(|k| (200u32.wrapping_sub(((k / cw) * 3 + i * 2) as u32) & 0xff) as u8)
            .collect();
        let frame = CoreFrame::Video(oxideav_core::VideoFrame {
            pts: Some(i as i64),
            planes: vec![
                oxideav_core::VideoPlane { stride: w, data: y },
                oxideav_core::VideoPlane {
                    stride: cw,
                    data: cb,
                },
                oxideav_core::VideoPlane {
                    stride: cw,
                    data: cr,
                },
            ],
        });
        encoder
            .send_frame(&frame)
            .expect("a frame the encoder accepts");
        while let Ok(packet) = encoder.receive_packet() {
            packets.push(packet);
        }
    }
    encoder.flush().expect("flush");
    while let Ok(packet) = encoder.receive_packet() {
        packets.push(packet);
    }
    let mut writer = PacketWriter::new(Cursor::new(Vec::new()));
    let serial = 0x4c75_6d61;
    let headers: Vec<&Packet> = packets.iter().filter(|p| p.flags.header).collect();
    let data: Vec<&Packet> = packets.iter().filter(|p| !p.flags.header).collect();
    for (i, packet) in headers.iter().enumerate() {
        let end = if i == 0 || i + 1 == headers.len() {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        writer
            .write_packet(packet.data.clone(), serial, end, 0)
            .expect("write to memory");
    }
    let mut last_key = 0u64;
    for (i, packet) in data.iter().enumerate() {
        let number = numbers.get(i).copied().unwrap_or(i as u64);
        if packet.flags.keyframe {
            last_key = number;
        }
        let granule = ident
            .granule_position_for_frame(number, last_key)
            .expect("a representable granule position");
        let end = if i + 1 == data.len() {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::EndPage
        };
        writer
            .write_packet(packet.data.clone(), serial, end, granule)
            .expect("write to memory");
    }
    writer.into_inner().into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_synthetic_clip() {
        let bytes = synthetic_clip(48, 32, 12, 25);
        let mut video = Video::open(bytes).unwrap();
        let info = video.info().clone();
        assert_eq!((info.width, info.height), (48, 32));
        assert_eq!((info.fps_num, info.fps_den), (25, 1));
        assert!(!info.has_audio);
        assert_eq!(info.frame_time_ms(5), 200);
        assert_eq!(info.frame_at_ms(200), 5);
        let mut count = 0;
        let mut keyframes = 0;
        while let Some(frame) = video.next_frame().unwrap() {
            assert_eq!(frame.index, count);
            assert_eq!(frame.time_ms, info.frame_time_ms(count));
            assert_eq!(frame.rgba.len(), 48 * 32 * 4);
            assert!(frame.rgba.chunks(4).all(|p| p[3] == 255));
            // The square is bright white; the gradient darker.
            let at = |x: usize, y: usize| &frame.rgba[(y * 48 + x) * 4..(y * 48 + x) * 4 + 3];
            let square = at(12 + count as usize * 2, 10 + count as usize);
            let corner = at(47, 31);
            assert!(
                square[0] > 200 && square[1] > 150 && square[1] > corner[1] + 40,
                "{square:?} vs {corner:?}"
            );
            keyframes += usize::from(frame.keyframe);
            count += 1;
        }
        assert_eq!(count, 12);
        assert_eq!(keyframes, 2);
        assert!(video.is_finished());
        assert!(video.next_frame().unwrap().is_none());
        video.rewind().unwrap();
        assert_eq!(video.next_index(), 0);
        assert!(video.next_frame().unwrap().is_some());
    }

    /// Rewrites an Ogg file packet by packet, on the same pages and with
    /// the same granule positions; `edit` gets each packet's number in
    /// the file and its bytes.
    fn remux(bytes: &[u8], mut edit: impl FnMut(usize, Vec<u8>) -> Vec<u8>) -> Vec<u8> {
        let mut reader = PacketReader::new(Cursor::new(bytes.to_vec()));
        let mut writer = PacketWriter::new(Cursor::new(Vec::new()));
        let mut number = 0;
        while let Some(packet) = reader.read_packet().unwrap() {
            let end = if packet.last_in_stream() {
                PacketWriteEndInfo::EndStream
            } else if packet.last_in_page() {
                PacketWriteEndInfo::EndPage
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            let (serial, granule) = (packet.stream_serial(), packet.absgp_page());
            writer
                .write_packet(edit(number, packet.data), serial, end, granule)
                .unwrap();
            number += 1;
        }
        writer.into_inner().into_inner()
    }

    /// A packet of an Ogg file and how it sits in it.
    #[derive(Clone)]
    struct Piece {
        serial: u32,
        data: Vec<u8>,
        granule: u64,
        end: PacketWriteEndInfo,
    }

    /// The packets of an Ogg file, in file order.
    fn pieces(bytes: &[u8]) -> Vec<Piece> {
        let mut reader = PacketReader::new(Cursor::new(bytes));
        let mut out = Vec::new();
        while let Some(packet) = reader.read_packet().unwrap() {
            let end = if packet.last_in_stream() {
                PacketWriteEndInfo::EndStream
            } else if packet.last_in_page() {
                PacketWriteEndInfo::EndPage
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            out.push(Piece {
                serial: packet.stream_serial(),
                granule: packet.absgp_page(),
                data: packet.data,
                end,
            });
        }
        out
    }

    /// An Ogg file of these packets.
    fn mux(pieces: &[Piece]) -> Vec<u8> {
        let mut writer = PacketWriter::new(Cursor::new(Vec::new()));
        for piece in pieces {
            writer
                .write_packet(piece.data.clone(), piece.serial, piece.end, piece.granule)
                .unwrap();
        }
        writer.into_inner().into_inner()
    }

    #[test]
    fn numbers_frames_by_the_granule_positions() {
        let clip = pieces(&synthetic_clip(32, 32, 1, 10));
        let ident = decode_identification_header(&clip[0].data).unwrap();
        let at = |frame: u64, key: u64| ident.granule_position_for_frame(frame, key).unwrap();
        // Without a position, a packet follows the one before.
        assert_eq!(next_frame_number(&ident, None, None), 0);
        assert_eq!(next_frame_number(&ident, Some(4), None), 5);
        // The position of the page says which frame its last packet is.
        assert_eq!(next_frame_number(&ident, None, Some(at(0, 0))), 0);
        assert_eq!(next_frame_number(&ident, Some(4), Some(at(5, 0))), 5);
        assert_eq!(next_frame_number(&ident, Some(4), Some(at(9, 0))), 9);
        assert_eq!(next_frame_number(&ident, Some(70), Some(at(75, 64))), 75);
        assert_eq!(next_frame_number(&ident, None, Some(at(30, 0))), 30);
        // Two packets may claim the same frame; none goes back.
        assert_eq!(next_frame_number(&ident, Some(9), Some(at(9, 0))), 9);
        assert_eq!(next_frame_number(&ident, Some(9), Some(at(3, 0))), 10);
        // Positions that mark no frame: no packet ends on the page, or a
        // header's zero.
        assert_eq!(next_frame_number(&ident, Some(4), Some(u64::MAX)), 5);
        assert_eq!(next_frame_number(&ident, Some(4), Some(0)), 5);
        assert_eq!(next_frame_number(&ident, Some(u64::MAX), None), u64::MAX);
        // A jump of up to two minutes of frames is a gap; a longer one is
        // a damaged position, and the packet follows the one before.
        assert_eq!(
            next_frame_number(&ident, Some(4), Some(at(3604, 3600))),
            3604
        );
        assert_eq!(next_frame_number(&ident, Some(4), Some(at(9000, 8960))), 5);
        assert_eq!(next_frame_number(&ident, None, Some(at(9000, 8960))), 0);
        // Before bitstream 3.2.1 the position held the frame's index, not
        // the count of frames.
        let legacy = TheoraIdentHeader {
            vrev: 0,
            ..ident.clone()
        };
        let shift = ident.kfgshift;
        assert_eq!(next_frame_number(&ident, None, Some(4 << shift)), 3);
        assert_eq!(next_frame_number(&legacy, None, Some(4 << shift)), 4);
        assert_eq!(next_frame_number(&legacy, Some(0), Some(0)), 0);
        // Times do not overflow on absurd numbers.
        let film = TheoraIdentHeader {
            frn: 30000,
            frd: 1001,
            ..ident.clone()
        };
        let info = VideoInfo::from_ident(&film, false);
        assert_eq!(info.frame_time_ms(115), 3837);
        assert_eq!(info.frame_time_ms(u64::MAX), u64::MAX);
        // A frame is shown at the start of its interval; before
        // bitstream 3.2.1, the original's decoder gives its end.
        assert_eq!(info.shown_at_ms(115), 3837);
        let old = VideoInfo::from_ident(
            &TheoraIdentHeader {
                vrev: 0,
                ..film.clone()
            },
            false,
        );
        assert!(old.before_3_2_1);
        assert_eq!(old.shown_at_ms(114), 3837);
        assert_eq!(old.shown_at_ms(u64::MAX), u64::MAX);
        // The coded frame holds the picture, whose offset the stream
        // gives from the bottom-left corner.
        let cropped = VideoInfo::from_ident(
            &TheoraIdentHeader {
                fmbw: 40,
                fmbh: 23,
                picw: 640,
                pich: 360,
                picx: 0,
                picy: 2,
                ..ident.clone()
            },
            true,
        );
        assert_eq!((cropped.width, cropped.height), (640, 360));
        assert_eq!((cropped.coded_width, cropped.coded_height), (640, 368));
        assert_eq!((cropped.pic_x, cropped.pic_y), (0, 6));
        assert!(!cropped.before_3_2_1 && cropped.has_audio);
    }

    #[test]
    fn times_frames_of_a_stream_that_leaves_frames_out() {
        let clean = synthetic_clip(32, 32, 8, 10);
        let mut clip = pieces(&clean);
        assert_eq!(clip.len(), 3 + 8);
        let ident = decode_identification_header(&clip[0].data).unwrap();
        // Frames 1, 3, 4, 8, 10 and 11 are missing: the positions skip
        // them. The fifth to seventh packets share a page, whose position
        // is the seventh's; the sixth is a zero-length packet.
        let numbers = [0, 2, 2, 5, 0, 0, 9, 12];
        for (piece, number) in clip[3..].iter_mut().zip(numbers) {
            piece.granule = ident.granule_position_for_frame(number, 0).unwrap();
        }
        clip[3 + 4].end = PacketWriteEndInfo::NormalPacket;
        clip[3 + 5].end = PacketWriteEndInfo::NormalPacket;
        clip[3 + 5].data.clear();
        let granule = clip[3 + 6].granule;
        clip[3 + 4].granule = granule;
        clip[3 + 5].granule = granule;
        // The stream is not closed either.
        clip[3 + 7].end = PacketWriteEndInfo::EndPage;
        let mut video = Video::open(mux(&clip)).unwrap();
        let mut reference = Video::open(clean).unwrap();
        let mut frames = Vec::new();
        while let Some(frame) = video.next_frame().unwrap() {
            frames.push(frame);
        }
        let times: Vec<u64> = frames.iter().map(|f| f.time_ms).collect();
        assert_eq!(times, [0, 200, 200, 500, 600, 700, 900, 1200]);
        let indices: Vec<u64> = frames.iter().map(|f| f.index).collect();
        assert_eq!(indices, [0, 1, 2, 3, 4, 5, 6, 7]);
        // The pictures are the stream's, in order; the zero-length
        // packet repeats the one before it.
        for frame in &frames[..5] {
            let expected = reference.next_frame().unwrap().unwrap();
            assert_eq!(frame.rgba, expected.rgba);
            assert_eq!(expected.time_ms, expected.index * 100);
        }
        assert_eq!(frames[5].rgba, frames[4].rgba);
        assert!(!frames[5].keyframe);
        // Rewinding starts the clock again.
        video.rewind().unwrap();
        assert_eq!(video.next_frame().unwrap().unwrap().time_ms, 0);
        assert_eq!(video.next_frame().unwrap().unwrap().time_ms, 200);
    }

    /// A video file whose first stream is a (made up) Vorbis stream of
    /// `sound` data packets, three to a page, its pages between the
    /// video's; no stream is closed.
    fn clip_with_sound_first(video: &[u8], sound: usize) -> (Vec<u8>, Vec<Piece>) {
        let serial = 0x536f_756e;
        let page = |data: Vec<u8>, granule: u64, end| Piece {
            serial,
            data,
            granule,
            end,
        };
        let mut audio = vec![
            page(b"\x01vorbis-ident".to_vec(), 0, PacketWriteEndInfo::EndPage),
            page(
                b"\x03vorbis-comment".to_vec(),
                0,
                PacketWriteEndInfo::NormalPacket,
            ),
            page(b"\x05vorbis-setup".to_vec(), 0, PacketWriteEndInfo::EndPage),
        ];
        for i in 0..sound {
            let last_of_page = i % 3 == 2 || i + 1 == sound;
            audio.push(page(
                vec![0, i as u8],
                ((i / 3 + 1) * 3 * 1024).min(sound * 1024) as u64,
                if last_of_page {
                    PacketWriteEndInfo::EndPage
                } else {
                    PacketWriteEndInfo::NormalPacket
                },
            ));
        }
        let mut picture = pieces(video);
        if let Some(last) = picture.last_mut() {
            last.end = PacketWriteEndInfo::EndPage;
        }
        // The first pages of both streams, the sound's first; then the
        // other headers; then the data, three sound packets a frame.
        let mut all = vec![audio[0].clone(), picture[0].clone()];
        all.extend_from_slice(&audio[1..3]);
        all.extend_from_slice(&picture[1..3]);
        let mut sound_pages = audio[3..].chunks(3);
        for frame in &picture[3..] {
            all.extend_from_slice(sound_pages.next().unwrap_or_default());
            all.push(frame.clone());
        }
        for rest in sound_pages {
            all.extend_from_slice(rest);
        }
        (mux(&all), audio)
    }

    #[test]
    fn plays_a_file_whose_first_stream_is_the_sound() {
        let clean = synthetic_clip(32, 32, 5, 10);
        let (bytes, _) = clip_with_sound_first(&clean, 20);
        let mut video = Video::open(bytes).unwrap();
        assert!(video.info().has_audio);
        let mut reference = Video::open(clean).unwrap();
        assert!(!reference.info().has_audio);
        for _ in 0..5 {
            assert_eq!(video.next_frame().unwrap(), reference.next_frame().unwrap());
        }
        assert!(video.next_frame().unwrap().is_none());
        video.rewind().unwrap();
        assert!(video.info().has_audio);
        assert_eq!(video.next_frame().unwrap().unwrap().index, 0);
    }

    #[test]
    fn extracts_the_sound_track_and_closes_it() {
        let clean = synthetic_clip(32, 32, 5, 10);
        let (bytes, audio) = clip_with_sound_first(&clean, 20);
        // Nothing closes the streams of the file.
        assert!(
            pieces(&bytes)
                .iter()
                .all(|p| p.end != PacketWriteEndInfo::EndStream)
        );
        let track = pieces(&sound_track(&bytes).unwrap());
        assert_eq!(track.len(), audio.len());
        for (i, (got, expected)) in track.iter().zip(&audio).enumerate() {
            assert_eq!(got.serial, expected.serial);
            assert_eq!(got.data, expected.data);
            assert_eq!(got.granule, expected.granule, "packet {i}");
            if i + 1 == track.len() {
                assert!(got.end == PacketWriteEndInfo::EndStream);
            } else {
                assert!(got.end == expected.end, "packet {i}");
            }
        }
        // A file cut in the middle of a page keeps the sound before the
        // cut, closed too.
        let cut = sound_track(&bytes[..bytes.len() - 5]).unwrap();
        let cut = pieces(&cut);
        assert_eq!(cut.len(), audio.len() - 2, "the last page is lost");
        assert!(cut.last().unwrap().end == PacketWriteEndInfo::EndStream);
        assert!(
            cut.iter()
                .zip(&audio)
                .all(|(got, expected)| got.data == expected.data)
        );
        // A stream already closed stays as it is.
        let again = pieces(&sound_track(&mux(&track)).unwrap());
        assert_eq!(again.len(), track.len());
        // No sound: no track.
        assert!(sound_track(&clean).is_none());
        assert!(sound_track(b"").is_none());
        assert!(sound_track(b"OggS, or so it says").is_none());
    }

    /// A Theora comment header with a vendor string and user comments.
    fn comment_header(vendor: &[u8], comments: &[&[u8]]) -> Vec<u8> {
        let mut out = b"\x81theora".to_vec();
        out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        out.extend_from_slice(vendor);
        out.extend_from_slice(&(comments.len() as u32).to_le_bytes());
        for comment in comments {
            out.extend_from_slice(&(comment.len() as u32).to_le_bytes());
            out.extend_from_slice(comment);
        }
        out
    }

    #[test]
    fn gives_the_decoder_a_bare_comment_header() {
        use oxideav_theora::parse_comment_header;
        let bare = comment_header(b"", &[]);
        // Latin-1 text, in the vendor string or in a comment: what the
        // decoder refuses.
        let latin1_vendor = comment_header(b"Encoder \xa9 2005", &[b"TITLE=Intro"]);
        let latin1_comment = comment_header(b"Encoder", &[b"ENCODER=Tool \xa9 2005"]);
        assert!(parse_comment_header(&latin1_vendor).is_err());
        assert!(parse_comment_header(&latin1_comment).is_err());
        let mut cases = vec![latin1_vendor.clone(), latin1_comment.clone()];
        // Truncated anywhere after the signature.
        for len in 7..latin1_comment.len() {
            cases.push(latin1_comment[..len].to_vec());
        }
        // Lengths far beyond the packet.
        let mut huge_vendor = b"\x81theora".to_vec();
        huge_vendor.extend_from_slice(&u32::MAX.to_le_bytes());
        huge_vendor.extend_from_slice(b"short");
        cases.push(huge_vendor);
        let mut huge_count = comment_header(b"Encoder", &[]);
        let at = huge_count.len() - 4;
        huge_count[at..].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(huge_count);
        let mut huge_comment = comment_header(b"Encoder", &[b"A=B"]);
        let at = huge_comment.len() - 7;
        huge_comment[at..at + 4].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
        cases.push(huge_comment);
        for case in cases {
            let out = decoder_comment_header(&case);
            assert_eq!(out, bare, "{case:02x?}");
            assert!(parse_comment_header(&out).is_ok());
        }
        // Anything else is left for the decoder to refuse.
        for other in [&b""[..], b"\x81theor", b"\x82theora\0\0", b"\x03vorbis"] {
            assert_eq!(decoder_comment_header(other), other);
        }
    }

    #[test]
    fn opens_a_stream_whose_comments_are_not_utf8() {
        let clean = synthetic_clip(32, 32, 4, 10);
        let latin1 = comment_header(b"Encoder \xa9 2005", &[b"ENCODER=Tool \xa9 2005"]);
        let truncated = latin1[..latin1.len() - 3].to_vec();
        for header in [latin1, truncated] {
            let bytes = remux(&clean, |number, data| {
                if number == 1 {
                    assert!(is_theora_comment_header(&data));
                    header.clone()
                } else {
                    data
                }
            });
            let mut video = Video::open(bytes).unwrap();
            // The stream's own header is kept for callers.
            assert_eq!(video.headers()[1], header);
            let mut reference = Video::open(clean.clone()).unwrap();
            let mut count = 0;
            while let Some(frame) = video.next_frame().unwrap() {
                assert_eq!(Some(frame), reference.next_frame().unwrap());
                count += 1;
            }
            assert_eq!(count, 4);
            video.rewind().unwrap();
            assert!(video.next_frame().unwrap().is_some());
        }
    }

    #[test]
    fn converts_yuv_with_the_engine_tables() {
        // Studio black and white, and a pure red.
        let px = |y: u8, u: u8, v: u8| {
            let out = yuv_to_rgba(1, 1, (1, &[y]), (1, &[u]), (1, &[v]), 1, 1);
            [out[0], out[1], out[2]]
        };
        assert_eq!(px(16, 128, 128), [0, 0, 0]);
        // The engine's rounding lands studio white one step short.
        assert!(px(235, 128, 128).iter().all(|c| *c >= 254));
        let red = px(81, 90, 240);
        assert!(red[0] > 240 && red[1] < 20 && red[2] < 20, "{red:?}");
        // Chroma is shared by the 2x2 blocks of a 4:2:0 frame.
        let out = yuv_to_rgba(
            2,
            2,
            (2, &[235, 16, 16, 235]),
            (1, &[128]),
            (1, &[128]),
            2,
            2,
        );
        assert!(out[..3].iter().all(|c| *c >= 254) && out[3] == 255);
        assert_eq!(&out[4..8], &[0, 0, 0, 255]);
    }

    #[test]
    fn rejects_files_without_video() {
        assert!(matches!(
            Video::open(Vec::new()).err(),
            Some(Error::Container(_) | Error::NoVideoStream)
        ));
        assert!(matches!(
            Video::open(b"not an ogg file at all, just text".to_vec()).err(),
            Some(Error::Container(_) | Error::NoVideoStream)
        ));
    }
}
