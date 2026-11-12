//! AVI files written in code, for tests and demos: no video file is
//! needed in the repository.

/// The sound of an AVI file to write.
#[derive(Debug, Clone, Default)]
pub struct AviSound {
    /// `wFormatTag`: 1 is PCM.
    pub tag: u16,
    /// Channels.
    pub channels: u16,
    /// Samples per second.
    pub sample_rate: u32,
    /// `nBlockAlign`.
    pub block_align: u16,
    /// Bits per sample.
    pub bits: u16,
    /// What follows the fixed fields of the format.
    pub extra: Vec<u8>,
    /// The data, one chunk per item, spread among the frames.
    pub chunks: Vec<Vec<u8>>,
}

/// An AVI file to write: one video stream and, optionally, one audio
/// stream.
#[derive(Debug, Clone, Default)]
pub struct AviSpec {
    /// Picture width.
    pub width: u32,
    /// Picture height; negative for rows from the top.
    pub height: i32,
    /// `biCompression`, also written as the stream's handler.
    pub compression: u32,
    /// Bits per pixel.
    pub bit_count: u16,
    /// The colour table: blue, green, red, zero.
    pub palette: Vec<[u8; 4]>,
    /// With `scale`: `rate / scale` frames per second.
    pub rate: u32,
    /// See `rate`.
    pub scale: u32,
    /// `dwStart` of the video stream.
    pub start: u32,
    /// The frames: the data of each chunk.
    pub frames: Vec<Vec<u8>>,
    /// Palette changes (`##pc` chunks): the frame each comes before
    /// and its data.
    pub palette_changes: Vec<(usize, Vec<u8>)>,
    /// The frames the index does not flag as key frames.
    pub delta_frames: Vec<usize>,
    /// The sound.
    pub sound: Option<AviSound>,
    /// Whether the audio stream comes first (stream 0).
    pub sound_first: bool,
    /// Whether an `idx1` index is written.
    pub index: bool,
    /// Whether the frames are grouped in `rec ` lists.
    pub records: bool,
}

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn list(kind: &[u8; 4], parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = kind.to_vec();
    for part in parts {
        body.extend_from_slice(part);
    }
    chunk(b"LIST", &body)
}

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Writes an AVI file.
pub fn build_avi(spec: &AviSpec) -> Vec<u8> {
    let frames = spec.frames.len() as u32;
    let (video_number, audio_number) = if spec.sound_first && spec.sound.is_some() {
        (1u8, 0u8)
    } else {
        (0, 1)
    };
    let id = |number: u8, kind: &[u8; 2]| [b'0', b'0' + number, kind[0], kind[1]];

    let micro_sec = if spec.rate == 0 {
        0
    } else {
        (1_000_000u64 * u64::from(spec.scale) / u64::from(spec.rate)) as u32
    };
    let avih = chunk(
        b"avih",
        &words(&[
            micro_sec,
            0,
            0,
            0x10,
            frames,
            0,
            1 + u32::from(spec.sound.is_some()),
            0,
            spec.width,
            spec.height.unsigned_abs(),
            0,
            0,
            0,
            0,
        ]),
    );

    let mut video_header = b"vids".to_vec();
    video_header.extend_from_slice(&words(&[
        spec.compression,
        0,
        0,
        0,
        spec.scale,
        spec.rate,
        spec.start,
        frames,
        0,
        0,
        0,
        0,
        0,
    ]));
    let mut video_format = words(&[40, spec.width, spec.height as u32]);
    video_format.extend_from_slice(&1u16.to_le_bytes());
    video_format.extend_from_slice(&spec.bit_count.to_le_bytes());
    video_format.extend_from_slice(&words(&[
        spec.compression,
        0,
        0,
        0,
        spec.palette.len() as u32,
        0,
    ]));
    for entry in &spec.palette {
        video_format.extend_from_slice(entry);
    }
    let video_list = list(
        b"strl",
        &[chunk(b"strh", &video_header), chunk(b"strf", &video_format)],
    );

    let audio_list = spec.sound.as_ref().map(|sound| {
        let bytes: usize = sound.chunks.iter().map(Vec::len).sum();
        let mut header = b"auds".to_vec();
        header.extend_from_slice(&words(&[
            0,
            0,
            0,
            0,
            u32::from(sound.block_align.max(1)),
            sound.sample_rate * u32::from(sound.block_align.max(1)),
            0,
            (bytes / usize::from(sound.block_align.max(1))) as u32,
            0,
            0,
            u32::from(sound.block_align),
            0,
            0,
        ]));
        let mut format = sound.tag.to_le_bytes().to_vec();
        format.extend_from_slice(&sound.channels.to_le_bytes());
        format.extend_from_slice(&sound.sample_rate.to_le_bytes());
        format.extend_from_slice(&(sound.sample_rate * u32::from(sound.block_align)).to_le_bytes());
        format.extend_from_slice(&sound.block_align.to_le_bytes());
        format.extend_from_slice(&sound.bits.to_le_bytes());
        if sound.tag != 1 || !sound.extra.is_empty() {
            format.extend_from_slice(&(sound.extra.len() as u16).to_le_bytes());
            format.extend_from_slice(&sound.extra);
        }
        list(b"strl", &[chunk(b"strh", &header), chunk(b"strf", &format)])
    });

    let mut header_parts = vec![avih];
    match audio_list {
        Some(audio) if spec.sound_first => {
            header_parts.push(audio);
            header_parts.push(video_list);
        }
        Some(audio) => {
            header_parts.push(video_list);
            header_parts.push(audio);
        }
        None => header_parts.push(video_list),
    }
    let headers = list(b"hdrl", &header_parts);

    // The chunks of `movi`, the sound spread among the frames, and the
    // index: offsets from the `movi` FourCC.
    let mut movie = b"movi".to_vec();
    let mut index = Vec::new();
    let sound_chunks: &[Vec<u8>] = spec.sound.as_ref().map_or(&[], |s| &s.chunks);
    let mut sound_at = 0;
    let steps = spec.frames.len().max(1);
    for n in 0..steps {
        let mut group: Vec<(Vec<u8>, [u8; 4], u32)> = Vec::new();
        for (_, data) in spec.palette_changes.iter().filter(|(at, _)| *at == n) {
            group.push((data.clone(), id(video_number, b"pc"), 0));
        }
        if let Some(frame) = spec.frames.get(n) {
            let kind = if spec.compression == 0 { b"db" } else { b"dc" };
            let flags = if spec.delta_frames.contains(&n) {
                0
            } else {
                0x10
            };
            group.push((frame.clone(), id(video_number, kind), flags));
        }
        // Each frame takes its share of the sound chunks.
        let sound_end = sound_chunks.len() * (n + 1) / steps;
        for data in &sound_chunks[sound_at..sound_end] {
            group.push((data.clone(), id(audio_number, b"wb"), 0x10));
        }
        sound_at = sound_end;
        let mut record = b"rec ".to_vec();
        let base = if spec.records {
            // After the list's own header.
            movie.len() + 8
        } else {
            movie.len()
        };
        let target = if spec.records {
            &mut record
        } else {
            &mut movie
        };
        let before = target.len();
        for (data, id, flags) in &group {
            let offset = base + target.len() - before + if spec.records { 4 } else { 0 };
            index.extend_from_slice(id);
            index.extend_from_slice(&words(&[*flags, offset as u32, data.len() as u32]));
            target.extend_from_slice(&chunk(id, data));
        }
        if spec.records {
            movie.extend_from_slice(&chunk(b"LIST", &record));
        }
    }

    let mut body = b"AVI ".to_vec();
    body.extend_from_slice(&headers);
    body.extend_from_slice(&chunk(b"JUNK", &[0; 5]));
    body.extend_from_slice(&chunk(b"LIST", &movie));
    if spec.index {
        body.extend_from_slice(&chunk(b"idx1", &index));
    }
    chunk(b"RIFF", &body)
}

/// A short AVI clip for tests and demos: `frames` uncompressed 24-bit
/// frames of `width` by `height` pixels at `fps` frames per second,
/// showing a bright square moving over a gradient (frame `n` has its
/// top-left pixel's red at `n * 10`), with `sound_ms` milliseconds of
/// silent 16-bit mono PCM at 8000 Hz when that is not 0.
pub fn synthetic_avi(width: u32, height: u32, frames: u32, fps: u32, sound_ms: u32) -> Vec<u8> {
    let stride = (width as usize * 3).div_ceil(4) * 4;
    let pictures = (0..frames)
        .map(|n| {
            let mut data = vec![0; stride * height as usize];
            for y in 0..height {
                // Rows from the bottom.
                let row = &mut data[(height - 1 - y) as usize * stride..];
                for x in 0..width {
                    let square = x.abs_diff(n * 2 % width.max(1)) < 4 && y.abs_diff(height / 2) < 4;
                    let (r, g, b) = match (x, y, square) {
                        (0, 0, _) => ((n * 10) as u8, 0, 0),
                        (_, _, true) => (255, 255, 255),
                        _ => ((x * 255 / width) as u8, (y * 255 / height) as u8, 64),
                    };
                    row[x as usize * 3..x as usize * 3 + 3].copy_from_slice(&[b, g, r]);
                }
            }
            data
        })
        .collect();
    let sound = (sound_ms != 0).then(|| {
        let bytes = sound_ms as usize * 8 * 2;
        AviSound {
            tag: 1,
            channels: 1,
            sample_rate: 8000,
            block_align: 2,
            bits: 16,
            extra: Vec::new(),
            chunks: vec![0u8; bytes].chunks(1600).map(<[u8]>::to_vec).collect(),
        }
    });
    build_avi(&AviSpec {
        width,
        height: height as i32,
        compression: 0,
        bit_count: 24,
        rate: fps,
        scale: 1,
        frames: pictures,
        sound,
        index: true,
        ..AviSpec::default()
    })
}
