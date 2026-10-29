//! Video playback in the game (`Game.PlayTheora`): timing, skipping,
//! subtitles and drawing of a video decoded by `luma-video`.
//!
//! Written with reference to `VidTheoraPlayer.cpp`, `VidSubtitler.cpp`
//! and `BGame.cpp` of Wintermute Engine 1.x (MIT License, Copyright (c)
//! 2003-2013 Jan Nedoma); see `NOTICE`. The rules are documented in
//! `docs/video.md`.

use std::sync::Arc;

use luma_audio::{SoundId, SoundType};
use luma_formats::font::TextAlign;
use luma_render::{DrawList, Quad, Rect, Texture};
use luma_script::Engine;
use luma_video::{Frame, Video, VideoInfo};

use crate::world::World;

/// How a video is placed on the screen (`TVideoPlayback`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoPlayback {
    /// At a position, at 100 % (`VID_PLAY_POS`, 0).
    Pos,
    /// Scaled to fit the screen, keeping its aspect, centred
    /// (`VID_PLAY_STRETCH`, 1).
    #[default]
    Stretch,
    /// Centred at 100 % (`VID_PLAY_CENTER`, 2).
    Center,
}

impl VideoPlayback {
    /// From the script's code; anything else stretches.
    pub fn from_code(code: i32) -> VideoPlayback {
        match code {
            0 => VideoPlayback::Pos,
            2 => VideoPlayback::Center,
            _ => VideoPlayback::Stretch,
        }
    }
}

/// One subtitle of a `.sub` file: a text shown from a frame to a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtitle {
    /// The text, with `|` turned into line breaks.
    pub text: String,
    /// First frame.
    pub start: i64,
    /// Last frame; `0` keeps the subtitle until the next one.
    pub end: i64,
}

/// Parses a `.sub` file (`CVidSubtitler::LoadSubtitles`): each line is
/// `{start}{end}text`; lines without a start, with no text, or with
/// `{1}{1}` are dropped.
pub fn parse_subtitles(bytes: &[u8]) -> Vec<Subtitle> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for line in text.split(['\n', '\0']) {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let (mut start, mut end) = (None, None);
        let mut in_token = false;
        let mut token = String::new();
        let mut token_index: i32 = -1;
        let mut body = String::new();
        for ch in line.chars() {
            match ch {
                '{' => {
                    if in_token {
                        token.push(ch);
                    } else {
                        in_token = true;
                        token.clear();
                        token_index += 1;
                    }
                }
                '}' => {
                    if in_token {
                        in_token = false;
                        let value = token.trim().parse::<i64>().unwrap_or(0);
                        match token_index {
                            0 => start = Some(value),
                            1 => end = Some(value),
                            _ => {}
                        }
                    } else {
                        body.push(ch);
                    }
                }
                _ if in_token => token.push(ch),
                '|' => body.push('\n'),
                _ => body.push(ch),
            }
        }
        if let Some(start) = start {
            let end = end.unwrap_or(-1);
            if !body.is_empty() && !(start == 1 && end == 1) {
                out.push(Subtitle {
                    text: body,
                    start,
                    end,
                });
            }
        }
    }
    out
}

/// The subtitles of a video and the one shown (`CVidSubtitler`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtitler {
    /// The subtitles, in file order.
    pub subtitles: Vec<Subtitle>,
    current: usize,
    show: bool,
    last_frame: i64,
}

impl Subtitler {
    /// A subtitler over parsed subtitles.
    pub fn new(subtitles: Vec<Subtitle>) -> Subtitler {
        Subtitler {
            subtitles,
            current: 0,
            show: false,
            last_frame: -1,
        }
    }

    /// Moves to the subtitle of a frame (`CVidSubtitler::Update`).
    pub fn update(&mut self, frame: i64) {
        if frame == self.last_frame {
            return;
        }
        self.last_frame = frame;
        self.show = false;
        while self.current < self.subtitles.len() {
            let end = self.subtitles[self.current].end;
            let next_ok = self.current + 1 < self.subtitles.len()
                && self.subtitles[self.current + 1].start <= frame;
            if frame > end {
                if next_ok {
                    self.current += 1;
                } else {
                    self.show = end == 0;
                    break;
                }
            } else {
                self.show = true;
                break;
            }
        }
    }

    /// The text shown now, if any.
    pub fn shown(&self) -> Option<&str> {
        self.show
            .then(|| self.subtitles.get(self.current).map(|s| s.text.as_str()))
            .flatten()
    }
}

/// A video being played (`CVidTheoraPlayer`).
pub struct VideoPlayer {
    /// The file played.
    pub filename: String,
    video: Video,
    /// The picture size and frame rate.
    pub info: VideoInfo,
    /// How it is placed.
    pub playback: VideoPlayback,
    /// Where the picture's top-left corner is.
    pub x: i32,
    /// See `x`.
    pub y: i32,
    /// Scale in percent.
    pub zoom: f32,
    /// Whether the game froze for the video.
    pub freeze_game: bool,
    /// Every frame is shown, none dropped.
    pub dont_drop_frames: bool,
    started: bool,
    start_clock: u64,
    /// The sound track.
    pub sound: Option<SoundId>,
    pending: Option<Frame>,
    /// The frame shown.
    pub frame: Option<Frame>,
    /// Counts the frames shown, so that a new one is uploaded once.
    pub frame_serial: u64,
    /// Whether the video ended or was stopped.
    pub finished: bool,
    /// The subtitles, when a `.sub` file was found.
    pub subtitler: Option<Subtitler>,
    /// The height of the subtitle shown, measured this frame.
    pub subtitle_height: i32,
}

impl std::fmt::Debug for VideoPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoPlayer")
            .field("filename", &self.filename)
            .field("info", &self.info)
            .field("playback", &self.playback)
            .field("x", &self.x)
            .field("y", &self.y)
            .field("zoom", &self.zoom)
            .field("frame", &self.frame.as_ref().map(|f| f.index))
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl VideoPlayer {
    /// Whether the video plays.
    pub fn is_playing(&self) -> bool {
        !self.finished
    }

    /// The frame shown, by index.
    pub fn frame_index(&self) -> Option<u64> {
        self.frame.as_ref().map(|f| f.index)
    }

    /// The movie time in milliseconds (`GetMovieTime`): the sound's
    /// position when there is one, else the wall clock since the start.
    fn movie_time_ms(&self, clock: u64, sound_position: Option<u64>) -> u64 {
        if !self.started {
            return 0;
        }
        match sound_position {
            Some(position) => position,
            None => clock.saturating_sub(self.start_clock),
        }
    }
}

/// The path of a video's subtitle file: the same name with `.sub`.
fn subtitle_path_of(filename: &str) -> String {
    match filename.rfind('.') {
        Some(dot) if !filename[dot..].contains(['\\', '/']) => format!("{}.sub", &filename[..dot]),
        _ => format!("{filename}.sub"),
    }
}

impl World {
    /// The video playing, if any.
    pub fn video(&self) -> Option<&VideoPlayer> {
        self.video.as_ref()
    }

    /// Drops a video that ended, the frame after (the engine deletes the
    /// player once `IsFinished`).
    pub(crate) fn drop_finished_video(&mut self) {
        if self.video.as_ref().is_some_and(|v| !v.is_playing()) {
            self.video = None;
        }
    }

    /// Whether a video plays (`IsVideoPlaying`).
    pub fn is_video_playing(&self) -> bool {
        self.video.as_ref().is_some_and(VideoPlayer::is_playing)
    }

    /// Opens and starts a video (`Game.PlayTheora`): the game freezes,
    /// the sound track plays at the sound effects' volume, the subtitles
    /// load from `subtitle_file` or the video's `.sub`. `false` when the
    /// file cannot be opened.
    #[allow(clippy::too_many_arguments, reason = "the engine's own signature")]
    pub fn play_theora(
        &mut self,
        engine: &mut Engine,
        filename: &str,
        playback: VideoPlayback,
        x: i32,
        y: i32,
        freeze_music: bool,
        drop_frames: bool,
        subtitle_file: Option<&str>,
    ) -> bool {
        self.stop_video(engine);
        let bytes = match self.vfs.read(filename) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.log(format!("Error opening video file '{filename}': {err}"));
                return false;
            }
        };
        let sound = if luma_video::Video::open(bytes.clone())
            .as_ref()
            .is_ok_and(|v| v.info().has_audio)
        {
            match luma_audio::decode(&bytes) {
                Ok(clip) => {
                    let volume = self.sounds.audio.volume(SoundType::Sfx);
                    Some(
                        self.sounds
                            .audio
                            .add(Arc::new(clip), SoundType::Sfx, volume),
                    )
                }
                Err(err) => {
                    self.log(format!(
                        "Error decoding the sound of video '{filename}': {err}"
                    ));
                    None
                }
            }
        } else {
            None
        };
        let video = match Video::open(bytes) {
            Ok(video) => video,
            Err(err) => {
                self.log(format!("Error loading video '{filename}': {err}"));
                if let Some(sound) = sound {
                    self.sounds.audio.remove(sound);
                }
                return false;
            }
        };
        let info = video.info().clone();
        let (sw, sh) = (self.settings.width as f32, self.settings.height as f32);
        let (w, h) = (info.width as f32, info.height as f32);
        let (px, py, zoom) = match playback {
            VideoPlayback::Pos => (x, y, 100.0),
            VideoPlayback::Stretch => {
                let zoom = (sw / w.max(1.0) * 100.0).min(sh / h.max(1.0) * 100.0);
                (
                    ((sw - w * zoom / 100.0) / 2.0) as i32,
                    ((sh - h * zoom / 100.0) / 2.0) as i32,
                    zoom,
                )
            }
            VideoPlayback::Center => (((sw - w) / 2.0) as i32, ((sh - h) / 2.0) as i32, 100.0),
        };
        let sub_path = subtitle_file
            .map(str::to_string)
            .unwrap_or_else(|| subtitle_path_of(filename));
        let subtitler = self
            .vfs
            .read(&sub_path)
            .ok()
            .map(|bytes| Subtitler::new(parse_subtitles(&bytes)));
        self.freeze(engine, freeze_music);
        self.video = Some(VideoPlayer {
            filename: filename.to_string(),
            video,
            info,
            playback,
            x: px,
            y: py,
            zoom,
            freeze_game: true,
            dont_drop_frames: !drop_frames,
            started: false,
            start_clock: 0,
            sound,
            pending: None,
            frame: None,
            frame_serial: 0,
            finished: false,
            subtitler,
            subtitle_height: 0,
        });
        self.update_video(engine);
        true
    }

    /// Stops the video playing, if any (`StopVideo`): its sound stops
    /// and the game unfreezes.
    pub fn stop_video(&mut self, engine: &mut Engine) -> bool {
        let Some(mut player) = self.video.take() else {
            return false;
        };
        if let Some(sound) = player.sound.take() {
            self.sounds.audio.stop(sound);
            self.sounds.audio.remove(sound);
        }
        if player.freeze_game && !player.finished {
            self.unfreeze(engine);
        }
        true
    }

    /// One frame of the video (`CVidTheoraPlayer::Update`): decodes up
    /// to the frame due at the movie time, drops the late ones unless
    /// asked not to, starts the sound with the first frame, and ends the
    /// playback at the end of the stream.
    pub(crate) fn update_video(&mut self, engine: &mut Engine) {
        let clock = self.clock;
        let Some(player) = self.video.as_mut() else {
            return;
        };
        if player.finished {
            return;
        }
        let sound_position = player
            .sound
            .filter(|_| player.started)
            .map(|s| self.sounds.audio.position_ms(s));
        let movie_time = player.movie_time_ms(clock, sound_position);
        let mut ended = false;
        // The frame ready to show (`DecodeTheora`): the first decoded
        // whose time is not past, late ones being dropped as they are
        // decoded unless asked not to; a frame kept from an earlier
        // tick is shown once its time comes.
        loop {
            let fresh = player.pending.is_none();
            if fresh {
                match player.video.next_frame() {
                    Ok(Some(frame)) => player.pending = Some(frame),
                    Ok(None) => {
                        ended = true;
                        break;
                    }
                    Err(err) => {
                        let what = format!("Error decoding video '{}': {err}", player.filename);
                        self.log(what);
                        ended = true;
                        break;
                    }
                }
            }
            let late = player
                .pending
                .as_ref()
                .is_some_and(|f| f.time_ms < movie_time);
            if !late || !fresh || player.dont_drop_frames || !player.started {
                break;
            }
            player.pending = None;
        }
        let Some(player) = self.video.as_mut() else {
            return;
        };
        if ended && player.pending.is_none() {
            player.finished = true;
            let sound = player.sound.take();
            let unfreeze = player.freeze_game;
            if let Some(sound) = sound {
                self.sounds.audio.stop(sound);
                self.sounds.audio.remove(sound);
            }
            if unfreeze {
                self.unfreeze(engine);
            }
            return;
        }
        if !player.started {
            player.started = true;
            player.start_clock = clock;
            if let Some(sound) = player.sound {
                self.sounds.audio.play(sound, false);
            }
        }
        let Some(player) = self.video.as_mut() else {
            return;
        };
        let show = player
            .pending
            .as_ref()
            .is_some_and(|f| player.frame.is_none() || f.time_ms <= movie_time);
        if show {
            player.frame = player.pending.take();
            player.frame_serial += 1;
        }
        if let Some(subtitler) = player.subtitler.as_mut() {
            let frame = player.info.frame_at_ms(movie_time) as i64;
            subtitler.update(frame);
        }
        self.prepare_video_subtitle();
    }

    /// The font of the video subtitles: `VIDEO_FONT`, else the system
    /// font.
    fn video_font(&self) -> Option<String> {
        self.video_font.clone().or_else(|| self.system_font.clone())
    }

    fn prepare_video_subtitle(&mut self) {
        if !self.video_subtitles {
            return;
        }
        let Some(text) = self
            .video
            .as_ref()
            .and_then(|p| p.subtitler.as_ref())
            .and_then(Subtitler::shown)
            .map(str::to_string)
        else {
            return;
        };
        let width = self.settings.width as i32;
        let Some(font) = self.video_font() else {
            return;
        };
        let height = self.fonts.get_mut(&font).map_or(0, |f| {
            f.prepare(&text, width, TextAlign::Center, -1);
            f.text_height(&text, width)
        });
        if let Some(player) = self.video.as_mut() {
            player.subtitle_height = height;
        }
    }

    /// Uploads the frame shown as a texture, reusing the texture while
    /// the size does not change.
    pub(crate) fn upload_video(&mut self, renderer: &luma_render::Renderer) {
        let Some(player) = self.video.as_ref() else {
            if let Some(texture) = self.video_texture.take() {
                renderer.forget(&texture);
            }
            self.video_uploaded = 0;
            return;
        };
        let Some(frame) = player.frame.as_ref() else {
            return;
        };
        if self.video_uploaded == player.frame_serial && self.video_texture.is_some() {
            return;
        }
        let same_size = self
            .video_texture
            .as_ref()
            .is_some_and(|t| t.width == frame.width && t.height == frame.height);
        if same_size {
            if let Some(texture) = &self.video_texture {
                renderer.write_texture(texture, &frame.rgba);
            }
        } else {
            if let Some(old) = self.video_texture.take() {
                renderer.forget(&old);
            }
            self.video_texture =
                Some(renderer.texture_from_rgba("video", frame.width, frame.height, &frame.rgba));
        }
        self.video_uploaded = player.frame_serial;
    }

    /// Draws the video and its subtitle (`CVidTheoraPlayer::Display`).
    pub(crate) fn draw_video<'a>(&'a self, list: &mut DrawList<'a>) {
        let Some(player) = self.video.as_ref() else {
            return;
        };
        if let (Some(texture), Some(frame)) = (&self.video_texture, &player.frame) {
            let _ = frame;
            let zoom = player.zoom / 100.0;
            list.push(Quad {
                texture,
                dst: Rect::new(
                    player.x as f32,
                    player.y as f32,
                    (texture.width as f32 * zoom).trunc(),
                    (texture.height as f32 * zoom).trunc(),
                ),
                src: None,
                tint: luma_formats::Rgba::OPAQUE_WHITE,
                flip_x: false,
                flip_y: false,
            });
        }
        if !self.video_subtitles {
            return;
        }
        let Some(text) = player.subtitler.as_ref().and_then(Subtitler::shown) else {
            return;
        };
        let Some(font) = self.video_font().and_then(|f| self.fonts.get(&f)) else {
            return;
        };
        let (width, height) = (self.settings.width as i32, self.settings.height as i32);
        font.draw(
            list,
            text,
            0,
            height - player.subtitle_height - 5,
            width,
            TextAlign::Center,
            -1,
        );
    }

    /// The texture of the frame shown, once uploaded.
    pub fn video_texture(&self) -> Option<&Texture> {
        self.video_texture.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subtitles_and_times_them() {
        let subs = parse_subtitles(
            b"{12}{48}First line|second line\r\n{60}{0}Stays\n{1}{1}dropped\nno braces\n{100}{110}Last",
        );
        assert_eq!(subs.len(), 3);
        assert_eq!(subs[0].text, "First line\nsecond line");
        assert_eq!((subs[0].start, subs[0].end), (12, 48));
        assert_eq!((subs[1].start, subs[1].end), (60, 0));
        let mut s = Subtitler::new(subs);
        // The engine shows a subtitle from the end of the previous one,
        // so the first shows from the start.
        s.update(5);
        assert_eq!(s.shown(), Some("First line\nsecond line"));
        s.update(20);
        assert_eq!(s.shown(), Some("First line\nsecond line"));
        s.update(50);
        assert_eq!(s.shown(), None);
        s.update(70);
        assert_eq!(s.shown(), Some("Stays"));
        s.update(99);
        assert_eq!(s.shown(), Some("Stays"), "an end of 0 keeps the subtitle");
        s.update(105);
        assert_eq!(s.shown(), Some("Last"));
        s.update(200);
        assert_eq!(s.shown(), None);
    }

    #[test]
    fn subtitle_paths_and_playback_codes() {
        assert_eq!(
            subtitle_path_of("assets\\data0001.ogg"),
            "assets\\data0001.sub"
        );
        assert_eq!(subtitle_path_of("video"), "video.sub");
        assert_eq!(subtitle_path_of("dir.v\\clip"), "dir.v\\clip.sub");
        assert_eq!(VideoPlayback::from_code(0), VideoPlayback::Pos);
        assert_eq!(VideoPlayback::from_code(2), VideoPlayback::Center);
        assert_eq!(VideoPlayback::from_code(7), VideoPlayback::Stretch);
    }
}
