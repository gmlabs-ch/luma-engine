//! The audio system: Kira mixing the sounds on the default output device,
//! or on a mock backend that the game clock drives when no device is
//! there (continuous integration, headless runs, the `device` feature
//! off).
//!
//! The rules of a sound (when it plays, pauses, resumes, loops, how
//! loud and where between the speakers) follow the original engine
//! (`BSoundMgr.cpp`, `BSoundBuffer.cpp`, `BSoundOGG.cpp` and `BSound.cpp`
//! of Wintermute Engine 1.x, MIT License, Copyright (c) 2003-2013 Jan
//! Nedoma); see `NOTICE` and `docs/audio.md`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::effect::delay::{DelayBuilder, DelayHandle};
use kira::effect::reverb::{ReverbBuilder, ReverbHandle};
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::{MainTrackBuilder, TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Capacities, Decibels, Mix, Panning, Tween};

use crate::clip::Clip;
use crate::error::{Error, Result};

/// What a sound is, for the global volume it follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundType {
    /// A sound effect.
    Sfx,
    /// Speech.
    Speech,
    /// Music.
    Music,
}

/// A sound loaded into the audio system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundId(pub u64);

/// An effect on a sound, with the parameters of the DirectSound effects
/// the engine applied (`CBSoundBuffer::ApplyFX`), mapped onto Kira's
/// delay and reverb (see `docs/audio.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SoundEffect {
    /// `SoundFXEcho`.
    Echo {
        /// Wet/dry mix in percent (0-100).
        wet_dry_mix: f32,
        /// Feedback in percent (0-100).
        feedback: f32,
        /// Left delay in milliseconds (1-2000).
        left_delay_ms: f32,
        /// Right delay in milliseconds (1-2000).
        right_delay_ms: f32,
    },
    /// `SoundFXReverb`.
    Reverb {
        /// Input gain in decibels (-96-0).
        in_gain_db: f32,
        /// Reverb mix in decibels (-96-0).
        reverb_mix_db: f32,
        /// Reverb time in milliseconds (0.001-3000).
        reverb_time_ms: f32,
        /// High-frequency reverb time ratio (0.001-0.999).
        high_freq_rt_ratio: f32,
    },
}

/// The Kira parameters of an effect: the track's volume and the effect
/// settings.
struct EffectParams {
    track_db: Decibels,
    delay: Option<(Duration, Decibels, Mix)>,
    reverb: Option<(f64, f64, Mix)>,
}

impl SoundEffect {
    /// The engine's parameters as Kira's. The echo's two delays become
    /// one, their mean, since Kira's delay has one line; the reverb's
    /// time becomes the feedback of a comb filter of 35 ms (Freeverb's
    /// tuning) that decays by 60 dB in that time, and its high-frequency
    /// ratio the damping.
    fn params(self) -> EffectParams {
        match self {
            SoundEffect::Echo {
                wet_dry_mix,
                feedback,
                left_delay_ms,
                right_delay_ms,
            } => {
                let delay_ms = ((left_delay_ms + right_delay_ms) / 2.0).clamp(1.0, 2000.0);
                let feedback = decibels((feedback / 100.0).clamp(0.0, 1.0));
                let mix = Mix((wet_dry_mix / 100.0).clamp(0.0, 1.0));
                EffectParams {
                    track_db: Decibels::IDENTITY,
                    delay: Some((Duration::from_secs_f32(delay_ms / 1000.0), feedback, mix)),
                    reverb: None,
                }
            }
            SoundEffect::Reverb {
                in_gain_db,
                reverb_mix_db,
                reverb_time_ms,
                high_freq_rt_ratio,
            } => {
                let seconds = (reverb_time_ms / 1000.0).clamp(0.000_001, 3.0);
                let feedback = 10f32.powf(-3.0 * 0.035 / seconds).clamp(0.0, 0.98);
                let damping = (1.0 - high_freq_rt_ratio).clamp(0.0, 1.0);
                let wet = 10f32.powf(reverb_mix_db.clamp(-96.0, 0.0) / 20.0);
                let mix = Mix(wet / (1.0 + wet));
                EffectParams {
                    track_db: Decibels(in_gain_db.clamp(-96.0, 0.0)),
                    delay: None,
                    reverb: Some((f64::from(feedback), f64::from(damping), mix)),
                }
            }
        }
    }

    /// Whether two effects are of the same kind, so that one can turn
    /// into the other without restarting the sound.
    fn same_kind(self, other: SoundEffect) -> bool {
        matches!(
            (self, other),
            (SoundEffect::Echo { .. }, SoundEffect::Echo { .. })
                | (SoundEffect::Reverb { .. }, SoundEffect::Reverb { .. })
        )
    }
}

/// The handles of a sound's effect, to change its parameters.
enum EffectHandle {
    Delay(DelayHandle),
    Reverb(ReverbHandle),
}

/// The track a sound with an effect plays on, with the effect.
struct EffectTrack {
    track: TrackHandle,
    handle: EffectHandle,
    /// The delay time the track was built with (Kira cannot change it).
    delay: Option<Duration>,
}

/// The volume DirectSound was given for a sound
/// (`CBSoundBuffer::SetVolume`), in hundredths of a decibel: the type's
/// volume (0 to 200 for 0 to 100 %) weighted by the sound's own volume
/// in percent and truncated, the remaining fraction cubed, times
/// -100 dB, truncated. `None` when the result is outside what
/// DirectSound accepts (-10000 to 0): the call failed and the sound
/// kept the loudness it had.
pub fn attenuation(type_volume: i32, private_volume: i32) -> Option<i32> {
    let volume = (type_volume as f32 / 100.0 * private_volume as f32) as i32;
    let remaining = 1.0 - f64::from(volume) / 200.0;
    let centibels = (-10000.0 * remaining.powi(3)) as i32;
    (-10000..=0).contains(&centibels).then_some(centibels)
}

/// The gain of a volume in percent, as the engine attenuates it: the
/// cube of the remaining fraction, in hundredths of a decibel, turned
/// back into a factor.
pub fn gain(volume_percent: f32) -> f32 {
    let remaining = 1.0 - (volume_percent / 100.0).clamp(0.0, 1.0);
    let attenuation = -10000.0 * remaining.powi(3);
    10f32.powf(attenuation / 2000.0)
}

/// DirectSound's pan (`IDirectSoundBuffer::SetPan`, hundredths of a
/// decibel taken off the far channel, the near one untouched) as Kira's:
/// the panning that gives the two channels that ratio, and the decibels
/// that bring the near channel back to its level.
fn directsound_pan(pan: i32) -> (Panning, f32) {
    if pan == 0 {
        return (Panning::CENTER, 0.0);
    }
    let far = 10f32.powf(-(pan.abs() as f32) / 2000.0);
    let far2 = far * far;
    let side = (1.0 - far2) / (1.0 + far2);
    let compensation = 10.0 * ((1.0 + far2) / 2.0).log10();
    (Panning(if pan < 0 { -side } else { side }), compensation)
}

/// The leftmost pan DirectSound accepts (`DSBPAN_LEFT`).
pub const PAN_LEFT: i32 = -10000;
/// The rightmost pan DirectSound accepts (`DSBPAN_RIGHT`).
pub const PAN_RIGHT: i32 = 10000;

/// Files larger than this are streamed by the engine even when the
/// caller did not ask (`MAX_NONSTREAMED_FILE_SIZE`).
const MAX_NONSTREAMED_FILE_SIZE: usize = 1024 * 1024;
/// The seconds of sound a streaming buffer holds
/// (`CBGame::m_SoundBufferSizeSec`) and its number of blocks.
const STREAM_BUFFER_SECONDS: u64 = 3;
const STREAM_BLOCKS: u64 = 16;

/// Whether the engine streams a sound (`CBSoundBuffer::LoadFromFile`,
/// `CreateSoundBuffer`): when the caller asks for it or the file is
/// larger than 1 MB, and the sound is at least one streaming block long
/// (3 s / 16). Only a streamed sound loops from its loop start.
pub fn is_streamed(clip: &Clip, asked: bool) -> bool {
    let block_frames = u64::from(clip.sample_rate()) * STREAM_BUFFER_SECONDS / STREAM_BLOCKS;
    (asked || clip.file_size() > MAX_NONSTREAMED_FILE_SIZE) && clip.frames() as u64 >= block_frames
}

/// A factor as decibels, floored at Kira's silence.
fn decibels(factor: f32) -> Decibels {
    if factor <= 0.0 {
        return Decibels::SILENCE;
    }
    Decibels((20.0 * factor.log10()).max(Decibels::SILENCE.0))
}

/// The mock backend runs at this rate, so that a millisecond is a whole
/// number of frames.
const MOCK_RATE: u32 = 8000;
/// Frames the mock backend renders per step: one millisecond.
const MOCK_STEP: usize = 8;

enum Manager {
    #[cfg(feature = "device")]
    Device(Box<AudioManager<kira::backend::cpal::CpalBackend>>, String),
    Mock(Box<AudioManager<MockBackend>>),
}

struct Voice {
    clip: Arc<Clip>,
    kind: SoundType,
    handle: Option<StaticSoundHandle>,
    /// The effect asked for the sound.
    effect: Option<SoundEffect>,
    /// The track with the effect, while the sound plays with one.
    track: Option<EffectTrack>,
    looping: bool,
    loop_start_ms: u64,
    /// Whether the engine streams the sound: only then does a loop
    /// restart at `loop_start_ms`.
    streamed: bool,
    /// Where every `play` starts (`CBSound::m_SoundPosition`).
    start_ms: u64,
    /// Where the buffer stands while nothing plays: where a resume
    /// starts a sound that is not held (the DirectSound play cursor).
    cursor_ms: u64,
    private_volume: i32,
    /// The volume in hundredths of a decibel, the last one DirectSound
    /// accepted.
    centibels: i32,
    /// The pan in hundredths of a decibel, the last one DirectSound
    /// accepted.
    pan: i32,
    /// Paused by `pause` (`CBSound::m_SoundPaused`).
    script_paused: bool,
    freeze_paused: bool,
}

/// Sounds loaded and playing.
pub struct Audio {
    manager: Manager,
    voices: HashMap<SoundId, Voice>,
    next_id: u64,
    /// The three type volumes as the engine keeps them: 0 to 200 for 0
    /// to 100 %.
    volume_sfx: i32,
    volume_speech: i32,
    volume_music: i32,
    /// The master volume as the engine keeps it: a wave device volume,
    /// 0 to 0xFFFF.
    volume_master: u16,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("device", &self.device())
            .field("sounds", &self.voices.len())
            .finish()
    }
}

fn settings<B: kira::backend::Backend>(
    backend_settings: B::Settings,
    internal_buffer_size: usize,
) -> AudioManagerSettings<B> {
    AudioManagerSettings {
        capacities: Capacities::default(),
        main_track_builder: MainTrackBuilder::default().sound_capacity(256),
        internal_buffer_size,
        backend_settings,
    }
}

impl Audio {
    /// Opens the default output device when the crate is built with the
    /// `device` feature and a device is there; otherwise the silent
    /// backend driven by [`Audio::tick`].
    pub fn new() -> Audio {
        match Audio::with_device() {
            Ok(audio) => audio,
            Err(_) => Audio::null(),
        }
    }

    /// Opens the default output device, or fails.
    #[cfg(feature = "device")]
    pub fn with_device() -> Result<Audio> {
        use kira::backend::cpal::CpalBackendSettings;
        let manager = AudioManager::<kira::backend::cpal::CpalBackend>::new(settings(
            CpalBackendSettings::default(),
            128,
        ))
        .map_err(|e| Error::Device(e.to_string()))?;
        Ok(Audio::from_manager(Manager::Device(
            Box::new(manager),
            "default output device".to_string(),
        )))
    }

    /// Opens the default output device, or fails: this build has no
    /// device backend (the `device` feature is off).
    #[cfg(not(feature = "device"))]
    pub fn with_device() -> Result<Audio> {
        Err(Error::Device(
            "built without the device feature".to_string(),
        ))
    }

    /// An audio system without a device: sounds play silently, advanced
    /// by [`Audio::tick`].
    pub fn null() -> Audio {
        let manager = AudioManager::<MockBackend>::new(settings(
            MockBackendSettings {
                sample_rate: MOCK_RATE,
            },
            MOCK_STEP,
        ))
        .unwrap_or_else(|()| unreachable!("the mock backend cannot fail"));
        Audio::from_manager(Manager::Mock(Box::new(manager)))
    }

    fn from_manager(manager: Manager) -> Audio {
        Audio {
            manager,
            voices: HashMap::new(),
            next_id: 1,
            volume_sfx: 200,
            volume_speech: 200,
            volume_music: 200,
            volume_master: 0xFFFF,
        }
    }

    /// The output device, or `None` for the silent backend.
    pub fn device(&self) -> Option<&str> {
        match &self.manager {
            #[cfg(feature = "device")]
            Manager::Device(_, description) => Some(description),
            Manager::Mock(_) => None,
        }
    }

    /// Advances the sounds by `ms` on the silent backend; a device needs
    /// nothing.
    pub fn tick(&mut self, ms: u64) {
        match &mut self.manager {
            Manager::Mock(manager) => {
                let backend = manager.backend_mut();
                for _ in 0..ms {
                    backend.on_start_processing();
                    backend.process();
                }
            }
            #[cfg(feature = "device")]
            Manager::Device(..) => {}
        }
    }

    fn type_volume(&self, kind: SoundType) -> i32 {
        match kind {
            SoundType::Sfx => self.volume_sfx,
            SoundType::Speech => self.volume_speech,
            SoundType::Music => self.volume_music,
        }
    }

    /// The decibels of a voice: the volume DirectSound last accepted,
    /// with what its pan takes back.
    fn voice_decibels(voice: &Voice) -> Decibels {
        let (_, compensation) = directsound_pan(voice.pan);
        Decibels((voice.centibels as f32 / 100.0 + compensation).max(Decibels::SILENCE.0))
    }

    fn apply_master(&mut self) {
        let db = decibels(f32::from(self.volume_master) / 65535.0);
        match &mut self.manager {
            #[cfg(feature = "device")]
            Manager::Device(manager, _) => manager.main_track().set_volume(db, Tween::default()),
            Manager::Mock(manager) => manager.main_track().set_volume(db, Tween::default()),
        }
    }

    fn play_data(
        &mut self,
        data: StaticSoundData,
        track: Option<&mut EffectTrack>,
    ) -> Option<StaticSoundHandle> {
        if let Some(track) = track {
            return track.track.play(data).ok();
        }
        match &mut self.manager {
            #[cfg(feature = "device")]
            Manager::Device(manager, _) => manager.play(data).ok(),
            Manager::Mock(manager) => manager.play(data).ok(),
        }
    }

    /// A track carrying an effect, for a sound to play on.
    fn effect_track(&mut self, effect: SoundEffect) -> Option<EffectTrack> {
        let params = effect.params();
        let mut builder = TrackBuilder::new().volume(params.track_db);
        let mut delay_time = None;
        let handle = if let Some((delay, feedback, mix)) = params.delay {
            delay_time = Some(delay);
            EffectHandle::Delay(
                builder.add_effect(
                    DelayBuilder::new()
                        .delay_time(delay)
                        .feedback(feedback)
                        .mix(mix),
                ),
            )
        } else if let Some((feedback, damping, mix)) = params.reverb {
            EffectHandle::Reverb(
                builder.add_effect(
                    ReverbBuilder::new()
                        .feedback(feedback)
                        .damping(damping)
                        .mix(mix),
                ),
            )
        } else {
            return None;
        };
        let track = match &mut self.manager {
            #[cfg(feature = "device")]
            Manager::Device(manager, _) => manager.add_sub_track(builder).ok()?,
            Manager::Mock(manager) => manager.add_sub_track(builder).ok()?,
        };
        Some(EffectTrack {
            track,
            handle,
            delay: delay_time,
        })
    }

    /// Applies an effect to a sound, or removes it with `None`
    /// (`CBSound::ApplyFX`): a sound playing with an effect of the same
    /// kind takes the new parameters in place; otherwise it starts
    /// again where it was, on a track with the effect, keeping its
    /// paused state, as the engine pauses and resumes it around the
    /// change.
    pub fn set_effect(&mut self, id: SoundId, effect: Option<SoundEffect>) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        let previous = voice.effect;
        voice.effect = effect;
        let live = voice
            .handle
            .as_ref()
            .is_some_and(|h| h.state() != PlaybackState::Stopped);
        if !live || previous == effect {
            return true;
        }
        if let (Some(previous), Some(effect), Some(track)) =
            (previous, effect, voice.track.as_mut())
        {
            if previous.same_kind(effect) {
                let params = effect.params();
                let same_delay = params.delay.is_none_or(|(d, _, _)| Some(d) == track.delay);
                if same_delay {
                    track.track.set_volume(params.track_db, Tween::default());
                    match (&mut track.handle, params.delay, params.reverb) {
                        (EffectHandle::Delay(h), Some((_, feedback, mix)), _) => {
                            h.set_feedback(feedback, Tween::default());
                            h.set_mix(mix, Tween::default());
                        }
                        (EffectHandle::Reverb(h), _, Some((feedback, damping, mix))) => {
                            h.set_feedback(feedback, Tween::default());
                            h.set_damping(damping, Tween::default());
                            h.set_mix(mix, Tween::default());
                        }
                        _ => {}
                    }
                    return true;
                }
            }
        }
        // Another kind of effect: the sound starts again where it was.
        let position = self.position_ms(id);
        let held = self.voices.get(&id).is_some_and(|v| {
            v.handle.as_ref().is_some_and(|h| {
                matches!(
                    h.state(),
                    PlaybackState::Paused | PlaybackState::Pausing | PlaybackState::WaitingToResume
                )
            })
        });
        self.start(id, position);
        if held {
            if let Some(handle) = self.voices.get_mut(&id).and_then(|v| v.handle.as_mut()) {
                handle.pause(Tween::default());
            }
        }
        true
    }

    /// The effect applied to a sound.
    pub fn effect(&self, id: SoundId) -> Option<SoundEffect> {
        self.voices.get(&id).and_then(|v| v.effect)
    }

    /// Loads a decoded sound (`CBSoundMgr::AddSound`), stopped, with its
    /// own volume in percent. The engine streams it when its file is
    /// larger than 1 MB (a sprite frame's sound); see [`is_streamed`].
    pub fn add(&mut self, clip: Arc<Clip>, kind: SoundType, private_volume: i32) -> SoundId {
        self.add_voice(clip, kind, private_volume, false)
    }

    /// Loads a decoded sound the engine asks to stream (an object's
    /// sound, music, speech): streamed unless shorter than a streaming
    /// block; see [`is_streamed`].
    pub fn add_streamed(
        &mut self,
        clip: Arc<Clip>,
        kind: SoundType,
        private_volume: i32,
    ) -> SoundId {
        self.add_voice(clip, kind, private_volume, true)
    }

    fn add_voice(
        &mut self,
        clip: Arc<Clip>,
        kind: SoundType,
        private_volume: i32,
        streamed: bool,
    ) -> SoundId {
        let id = SoundId(self.next_id);
        self.next_id += 1;
        let streamed = is_streamed(&clip, streamed);
        self.voices.insert(
            id,
            Voice {
                clip,
                kind,
                handle: None,
                effect: None,
                track: None,
                looping: false,
                loop_start_ms: 0,
                streamed,
                start_ms: 0,
                cursor_ms: 0,
                private_volume,
                // A DirectSound buffer starts at full volume, in the
                // centre.
                centibels: 0,
                pan: 0,
                script_paused: false,
                freeze_paused: false,
            },
        );
        self.refresh_volume(id);
        id
    }

    /// Whether the engine streams the sound.
    pub fn is_streamed(&self, id: SoundId) -> bool {
        self.voices.get(&id).is_some_and(|v| v.streamed)
    }

    /// Unloads a sound.
    pub fn remove(&mut self, id: SoundId) -> bool {
        match self.voices.remove(&id) {
            Some(mut voice) => {
                if let Some(handle) = &mut voice.handle {
                    handle.stop(Tween::default());
                }
                true
            }
            None => false,
        }
    }

    /// Whether the sound is loaded.
    pub fn contains(&self, id: SoundId) -> bool {
        self.voices.contains_key(&id)
    }

    /// The number of sounds loaded.
    pub fn len(&self) -> usize {
        self.voices.len()
    }

    /// Whether no sound is loaded.
    pub fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }

    /// Whether a handle is in a playing state.
    fn handle_playing(handle: &StaticSoundHandle) -> bool {
        matches!(
            handle.state(),
            PlaybackState::Playing
                | PlaybackState::Pausing
                | PlaybackState::Resuming
                | PlaybackState::WaitingToResume
        )
    }

    /// Starts the voice at a position on a new Kira sound, stopping the
    /// one it had; the flags are the caller's business.
    fn start(&mut self, id: SoundId, at_ms: u64) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        if let Some(mut handle) = voice.handle.take() {
            handle.stop(Tween::default());
        }
        voice.track = None;
        voice.cursor_ms = 0;
        let (panning, _) = directsound_pan(voice.pan);
        // Past the end a buffer plays from its start.
        let at_ms = if at_ms >= voice.clip.duration_ms() {
            0
        } else {
            at_ms
        };
        let data = voice
            .clip
            .data
            .volume(Audio::voice_decibels(voice))
            .panning(panning)
            .start_position(at_ms as f64 / 1000.0)
            .loop_region(voice.loop_region());
        let effect = voice.effect;
        let mut track = effect.and_then(|e| self.effect_track(e));
        let handle = self.play_data(data, track.as_mut());
        if let Some(voice) = self.voices.get_mut(&id) {
            voice.handle = handle;
            voice.track = track;
            return voice.handle.is_some();
        }
        false
    }

    /// What `IDirectSoundBuffer::Play` does to a buffer
    /// (`CBSoundBuffer::Resume`): a held sound goes on, a playing one is
    /// left alone, any other starts where the buffer stands (its start
    /// after a stop or once finished).
    fn buffer_resume(&mut self, id: SoundId) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        let region = voice.loop_region();
        if let Some(handle) = &mut voice.handle {
            match handle.state() {
                PlaybackState::Paused | PlaybackState::Pausing | PlaybackState::WaitingToResume => {
                    handle.set_loop_region(region);
                    handle.resume(Tween::default());
                    return true;
                }
                PlaybackState::Playing | PlaybackState::Resuming => return true,
                PlaybackState::Stopping | PlaybackState::Stopped => {}
            }
        }
        let at = voice.cursor_ms;
        self.start(id, at)
    }

    /// Plays a sound (`CBSound::Play`, `CBSoundBuffer::Play`): always
    /// stopped and started again at its start position, whether it was
    /// playing, paused or stopped. The start position is the one
    /// [`Audio::set_position_ms`] left while the sound was not playing,
    /// and stays for every later play.
    pub fn play(&mut self, id: SoundId, looping: bool) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.script_paused = false;
        voice.looping = looping;
        let at = voice.start_ms;
        self.start(id, at)
    }

    /// Stops a sound (`CBSound::Stop`): the buffer goes back to its
    /// start. Neither the start position of the next play nor the flag
    /// of a sound held by [`Audio::pause_all`] is touched, as in the
    /// engine.
    pub fn stop(&mut self, id: SoundId) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.script_paused = false;
        if let Some(mut handle) = voice.handle.take() {
            handle.stop(Tween::default());
        }
        voice.track = None;
        voice.cursor_ms = 0;
        true
    }

    /// Pauses a sound (`CBSound::Pause`): it is marked paused even when
    /// it is not playing, so that a later [`Audio::resume`] starts it.
    pub fn pause(&mut self, id: SoundId) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.script_paused = true;
        if let Some(handle) = &mut voice.handle {
            if Audio::handle_playing(handle) {
                handle.pause(Tween::default());
            }
        }
        true
    }

    /// Resumes a sound paused by [`Audio::pause`] (`CBSound::Resume`);
    /// `false` when it was not: a sound held by [`Audio::pause_all`]
    /// alone stays held.
    pub fn resume(&mut self, id: SoundId) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        if !voice.script_paused {
            return false;
        }
        voice.script_paused = false;
        self.buffer_resume(id)
    }

    /// Whether a sound is playing (`CBSoundBuffer::IsPlaying`, the
    /// DirectSound status): `false` for a paused sound, one held by
    /// [`Audio::pause_all`] included.
    pub fn is_playing(&self, id: SoundId) -> bool {
        self.voices.get(&id).is_some_and(|v| {
            v.handle.as_ref().is_some_and(|h| {
                matches!(h.state(), PlaybackState::Playing | PlaybackState::Resuming)
            })
        })
    }

    /// Whether a sound was paused by [`Audio::pause`]
    /// (`CBSound::IsPaused`, the flag): `false` for a sound only held by
    /// [`Audio::pause_all`].
    pub fn is_paused(&self, id: SoundId) -> bool {
        self.voices.get(&id).is_some_and(|v| v.script_paused)
    }

    /// Whether a sound is started and not ended: playing or held in any
    /// way. Not a notion of the engine; for the callers that drop sounds
    /// once they ended.
    pub fn is_active(&self, id: SoundId) -> bool {
        self.voices.get(&id).is_some_and(|v| {
            v.handle
                .as_ref()
                .is_some_and(|h| !matches!(h.state(), PlaybackState::Stopped))
        })
    }

    /// The length of a sound in milliseconds.
    pub fn length_ms(&self, id: SoundId) -> u64 {
        self.voices.get(&id).map_or(0, |v| v.clip.duration_ms())
    }

    /// The position of a sound in milliseconds (0 when it is not
    /// started).
    pub fn position_ms(&self, id: SoundId) -> u64 {
        self.voices.get(&id).map_or(0, |v| {
            v.handle
                .as_ref()
                .filter(|h| h.state() != PlaybackState::Stopped)
                .map_or(0, |h| (h.position() * 1000.0) as u64)
        })
    }

    /// Moves a sound to a time (`CBSound::SetPositionTime`). A playing
    /// sound jumps there. One that is not playing takes the time as its
    /// start position, for the next [`Audio::play`] and every one after
    /// it (the engine never clears it; only a later call on the sound
    /// while it plays does).
    pub fn set_position_ms(&mut self, id: SoundId, ms: u64) -> bool {
        let playing = self.is_playing(id);
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        match &mut voice.handle {
            Some(handle) if handle.state() != PlaybackState::Stopped => {
                handle.seek_to(ms as f64 / 1000.0);
            }
            _ => voice.cursor_ms = ms,
        }
        voice.start_ms = if playing { 0 } else { ms };
        true
    }

    /// Where every [`Audio::play`] of the sound starts, in milliseconds.
    pub fn start_ms(&self, id: SoundId) -> u64 {
        self.voices.get(&id).map_or(0, |v| v.start_ms)
    }

    /// Sets where every [`Audio::play`] of the sound starts, without
    /// moving it (a saved game being loaded).
    pub fn set_start_ms(&mut self, id: SoundId, ms: u64) -> bool {
        match self.voices.get_mut(&id) {
            Some(voice) => {
                voice.start_ms = ms;
                true
            }
            None => false,
        }
    }

    /// Sets where a loop restarts (`CBSound::SetLoopStart`), in
    /// milliseconds. Only a streamed sound uses it; another loops whole.
    pub fn set_loop_start_ms(&mut self, id: SoundId, ms: u64) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.loop_start_ms = ms;
        let region = voice.loop_region();
        if let Some(handle) = &mut voice.handle {
            handle.set_loop_region(region);
        }
        true
    }

    /// Sets whether a sound loops.
    pub fn set_looping(&mut self, id: SoundId, looping: bool) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.looping = looping;
        let region = voice.loop_region();
        if let Some(handle) = &mut voice.handle {
            handle.set_loop_region(region);
        }
        true
    }

    /// The sound's own volume in percent.
    pub fn private_volume(&self, id: SoundId) -> i32 {
        self.voices.get(&id).map_or(0, |v| v.private_volume)
    }

    /// Whether the sound loops, as its last `play` asked.
    pub fn is_looping(&self, id: SoundId) -> bool {
        self.voices.get(&id).is_some_and(|v| v.looping)
    }

    /// Where the sound's loop restarts, in milliseconds.
    pub fn loop_start_ms(&self, id: SoundId) -> u64 {
        self.voices.get(&id).map_or(0, |v| v.loop_start_ms)
    }

    /// Whether the sound is held by [`Audio::pause_all`].
    pub fn is_freeze_paused(&self, id: SoundId) -> bool {
        self.voices.get(&id).is_some_and(|v| v.freeze_paused)
    }

    /// Stops and unloads every sound (a saved game being loaded).
    pub fn clear(&mut self) {
        let ids: Vec<SoundId> = self.voices.keys().copied().collect();
        for id in ids {
            self.stop(id);
            self.remove(id);
        }
    }

    /// Sets the sound's own volume in percent (`SetPrivateVolume`). Out
    /// of range (above 100 with a full type volume, below 0) DirectSound
    /// refuses the result and the sound keeps the loudness it had; the
    /// value is kept and read back all the same.
    pub fn set_private_volume(&mut self, id: SoundId, percent: i32) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.private_volume = percent;
        self.refresh_volume(id);
        true
    }

    /// `CBSoundBuffer::SetVolume` with the type's volume: the loudness
    /// changes when DirectSound accepts the value.
    fn refresh_volume(&mut self, id: SoundId) {
        let Some(voice) = self.voices.get(&id) else {
            return;
        };
        let Some(centibels) = attenuation(self.type_volume(voice.kind), voice.private_volume)
        else {
            return;
        };
        let Some(voice) = self.voices.get_mut(&id) else {
            return;
        };
        voice.centibels = centibels;
        let db = Audio::voice_decibels(voice);
        if let Some(handle) = &mut voice.handle {
            handle.set_volume(db, Tween::default());
        }
    }

    /// The sound's volume in hundredths of a decibel, as DirectSound
    /// last accepted it.
    pub fn centibels(&self, id: SoundId) -> i32 {
        self.voices.get(&id).map_or(0, |v| v.centibels)
    }

    /// Sets the pan (`CBSoundBuffer::SetPan`) in DirectSound's unit:
    /// hundredths of a decibel taken off the left channel when positive,
    /// off the right one when negative, 0 the centre. `false`, the pan
    /// unchanged, outside [`PAN_LEFT`]..=[`PAN_RIGHT`], as DirectSound
    /// refuses it.
    pub fn set_pan(&mut self, id: SoundId, pan: i32) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        if !(PAN_LEFT..=PAN_RIGHT).contains(&pan) {
            return false;
        }
        if voice.pan == pan {
            return true;
        }
        voice.pan = pan;
        let (panning, _) = directsound_pan(pan);
        let db = Audio::voice_decibels(voice);
        if let Some(handle) = &mut voice.handle {
            handle.set_panning(panning, Tween::default());
            handle.set_volume(db, Tween::default());
        }
        true
    }

    /// The sound's pan, in hundredths of a decibel.
    pub fn pan(&self, id: SoundId) -> i32 {
        self.voices.get(&id).map_or(0, |v| v.pan)
    }

    /// The global volume of a sound type in percent
    /// (`GetVolumePercent`): a byte.
    pub fn volume(&self, kind: SoundType) -> i32 {
        i32::from((self.type_volume(kind) / 2) as u8)
    }

    /// Sets the global volume of a sound type (`SetVolumePercent`). The
    /// engine takes a byte and does not clamp it: 150 is kept and read
    /// back as 150, -1 is 255; above 100 DirectSound refuses the
    /// resulting volume and the sounds keep their loudness.
    pub fn set_volume(&mut self, kind: SoundType, percent: i32) {
        self.set_raw_volume(kind, 2 * i32::from(percent as u8));
    }

    /// The global volume of a sound type as the engine keeps and saves
    /// it (`m_VolumeSFX`...): 200 for 100 %.
    pub fn raw_volume(&self, kind: SoundType) -> i32 {
        self.type_volume(kind)
    }

    /// Sets the global volume of a sound type in the engine's own unit
    /// (`CBSoundMgr::SetVolume`).
    pub fn set_raw_volume(&mut self, kind: SoundType, volume: i32) {
        match kind {
            SoundType::Sfx => self.volume_sfx = volume,
            SoundType::Speech => self.volume_speech = volume,
            SoundType::Music => self.volume_music = volume,
        }
        let ids: Vec<SoundId> = self
            .voices
            .iter()
            .filter(|(_, v)| v.kind == kind)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.refresh_volume(id);
        }
    }

    /// The master volume in percent (`GetMasterVolumePercent`).
    pub fn master_volume(&self) -> i32 {
        i32::from((self.volume_master / (0xFFFF / 100)) as u8)
    }

    /// Sets the master volume (`SetMasterVolumePercent`). The engine
    /// takes a byte and keeps the low 16 bits of 655 times it: 100 is
    /// 65500, and a value above 100 wraps (150 gives 32714, read back as
    /// 49 and heard at half volume).
    pub fn set_master_volume(&mut self, percent: i32) {
        let volume = (0xFFFF / 100) * u32::from(percent as u8);
        self.set_raw_master_volume(volume as u16);
    }

    /// The master volume as the engine keeps it (one channel of
    /// `m_VolumeWave`): 0 to 0xFFFF.
    pub fn raw_master_volume(&self) -> u16 {
        self.volume_master
    }

    /// Sets the master volume in the engine's own unit
    /// (`CBSoundMgr::SetVolumeWave`).
    pub fn set_raw_master_volume(&mut self, volume: u16) {
        self.volume_master = volume;
        self.apply_master();
    }

    /// Pauses every playing sound, music too when asked
    /// (`CBSoundMgr::PauseAll`), to be resumed by [`Audio::resume_all`].
    pub fn pause_all(&mut self, including_music: bool) {
        for voice in self.voices.values_mut() {
            if voice.kind == SoundType::Music && !including_music {
                continue;
            }
            if let Some(handle) = &mut voice.handle {
                if Audio::handle_playing(handle) {
                    handle.pause(Tween::default());
                    voice.freeze_paused = true;
                }
            }
        }
    }

    /// Resumes the sounds [`Audio::pause_all`] paused
    /// (`CBSoundMgr::ResumeAll`). A sound stopped in between kept its
    /// flag ([`Audio::stop`]) and starts again from its beginning, as in
    /// the engine.
    pub fn resume_all(&mut self) {
        let held: Vec<SoundId> = self
            .voices
            .iter()
            .filter(|(_, v)| v.freeze_paused)
            .map(|(id, _)| *id)
            .collect();
        for id in held {
            self.buffer_resume(id);
            if let Some(voice) = self.voices.get_mut(&id) {
                voice.freeze_paused = false;
            }
        }
    }
}

impl Voice {
    /// The loop region of the sound: none when it does not loop; from
    /// the loop start to the end for a streamed sound
    /// (`LoadSegment`); the whole sound otherwise (a DirectSound buffer
    /// played with `DSBPLAY_LOOPING`). A loop start past the end is the
    /// start. An empty sound does not loop, and the loop always spans at
    /// least one frame: the mixer would otherwise never leave it.
    fn loop_region(&self) -> Option<kira::sound::Region> {
        let frames = self.clip.frames() as u64;
        if !self.looping || frames == 0 {
            return None;
        }
        let start = if self.streamed {
            // The frame the loop start falls on, rounded up so that it
            // is never before the frame the mixer rounds to.
            let frame = (u128::from(self.loop_start_ms) * u128::from(self.clip.sample_rate()))
                .div_ceil(1000);
            if frame < u128::from(frames) {
                self.loop_start_ms
            } else {
                0
            }
        } else {
            0
        };
        Some((start as f64 / 1000.0..).into())
    }
}

impl Default for Audio {
    fn default() -> Self {
        Audio::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_follows_the_engine_curve() {
        assert!((gain(100.0) - 1.0).abs() < 1e-6);
        assert!((gain(0.0) - 1e-5).abs() < 1e-7);
        let half = gain(50.0);
        assert!(half > 0.23 && half < 0.24, "{half}");
        assert!(gain(75.0) > half && gain(75.0) < 1.0);
        assert_eq!(decibels(1.0), Decibels::IDENTITY);
        assert_eq!(decibels(0.0), Decibels::SILENCE);
        assert!((decibels(0.5).0 + 6.02).abs() < 0.01);
    }

    #[test]
    fn effects_play_on_their_own_tracks() {
        let mut audio = Audio::null();
        let clip = Arc::new(Clip::silence(1000, 500));
        let id = audio.add(clip, SoundType::Sfx, 100);
        let echo = SoundEffect::Echo {
            wet_dry_mix: 50.0,
            feedback: 50.0,
            left_delay_ms: 300.0,
            right_delay_ms: 400.0,
        };
        assert!(audio.set_effect(id, Some(echo)));
        assert_eq!(audio.effect(id), Some(echo));
        assert!(audio.play(id, false));
        audio.tick(100);
        assert!(audio.is_playing(id));
        assert!(audio.voices[&id].track.is_some());
        let at = audio.position_ms(id);
        assert!((95..=105).contains(&at), "{at}");
        // The same kind of effect changes in place.
        let louder = SoundEffect::Echo {
            wet_dry_mix: 80.0,
            feedback: 20.0,
            left_delay_ms: 300.0,
            right_delay_ms: 400.0,
        };
        assert!(audio.set_effect(id, Some(louder)));
        audio.tick(50);
        let at = audio.position_ms(id);
        assert!((145..=155).contains(&at), "{at}");
        // Another kind restarts the sound where it was, on a new track.
        let reverb = SoundEffect::Reverb {
            in_gain_db: -6.0,
            reverb_mix_db: -3.0,
            reverb_time_ms: 1000.0,
            high_freq_rt_ratio: 0.5,
        };
        assert!(audio.set_effect(id, Some(reverb)));
        assert!(audio.is_playing(id));
        audio.tick(50);
        let at = audio.position_ms(id);
        assert!((190..=210).contains(&at), "{at}");
        // Paused sounds stay paused through the change; removing the
        // effect plays on the main track.
        assert!(audio.pause(id));
        audio.tick(10);
        assert!(audio.set_effect(id, None));
        audio.tick(10);
        assert!(audio.is_paused(id));
        assert!(audio.voices[&id].track.is_none());
        assert!(audio.resume(id));
        audio.tick(50);
        assert!(audio.is_playing(id));
        // The parameter mapping.
        let params = echo.params();
        let (delay, feedback, mix) = params.delay.unwrap();
        assert_eq!(delay, Duration::from_secs_f32(0.35));
        assert!((feedback.0 + 6.02).abs() < 0.01);
        assert_eq!(mix, Mix(0.5));
        let params = reverb.params();
        let (feedback, damping, mix) = params.reverb.unwrap();
        assert!((feedback - 0.785).abs() < 0.01, "{feedback}");
        assert!((damping - 0.5).abs() < 1e-6);
        assert!((mix.0 - 0.414).abs() < 0.01, "{}", mix.0);
        assert_eq!(params.track_db, Decibels(-6.0));
    }

    #[test]
    fn null_backend_plays_by_time() {
        let mut audio = Audio::null();
        assert!(audio.device().is_none());
        let clip = Arc::new(Clip::silence(1000, 500));
        let id = audio.add(clip, SoundType::Speech, 80);
        assert_eq!(audio.length_ms(id), 500);
        assert!(!audio.is_playing(id));
        assert!(audio.play(id, false));
        audio.tick(1);
        assert!(audio.is_playing(id));
        audio.tick(200);
        let at = audio.position_ms(id);
        assert!((195..=205).contains(&at), "{at}");
        assert!(audio.pause(id));
        audio.tick(20);
        assert!(!audio.is_playing(id));
        assert!(audio.is_paused(id));
        audio.tick(200);
        let held = audio.position_ms(id);
        assert!((at - 5..=at + 25).contains(&held), "{held} vs {at}");
        assert!(audio.resume(id));
        audio.tick(400);
        assert!(!audio.is_playing(id), "finished");
        assert!(audio.play(id, true));
        audio.tick(1200);
        assert!(audio.is_playing(id), "looping");
        assert_eq!(audio.private_volume(id), 80);
        audio.set_volume(SoundType::Speech, 50);
        assert_eq!(audio.volume(SoundType::Speech), 50);
        assert!(audio.stop(id));
        audio.tick(20);
        assert_eq!(audio.position_ms(id), 0);
        assert!(!audio.is_playing(id));
        assert!(audio.remove(id));
        assert!(!audio.contains(id));
        assert!(audio.is_empty());
    }

    fn near(at: u64, want: u64) -> bool {
        (want.saturating_sub(6)..=want + 6).contains(&at)
    }

    /// `CBSound::Play`: always from the start position, whatever the
    /// sound was doing; the position set while it was not playing is
    /// used by every later play.
    #[test]
    fn play_always_restarts_at_the_start_position() {
        let mut audio = Audio::null();
        let id = audio.add(Arc::new(Clip::silence(1000, 1000)), SoundType::Sfx, 100);
        assert!(audio.play(id, false));
        audio.tick(200);
        assert!(near(audio.position_ms(id), 200));
        // A playing sound starts again.
        assert!(audio.play(id, false));
        audio.tick(50);
        assert!(near(audio.position_ms(id), 50), "{}", audio.position_ms(id));
        // So does a paused one, and it is no longer paused.
        assert!(audio.pause(id));
        audio.tick(20);
        assert!(audio.is_paused(id));
        assert!(audio.play(id, false));
        audio.tick(30);
        assert!(!audio.is_paused(id));
        assert!(audio.is_playing(id));
        assert!(near(audio.position_ms(id), 30), "{}", audio.position_ms(id));
        // A position set while stopped is where every play starts.
        assert!(audio.stop(id));
        audio.tick(20);
        assert!(audio.set_position_ms(id, 300));
        assert_eq!(audio.start_ms(id), 300);
        for _ in 0..2 {
            assert!(audio.play(id, false));
            audio.tick(40);
            assert!(
                near(audio.position_ms(id), 340),
                "{}",
                audio.position_ms(id)
            );
            assert!(audio.stop(id));
            audio.tick(20);
        }
        // Set while playing, the sound jumps and the start position is
        // cleared.
        assert!(audio.play(id, false));
        audio.tick(20);
        assert!(audio.set_position_ms(id, 600));
        audio.tick(40);
        assert!(
            near(audio.position_ms(id), 640),
            "{}",
            audio.position_ms(id)
        );
        assert_eq!(audio.start_ms(id), 0);
        assert!(audio.play(id, false));
        audio.tick(40);
        assert!(near(audio.position_ms(id), 40), "{}", audio.position_ms(id));
    }

    /// `CBSound::Pause` marks a sound that is not playing, and
    /// `CBSound::Resume` only acts on a marked sound.
    #[test]
    fn pause_and_resume_follow_the_flag() {
        let mut audio = Audio::null();
        let id = audio.add(Arc::new(Clip::silence(1000, 500)), SoundType::Sfx, 100);
        // Not paused: nothing to resume.
        assert!(!audio.resume(id));
        assert!(!audio.is_playing(id));
        // Paused while stopped, then resumed: it starts from its start.
        assert!(audio.pause(id));
        assert!(audio.is_paused(id));
        assert!(audio.resume(id));
        audio.tick(40);
        assert!(audio.is_playing(id));
        assert!(!audio.is_paused(id));
        assert!(near(audio.position_ms(id), 40), "{}", audio.position_ms(id));
        // A sound the freeze holds is not resumed by `resume`.
        audio.pause_all(true);
        audio.tick(20);
        assert!(audio.is_freeze_paused(id));
        assert!(!audio.resume(id));
        audio.tick(100);
        assert!(!audio.is_playing(id));
        assert!(audio.is_freeze_paused(id));
        let held = audio.position_ms(id);
        audio.resume_all();
        audio.tick(40);
        assert!(audio.is_playing(id));
        assert!(near(audio.position_ms(id), held + 40));
        // Paused by a script first, the freeze does not hold it and the
        // unfreeze leaves it paused.
        assert!(audio.pause(id));
        audio.tick(20);
        audio.pause_all(true);
        assert!(!audio.is_freeze_paused(id));
        audio.resume_all();
        audio.tick(20);
        assert!(!audio.is_playing(id));
        assert!(audio.is_paused(id));
    }

    /// `CBSoundBuffer::IsPlaying` is the device's status: a sound the
    /// freeze holds is not playing, and is not paused either
    /// (`CBSound::IsPaused` is the script's flag). A sound stopped while
    /// held starts again at the unfreeze (`CBSoundMgr::ResumeAll`).
    #[test]
    fn a_frozen_sound_is_neither_playing_nor_paused() {
        let mut audio = Audio::null();
        let id = audio.add(Arc::new(Clip::silence(1000, 500)), SoundType::Sfx, 100);
        let music = audio.add(Arc::new(Clip::silence(1000, 500)), SoundType::Music, 100);
        assert!(audio.play(id, false));
        assert!(audio.play(music, true));
        audio.tick(100);
        audio.pause_all(false);
        audio.tick(20);
        assert!(!audio.is_playing(id));
        assert!(!audio.is_paused(id));
        assert!(audio.is_active(id));
        assert!(audio.is_freeze_paused(id));
        assert!(audio.is_playing(music), "the music was left out");
        // Stopped during the freeze: the flag stays.
        assert!(audio.stop(id));
        audio.tick(20);
        assert!(!audio.is_active(id));
        assert!(audio.is_freeze_paused(id));
        audio.resume_all();
        audio.tick(40);
        assert!(audio.is_playing(id));
        assert!(!audio.is_freeze_paused(id));
        assert!(near(audio.position_ms(id), 40), "{}", audio.position_ms(id));
    }

    /// `CBSound::SetLoopStart` acts on streamed sounds only: those the
    /// engine asked to stream (or files over 1 MB) that are at least a
    /// streaming block long.
    #[test]
    fn only_streamed_sounds_loop_from_the_loop_start() {
        let long = Arc::new(Clip::silence(1000, 400));
        let short = Arc::new(Clip::silence(1000, 100));
        assert!(is_streamed(&long, true));
        assert!(!is_streamed(&long, false));
        assert!(!is_streamed(&short, true), "under 3 s / 16");
        assert!(is_streamed(&Clip::silence(1000, 187), true));
        assert!(!is_streamed(&Clip::silence(1000, 186), true));
        // A loop start on the last frame or after it, and an empty
        // sound, give no loop the mixer could not leave.
        let mut audio = Audio::null();
        let slow = audio.add_streamed(Arc::new(Clip::silence(1, 3000)), SoundType::Sfx, 100);
        assert!(audio.is_streamed(slow));
        assert!(audio.set_loop_start_ms(slow, 2999));
        assert!(audio.play(slow, true));
        let empty = audio.add_streamed(Arc::new(Clip::silence(1000, 0)), SoundType::Sfx, 100);
        assert!(audio.play(empty, true));
        audio.tick(50);
        assert!(audio.voices[&slow].loop_region().is_some());
        assert!(audio.voices[&empty].loop_region().is_none());
        let big = Clip::silence(1000, 400).with_file_size(1024 * 1024 + 1);
        assert!(is_streamed(&big, false));
        assert!(!is_streamed(
            &Clip::silence(1000, 400).with_file_size(1024 * 1024),
            false
        ));

        let mut audio = Audio::null();
        let streamed = audio.add_streamed(long.clone(), SoundType::Sfx, 100);
        let whole = audio.add(long, SoundType::Sfx, 100);
        assert!(audio.is_streamed(streamed));
        assert!(!audio.is_streamed(whole));
        for id in [streamed, whole] {
            assert!(audio.set_loop_start_ms(id, 300));
            assert!(audio.play(id, true));
        }
        audio.tick(450);
        assert!(
            near(audio.position_ms(streamed), 350),
            "{}",
            audio.position_ms(streamed)
        );
        assert!(
            near(audio.position_ms(whole), 50),
            "{}",
            audio.position_ms(whole)
        );
    }

    /// `CBSoundBuffer::SetVolume` and `CBSoundMgr::SetVolumePercent`:
    /// the engine's arithmetic, bytes that are not clamped, and values
    /// DirectSound refuses leaving the loudness as it was.
    #[test]
    fn volumes_follow_the_engine_arithmetic() {
        assert_eq!(attenuation(200, 100), Some(0));
        assert_eq!(attenuation(200, 0), Some(-10000));
        assert_eq!(attenuation(200, 50), Some(-1250));
        assert_eq!(attenuation(100, 50), Some(-4218));
        // 2 x 33 % truncated to 66, not 66.66.
        assert_eq!(attenuation(200, 33), attenuation(66, 100));
        assert_eq!(attenuation(200, 150), None);
        assert_eq!(attenuation(200, -10), None);
        assert_eq!(attenuation(300, 100), None);

        let mut audio = Audio::null();
        let id = audio.add(Arc::new(Clip::silence(1000, 500)), SoundType::Sfx, 50);
        assert_eq!(audio.centibels(id), -1250);
        // An own volume out of range is kept, the loudness is not
        // changed.
        assert!(audio.set_private_volume(id, 150));
        assert_eq!(audio.private_volume(id), 150);
        assert_eq!(audio.centibels(id), -1250);
        assert!(audio.set_private_volume(id, 100));
        assert_eq!(audio.centibels(id), 0);
        // A type volume is a byte, not clamped.
        audio.set_volume(SoundType::Sfx, 50);
        assert_eq!(audio.volume(SoundType::Sfx), 50);
        assert_eq!(audio.raw_volume(SoundType::Sfx), 100);
        assert_eq!(audio.centibels(id), -1250);
        audio.set_volume(SoundType::Sfx, 150);
        assert_eq!(audio.volume(SoundType::Sfx), 150);
        assert_eq!(audio.centibels(id), -1250, "refused: the loudness stays");
        audio.set_volume(SoundType::Sfx, -1);
        assert_eq!(audio.volume(SoundType::Sfx), 255);
        audio.set_volume(SoundType::Sfx, 256);
        assert_eq!(audio.volume(SoundType::Sfx), 0);
        assert_eq!(audio.centibels(id), -10000);
        assert_eq!(
            audio.volume(SoundType::Music),
            100,
            "the other types are untouched"
        );
        // The master volume keeps 16 bits of 655 x the byte.
        assert_eq!(audio.master_volume(), 100);
        audio.set_master_volume(100);
        assert_eq!(audio.raw_master_volume(), 65500);
        assert_eq!(audio.master_volume(), 100);
        audio.set_master_volume(40);
        assert_eq!(audio.master_volume(), 40);
        audio.set_master_volume(150);
        assert_eq!(audio.raw_master_volume(), 32714);
        assert_eq!(audio.master_volume(), 49);
        // A sound of a failed first volume starts at full volume, as a
        // new DirectSound buffer does.
        let loud = audio.add(Arc::new(Clip::silence(1000, 500)), SoundType::Music, 130);
        assert_eq!(audio.centibels(loud), 0);
    }

    /// `CBSoundBuffer::SetPan`: DirectSound's unit and range, the far
    /// channel attenuated and the near one left as it is.
    #[test]
    fn pan_is_directsound_s() {
        let mut audio = Audio::null();
        let id = audio.add(Arc::new(Clip::silence(1000, 500)), SoundType::Sfx, 100);
        assert_eq!(audio.pan(id), 0);
        assert!(audio.set_pan(id, -909));
        assert_eq!(audio.pan(id), -909);
        assert!(!audio.set_pan(id, 10001), "DirectSound refuses it");
        assert!(!audio.set_pan(id, -20000));
        assert_eq!(audio.pan(id), -909);
        assert!(audio.set_pan(id, PAN_RIGHT));
        // The channel gains of Kira's panning law with the compensation.
        for pan in [0, 909, -909, 3000, -10000] {
            let (panning, compensation) = directsound_pan(pan);
            let mix = (panning.0 + 1.0) * 0.5;
            let scale = 10f32.powf(compensation / 20.0) * std::f32::consts::SQRT_2;
            let (left, right) = ((1.0 - mix).sqrt() * scale, mix.sqrt() * scale);
            let (near, far) = if pan < 0 {
                (left, right)
            } else {
                (right, left)
            };
            assert!((near - 1.0).abs() < 1e-4, "{pan}: near {near}");
            let want = 10f32.powf(-(pan.abs() as f32) / 2000.0);
            assert!((far - want).abs() < 1e-4, "{pan}: far {far} vs {want}");
        }
    }
}
