//! Video playback in the game (`Game.PlayTheora`) and in an entity's
//! place (`Entity.PlayTheora`): timing, skipping, looping, subtitles and
//! drawing of a video decoded by `luma-video`.
//!
//! Written with reference to `VidTheoraPlayer.cpp`, `VidSubtitler.cpp`,
//! `BGame.cpp` and `AdEntity.cpp` of Wintermute Engine 1.x (MIT License,
//! Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`. The rules are
//! documented in `docs/video.md`.

use std::sync::Arc;

use luma_audio::{Audio, SoundId, SoundType};
use luma_formats::font::TextAlign;
use luma_formats::{DEFAULT_COLOR_KEY, Image, Rgba};
use luma_render::{DrawList, Quad, Rect, Texture};
use luma_script::{Engine, NativeId};
use luma_video::{Frame, Video, VideoInfo};

use crate::objects::Kind;
use crate::scene_view::{EntityRef, VideoOverlay};
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
/// `{1}{1}` are dropped. The text is kept as the file holds it, one
/// `char` per byte, like the rest of the game's text: the font reads it
/// in the game's encoding when it is drawn.
pub fn parse_subtitles(bytes: &[u8]) -> Vec<Subtitle> {
    let text: String = bytes.iter().map(|&b| char::from(b)).collect();
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
    /// Starts again at its end (an entity's video).
    pub looping: bool,
    /// Held by `PauseTheora`.
    pub paused: bool,
    /// Where playback started in the movie (`StartTime`), in
    /// milliseconds.
    pub time_offset_ms: u64,
    /// An image whose alpha channel masks the frames (an entity's video
    /// with an alpha file).
    alpha: Option<Image>,
    started: bool,
    start_clock: u64,
    /// When the pause started, on the clock timing the video.
    pause_started: Option<u64>,
    /// The clock and the sound's position when the sound was last found
    /// playing: where the movie time goes on from once the sound ends.
    sound_heard: Option<(u64, u64)>,
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
    /// A player of a video used as a texture (`SetTheoraTexture`): loops,
    /// has no sound, no placement and no subtitles.
    #[cfg(feature = "3d")]
    pub(crate) fn texture_loop(filename: &str, video: Video) -> VideoPlayer {
        let info = video.info().clone();
        VideoPlayer {
            filename: filename.to_string(),
            video,
            info,
            playback: VideoPlayback::Pos,
            x: 0,
            y: 0,
            zoom: 100.0,
            freeze_game: false,
            dont_drop_frames: false,
            looping: true,
            paused: false,
            time_offset_ms: 0,
            alpha: None,
            started: false,
            start_clock: 0,
            pause_started: None,
            sound_heard: None,
            sound: None,
            pending: None,
            frame: None,
            frame_serial: 0,
            finished: false,
            subtitler: None,
            subtitle_height: 0,
        }
    }

    /// Starts the video again from its first frame (`ResetStream`); an
    /// error when the stream cannot be rewound.
    #[cfg(feature = "3d")]
    pub(crate) fn restart(&mut self) -> Result<(), String> {
        self.video.rewind().map_err(|e| e.to_string())?;
        self.started = false;
        self.sound_heard = None;
        self.time_offset_ms = 0;
        self.pending = None;
        Ok(())
    }

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
            return self.time_offset_ms;
        }
        match sound_position {
            Some(position) => position,
            None => clock.saturating_sub(self.start_clock) + self.time_offset_ms,
        }
    }

    /// Whether the video plays and is not paused (`IsPlaying`).
    pub fn is_running(&self) -> bool {
        !self.finished && !self.paused
    }
}

/// A video shown in an entity's place (`CAdEntity::m_Theora`).
pub struct EntityVideo {
    /// The playback.
    pub player: VideoPlayer,
    /// The frame shown, once uploaded.
    pub texture: Option<Texture>,
    uploaded: u64,
}

/// Gives a frame the alpha of a mask image, pixel by pixel; outside the
/// mask the frame stays opaque (`CVidTheoraPlayer::GetAlphaAt`).
fn apply_alpha(frame: &mut Frame, mask: &Image) {
    for y in 0..frame.height {
        for x in 0..frame.width {
            let alpha = if x < mask.width && y < mask.height {
                mask.pixels[((y * mask.width + x) * 4 + 3) as usize]
            } else {
                0xff
            };
            frame.rgba[((y * frame.width + x) * 4 + 3) as usize] = alpha;
        }
    }
}

/// One step of a video's playback (`CVidTheoraPlayer::Update`): decodes
/// up to the frame due at the movie time on the clock `now`, drops the
/// late ones unless asked not to, starts the sound with the first frame
/// and shows the frame that is due. `true` when the stream ended and
/// nothing is left to show.
pub(crate) fn step_player(
    player: &mut VideoPlayer,
    audio: Option<&mut Audio>,
    now: u64,
    log: &mut Vec<String>,
) -> bool {
    // The sound's position while it plays. A sound track shorter than
    // the pictures ends first: the movie then goes on on the clock, from
    // where the sound was last heard.
    let sound_position = match (player.sound.filter(|_| player.started), audio.as_deref()) {
        (Some(sound), Some(audio)) if audio.is_playing(sound) => {
            let position = audio.position_ms(sound);
            player.sound_heard = Some((now, position));
            Some(position)
        }
        (Some(_), Some(_)) => Some(match player.sound_heard {
            Some((clock, position)) => position + now.saturating_sub(clock),
            None => now.saturating_sub(player.start_clock) + player.time_offset_ms,
        }),
        _ => None,
    };
    let movie_time = player.movie_time_ms(now, sound_position);
    let mut ended = false;
    // The frame ready to show (`DecodeTheora`): the first decoded whose
    // time is not past, late ones being dropped as they are decoded
    // unless asked not to; a frame kept from an earlier tick is shown
    // once its time comes.
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
                    log.push(format!("Error decoding video '{}': {err}", player.filename));
                    ended = true;
                    break;
                }
            }
        }
        let late = player
            .pending
            .as_ref()
            .is_some_and(|f| f.time_ms < movie_time);
        // Before the start, the frames up to the start time are skipped
        // (`SeekToTime`).
        let seeking = !player.started && player.time_offset_ms != 0;
        if !late || !fresh || (!seeking && (player.dont_drop_frames || !player.started)) {
            break;
        }
        player.pending = None;
    }
    if ended && player.pending.is_none() {
        return true;
    }
    if !player.started {
        player.started = true;
        player.start_clock = now;
        if let (Some(sound), Some(audio)) = (player.sound, audio) {
            if player.time_offset_ms != 0 {
                audio.set_position_ms(sound, player.time_offset_ms);
            }
            audio.play(sound, false);
        }
    }
    let show = player
        .pending
        .as_ref()
        .is_some_and(|f| player.frame.is_none() || f.time_ms <= movie_time);
    if show {
        let mut frame = player.pending.take();
        if let (Some(frame), Some(mask)) = (frame.as_mut(), player.alpha.as_ref()) {
            apply_alpha(frame, mask);
        }
        player.frame = frame;
        player.frame_serial += 1;
    }
    if let Some(subtitler) = player.subtitler.as_mut() {
        let frame = player.info.frame_at_ms(movie_time) as i64;
        subtitler.update(frame);
    }
    false
}

/// Uploads the frame a player shows as `texture`, reusing it while the
/// size does not change; `uploaded` remembers the frame uploaded.
pub(crate) fn upload_frame(
    renderer: &luma_render::Renderer,
    player: &VideoPlayer,
    texture: &mut Option<Texture>,
    uploaded: &mut u64,
) {
    let Some(frame) = player.frame.as_ref() else {
        return;
    };
    if *uploaded == player.frame_serial && texture.is_some() {
        return;
    }
    let same_size = texture
        .as_ref()
        .is_some_and(|t| t.width == frame.width && t.height == frame.height);
    if same_size {
        if let Some(texture) = texture {
            renderer.write_texture(texture, &frame.rgba);
        }
    } else {
        if let Some(old) = texture.take() {
            renderer.forget(&old);
        }
        *texture =
            Some(renderer.texture_from_rgba("video", frame.width, frame.height, &frame.rgba));
    }
    *uploaded = player.frame_serial;
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

    /// Opens a video file and loads its sound track as a sound effect at
    /// `volume` percent (`CVidTheoraPlayer::Initialize`); `None`, logged,
    /// when the file cannot be read or decoded.
    fn open_video(&mut self, filename: &str, volume: i32) -> Option<(Video, Option<SoundId>)> {
        let bytes = match self.vfs.read(filename) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.log(format!("Error opening video file '{filename}': {err}"));
                return None;
            }
        };
        // The sound track on its own, closed: the sound decoder reads the
        // whole container and refuses a file whose streams never end.
        let track = luma_video::sound_track(&bytes);
        let video = match Video::open(bytes) {
            Ok(video) => video,
            Err(err) => {
                self.log(format!("Error loading video '{filename}': {err}"));
                return None;
            }
        };
        let sound = track.and_then(|track| match luma_audio::decode(&track) {
            Ok(clip) => Some(
                self.sounds
                    .audio
                    .add(Arc::new(clip), SoundType::Sfx, volume),
            ),
            Err(err) => {
                self.log(format!(
                    "Error decoding the sound of video '{filename}': {err}"
                ));
                None
            }
        });
        Some((video, sound))
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
        let volume = self.sounds.audio.volume(SoundType::Sfx);
        let Some((video, sound)) = self.open_video(filename, volume) else {
            return false;
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
            looping: false,
            paused: false,
            time_offset_ms: 0,
            alpha: None,
            started: false,
            start_clock: 0,
            pause_started: None,
            sound_heard: None,
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
        let ended = step_player(player, Some(&mut self.sounds.audio), clock, &mut self.log);
        if ended {
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
        self.prepare_video_subtitle();
    }

    // ----------------------------------------------------------------
    // Videos in an entity's place
    // ----------------------------------------------------------------

    /// The video an entity plays, if any (`Entity.PlayTheora`).
    pub fn entity_video(&self, owner: NativeId) -> Option<&VideoPlayer> {
        self.entity_videos.get(&owner).map(|v| &v.player)
    }

    /// Plays a video in an entity's place (`CAdEntity::ScCallMethod`
    /// `PlayTheora`): at the entity's position, scaled by its `Scale`
    /// when it has one, at the entity's sound volume, from `start_ms`,
    /// looping when asked, with the alpha channel of `alpha_file` masking
    /// the frames. The game runs on; the entity's sprite shows again when
    /// the video ends. `false`, logged, when the file cannot be opened.
    pub fn play_entity_theora(
        &mut self,
        owner: NativeId,
        filename: &str,
        looping: bool,
        alpha_file: Option<&str>,
        start_ms: u64,
    ) -> bool {
        if !matches!(
            self.objects.get(owner).map(|n| &n.kind),
            Some(Kind::Entity(_))
        ) {
            return false;
        }
        self.stop_entity_video(owner);
        let volume = self.sounds.objects.get(&owner).map_or(100, |s| s.volume);
        let Some((video, sound)) = self.open_video(filename, volume) else {
            return false;
        };
        let alpha = alpha_file.and_then(|file| match self.vfs.read(file) {
            Ok(bytes) => match luma_formats::image::decode(&bytes, DEFAULT_COLOR_KEY) {
                Ok(image) => Some(image),
                Err(err) => {
                    self.log(format!("Error loading alpha image '{file}': {err}"));
                    None
                }
            },
            Err(err) => {
                self.log(format!("Error opening alpha image '{file}': {err}"));
                None
            }
        });
        let info = video.info().clone();
        let player = VideoPlayer {
            filename: filename.to_string(),
            video,
            info,
            playback: VideoPlayback::Pos,
            x: 0,
            y: 0,
            zoom: 100.0,
            freeze_game: false,
            dont_drop_frames: false,
            looping,
            paused: false,
            time_offset_ms: start_ms,
            alpha,
            started: false,
            start_clock: 0,
            pause_started: None,
            sound_heard: None,
            sound,
            pending: None,
            frame: None,
            frame_serial: 0,
            finished: false,
            subtitler: None,
            subtitle_height: 0,
        };
        self.entity_videos.insert(
            owner,
            EntityVideo {
                player,
                texture: None,
                uploaded: 0,
            },
        );
        self.update_entity_video(owner);
        true
    }

    /// Stops and drops an entity's video (`StopTheora`); whether it had
    /// one.
    pub fn stop_entity_video(&mut self, owner: NativeId) -> bool {
        let Some(mut video) = self.entity_videos.remove(&owner) else {
            return false;
        };
        if let Some(sound) = video.player.sound.take() {
            self.sounds.audio.stop(sound);
            self.sounds.audio.remove(sound);
        }
        if let Some(texture) = video.texture.take() {
            self.dropped_textures.push(texture);
        }
        true
    }

    /// Pauses an entity's video and its sound (`PauseTheora`); `false`
    /// when none plays.
    pub fn pause_entity_video(&mut self, owner: NativeId) -> bool {
        let timer = self.timer;
        let Some(video) = self.entity_videos.get_mut(&owner) else {
            return false;
        };
        if !video.player.is_running() {
            return false;
        }
        video.player.paused = true;
        video.player.pause_started = Some(timer);
        if let Some(sound) = video.player.sound {
            self.sounds.audio.pause(sound);
        }
        true
    }

    /// Resumes an entity's paused video (`ResumeTheora`); `false` when
    /// none is paused.
    pub fn resume_entity_video(&mut self, owner: NativeId) -> bool {
        let timer = self.timer;
        let Some(video) = self.entity_videos.get_mut(&owner) else {
            return false;
        };
        if !video.player.paused {
            return false;
        }
        video.player.paused = false;
        if let Some(since) = video.player.pause_started.take() {
            let paused_for = timer.saturating_sub(since);
            video.player.start_clock += paused_for;
            if let Some((clock, _)) = video.player.sound_heard.as_mut() {
                *clock += paused_for;
            }
        }
        if let Some(sound) = video.player.sound {
            self.sounds.audio.resume(sound);
        }
        true
    }

    /// One frame of the entities' videos (`CAdEntity::Update`): each
    /// advances on the game timer, so none moves while the game is
    /// frozen; a looping one starts again at its end, another ends and
    /// its entity shows its sprite again. The videos of entities that are
    /// gone go with them.
    pub(crate) fn update_entity_videos(&mut self) {
        let owners: Vec<NativeId> = self.entity_videos.keys().copied().collect();
        for owner in owners {
            if !self.objects.contains(owner) {
                self.stop_entity_video(owner);
                continue;
            }
            self.update_entity_video(owner);
        }
    }

    fn update_entity_video(&mut self, owner: NativeId) {
        if self.is_frozen() {
            return;
        }
        let timer = self.timer;
        let Some(video) = self.entity_videos.get_mut(&owner) else {
            return;
        };
        if video.player.paused {
            return;
        }
        let ended = step_player(
            &mut video.player,
            Some(&mut self.sounds.audio),
            timer,
            &mut self.log,
        );
        if !ended {
            return;
        }
        if !video.player.looping {
            self.stop_entity_video(owner);
            return;
        }
        // `ResetStream`: back to the first frame, the sound with it.
        match video.player.video.rewind() {
            Ok(()) => {
                video.player.started = false;
                video.player.sound_heard = None;
                video.player.time_offset_ms = 0;
                video.player.pending = None;
                if let Some(sound) = video.player.sound {
                    self.sounds.audio.stop(sound);
                }
            }
            Err(err) => {
                let filename = video.player.filename.clone();
                self.log(format!("Error restarting video '{filename}': {err}"));
                self.stop_entity_video(owner);
            }
        }
    }

    /// The videos drawn in the scene's entities' places this frame: an
    /// entity's video at its position, scaled by its `Scale` and tinted
    /// like it (`CAdEntity::Display`).
    pub(crate) fn video_overlays(&self) -> Vec<VideoOverlay<'_>> {
        let Some(view) = self.scene.as_ref() else {
            return Vec::new();
        };
        self.entity_videos
            .iter()
            .filter_map(|(owner, video)| {
                let texture = video.texture.as_ref()?;
                let Kind::Entity(r) = self.objects.get(*owner)?.kind else {
                    return None;
                };
                if matches!(r, EntityRef::Ui(_)) {
                    return None;
                }
                let entity = view.entity(r)?;
                Some(VideoOverlay {
                    entity: r,
                    texture,
                    zoom: entity.scale.filter(|s| *s >= 0.0).unwrap_or(100.0),
                    tint: view.scene.entity_tint(entity),
                })
            })
            .collect()
    }

    /// The quad of a container entity's video, drawn at `(x, y)` on
    /// screen, if the entity plays one.
    pub(crate) fn ui_video_quad<'a>(
        &'a self,
        owner: NativeId,
        x: i32,
        y: i32,
        tint: Rgba,
    ) -> Option<Quad<'a>> {
        let video = self.entity_videos.get(&owner)?;
        let texture = video.texture.as_ref()?;
        let Kind::Entity(EntityRef::Ui(index)) = self.objects.get(owner)?.kind else {
            return None;
        };
        let entity = self.ui_entities.get(index)?;
        let zoom = entity.def.scale.filter(|s| *s >= 0.0).unwrap_or(100.0) / 100.0;
        Some(Quad {
            texture,
            dst: Rect::new(
                x as f32,
                y as f32,
                (texture.width as f32 * zoom).trunc(),
                (texture.height as f32 * zoom).trunc(),
            ),
            src: None,
            tint: entity.def.tint.unwrap_or(tint),
            flip_x: false,
            flip_y: false,
        })
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
        for texture in self.dropped_textures.drain(..) {
            renderer.forget(&texture);
        }
        for video in self.entity_videos.values_mut() {
            upload_frame(
                renderer,
                &video.player,
                &mut video.texture,
                &mut video.uploaded,
            );
        }
        let Some(player) = self.video.as_ref() else {
            if let Some(texture) = self.video_texture.take() {
                renderer.forget(&texture);
            }
            self.video_uploaded = 0;
            return;
        };
        upload_frame(
            renderer,
            player,
            &mut self.video_texture,
            &mut self.video_uploaded,
        );
    }

    /// Draws the video and its subtitle (`CVidTheoraPlayer::Display`).
    pub(crate) fn draw_video<'a>(&'a self, list: &mut DrawList<'a>) {
        let Some(player) = self.video.as_ref() else {
            return;
        };
        if let (Some(texture), Some(_)) = (&self.video_texture, &player.frame) {
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

    fn player_of(bytes: Vec<u8>, sound: Option<SoundId>) -> VideoPlayer {
        let video = Video::open(bytes).unwrap();
        let info = video.info().clone();
        VideoPlayer {
            filename: "clip.ogv".into(),
            video,
            info,
            playback: VideoPlayback::Pos,
            x: 0,
            y: 0,
            zoom: 100.0,
            freeze_game: false,
            dont_drop_frames: false,
            looping: false,
            paused: false,
            time_offset_ms: 0,
            alpha: None,
            started: false,
            start_clock: 0,
            pause_started: None,
            sound_heard: None,
            sound,
            pending: None,
            frame: None,
            frame_serial: 0,
            finished: false,
            subtitler: None,
            subtitle_height: 0,
        }
    }

    #[test]
    fn frames_show_at_the_times_of_their_frame_numbers() {
        // Four pictures at 10 frames per second, numbered 0, 1, 5 and 9:
        // shown at 0, 100, 500 and 900 ms, not at 0, 100, 200 and 300.
        let bytes = luma_video::synthetic_clip_at(32, 32, 10, &[0, 1, 5, 9]);
        let mut player = player_of(bytes, None);
        let mut log = Vec::new();
        let mut shown = Vec::new();
        let mut ended_at = None;
        // The clock starts anywhere; a step every 20 ms.
        for step in 0..100u64 {
            if step_player(&mut player, None, 5000 + step * 20, &mut log) {
                ended_at = Some(step * 20);
                break;
            }
            let index = player.frame_index().unwrap();
            if shown.last().is_none_or(|(_, last)| *last != index) {
                shown.push((step * 20, index));
            }
        }
        assert_eq!(shown, [(0, 0), (100, 1), (500, 2), (900, 3)]);
        assert_eq!(ended_at, Some(920), "the step after the last frame");
        assert!(log.is_empty(), "{log:?}");
    }

    #[test]
    fn the_movie_goes_on_after_a_shorter_sound_track() {
        // Eight frames at 10 frames per second, the last at 700 ms, with
        // 300 ms of sound.
        let bytes = luma_video::synthetic_clip(32, 32, 8, 10);
        let mut audio = Audio::null();
        let sound = audio.add(
            Arc::new(luma_audio::Clip::silence(8000, 300)),
            SoundType::Sfx,
            100,
        );
        let mut player = player_of(bytes, Some(sound));
        let mut log = Vec::new();
        let mut shown = Vec::new();
        let mut ended_at = None;
        for step in 0..100u64 {
            if step_player(&mut player, Some(&mut audio), step * 20, &mut log) {
                ended_at = Some(step * 20);
                break;
            }
            let index = player.frame_index().unwrap();
            if shown.last().is_none_or(|(_, last)| *last != index) {
                shown.push((step * 20, index));
            }
            audio.tick(20);
        }
        assert!(!audio.is_playing(sound), "the sound ended first");
        let indices: Vec<u64> = shown.iter().map(|(_, index)| *index).collect();
        assert_eq!(indices, [0, 1, 2, 3, 4, 5, 6, 7], "{shown:?}");
        // Each frame within a step or two of its time, before and after
        // the end of the sound.
        for (at, index) in &shown {
            assert!(
                (index * 100..=index * 100 + 40).contains(at),
                "frame {index} at {at} ms"
            );
        }
        let ended_at = ended_at.expect("the video ends");
        assert!((720..=760).contains(&ended_at), "{ended_at}");
        assert!(log.is_empty(), "{log:?}");
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
