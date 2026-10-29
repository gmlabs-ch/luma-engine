//! Video playback: Ogg Theora files decoded to RGBA frames, one at a
//! time, with the timing a player needs.
//!
//! Part of [Luma Engine](https://github.com/gmlabs-ch/luma-engine).
//!
//! The Ogg container is read with the `ogg` crate and the Theora stream
//! decoded with `oxideav-theora`, both pure Rust. The sound track (Vorbis)
//! is not decoded here: `luma_audio::decode` reads it from the same
//! bytes. The YUV to RGB conversion uses the fixed-point tables of the
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
    /// Frame rate numerator.
    pub fps_num: u32,
    /// Frame rate denominator.
    pub fps_den: u32,
    /// Whether the file also carries a Vorbis sound track.
    pub has_audio: bool,
}

impl VideoInfo {
    /// When a frame is shown, in milliseconds from the start.
    pub fn frame_time_ms(&self, index: u64) -> u64 {
        if self.fps_num == 0 {
            return 0;
        }
        index * 1000 * u64::from(self.fps_den) / u64::from(self.fps_num)
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
    /// Index of the frame in the stream, from 0.
    pub index: u64,
    /// When the frame is shown, in milliseconds from the start.
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
        let decoder = TheoraDecoder::with_headers(
            CodecId::new(THEORA_CODEC_ID),
            &[&headers[0], &headers[1], &headers[2]],
        )
        .map_err(decode_err)?;
        let info = VideoInfo {
            width: ident.picw,
            height: ident.pich,
            fps_num: ident.frn,
            fps_den: ident.frd,
            has_audio,
        };
        Ok(Video {
            reader,
            bytes_len,
            serial,
            headers,
            decoder,
            ident,
            info,
            next_index: 0,
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
    /// packet repeats the previous frame, as the format specifies.
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
            let keyframe = packet.data.first().is_some_and(|b| b & 0x40 == 0);
            let frame = if packet.data.is_empty() {
                match &self.last {
                    Some(last) => Frame {
                        index,
                        time_ms: self.info.frame_time_ms(index),
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
                    time_ms: self.info.frame_time_ms(index),
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

    /// The three Theora header packets, for callers that re-mux the
    /// stream.
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
    for i in 0..frames as usize {
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
        if packet.flags.keyframe {
            last_key = i as u64;
        }
        let granule = ident
            .granule_position_for_frame(i as u64, last_key)
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
