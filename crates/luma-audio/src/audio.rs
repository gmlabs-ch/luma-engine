//! The audio system: Kira mixing the sounds on the default output device,
//! or on a mock backend that the game clock drives when no device is
//! there (continuous integration, headless runs, the `device` feature
//! off).
//!
//! The volume rules follow the original engine (`BSoundMgr.cpp`,
//! `BSoundBuffer.cpp` and `BSound.cpp` of WME Lite, MIT License,
//! Copyright (c) 2011 Jan Nedoma); see `NOTICE`.

use std::collections::HashMap;
use std::sync::Arc;

use kira::backend::mock::{MockBackend, MockBackendSettings};
use kira::sound::PlaybackState;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::MainTrackBuilder;
use kira::{AudioManager, AudioManagerSettings, Capacities, Decibels, Panning, Tween};

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

/// The gain of a volume in percent, as the engine attenuates it: the
/// cube of the remaining fraction, in hundredths of a decibel, turned
/// back into a factor.
pub fn gain(volume_percent: f32) -> f32 {
    let remaining = 1.0 - (volume_percent / 100.0).clamp(0.0, 1.0);
    let attenuation = -10000.0 * remaining.powi(3);
    10f32.powf(attenuation / 2000.0)
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
    looping: bool,
    loop_start_ms: u64,
    /// Where the next play starts.
    start_ms: u64,
    private_volume: i32,
    pan: f32,
    freeze_paused: bool,
}

/// Sounds loaded and playing.
pub struct Audio {
    manager: Manager,
    voices: HashMap<SoundId, Voice>,
    next_id: u64,
    volume_sfx: i32,
    volume_speech: i32,
    volume_music: i32,
    volume_master: i32,
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
            volume_sfx: 100,
            volume_speech: 100,
            volume_music: 100,
            volume_master: 100,
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

    /// The decibels of a voice: its type's volume weighted by its own,
    /// through the engine's curve.
    fn voice_decibels(&self, voice: &Voice) -> Decibels {
        let weighted = self.type_volume(voice.kind) as f32 / 100.0 * voice.private_volume as f32;
        decibels(gain(weighted))
    }

    fn apply_master(&mut self) {
        let db = decibels((self.volume_master as f32 / 100.0).clamp(0.0, 1.0));
        match &mut self.manager {
            #[cfg(feature = "device")]
            Manager::Device(manager, _) => manager.main_track().set_volume(db, Tween::default()),
            Manager::Mock(manager) => manager.main_track().set_volume(db, Tween::default()),
        }
    }

    fn play_data(&mut self, data: StaticSoundData) -> Option<StaticSoundHandle> {
        match &mut self.manager {
            #[cfg(feature = "device")]
            Manager::Device(manager, _) => manager.play(data).ok(),
            Manager::Mock(manager) => manager.play(data).ok(),
        }
    }

    /// Loads a decoded sound (`CBSoundMgr::AddSound`), stopped, with its
    /// own volume in percent.
    pub fn add(&mut self, clip: Arc<Clip>, kind: SoundType, private_volume: i32) -> SoundId {
        let id = SoundId(self.next_id);
        self.next_id += 1;
        self.voices.insert(
            id,
            Voice {
                clip,
                kind,
                handle: None,
                looping: false,
                loop_start_ms: 0,
                start_ms: 0,
                private_volume,
                pan: 0.0,
                freeze_paused: false,
            },
        );
        id
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

    /// Starts a sound from its start position (`Play`); a paused sound
    /// resumes, a playing one goes on.
    pub fn play(&mut self, id: SoundId, looping: bool) -> bool {
        let Some(voice) = self.voices.get(&id) else {
            return false;
        };
        let db = self.voice_decibels(voice);
        let voice = self.voices.get_mut(&id).expect("checked");
        voice.looping = looping;
        voice.freeze_paused = false;
        if let Some(handle) = &mut voice.handle {
            match handle.state() {
                PlaybackState::Paused | PlaybackState::Pausing | PlaybackState::WaitingToResume => {
                    handle.set_loop_region(loop_region(looping, voice.loop_start_ms));
                    handle.resume(Tween::default());
                    return true;
                }
                PlaybackState::Playing | PlaybackState::Resuming => {
                    handle.set_loop_region(loop_region(looping, voice.loop_start_ms));
                    return true;
                }
                PlaybackState::Stopping | PlaybackState::Stopped => {}
            }
        }
        let start = std::mem::take(&mut voice.start_ms);
        let data = voice
            .clip
            .data
            .volume(db)
            .panning(Panning(voice.pan))
            .start_position(start as f64 / 1000.0)
            .loop_region(loop_region(looping, voice.loop_start_ms));
        let handle = self.play_data(data);
        if let Some(voice) = self.voices.get_mut(&id) {
            voice.handle = handle;
            return voice.handle.is_some();
        }
        false
    }

    /// Stops a sound (`Stop`); it will play from its start again.
    pub fn stop(&mut self, id: SoundId) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        if let Some(mut handle) = voice.handle.take() {
            handle.stop(Tween::default());
        }
        voice.freeze_paused = false;
        voice.start_ms = 0;
        true
    }

    /// Pauses a sound (`Pause`).
    pub fn pause(&mut self, id: SoundId) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        if let Some(handle) = &mut voice.handle {
            if Audio::handle_playing(handle) {
                handle.pause(Tween::default());
            }
        }
        true
    }

    /// Resumes a paused sound (`Resume`).
    pub fn resume(&mut self, id: SoundId) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.freeze_paused = false;
        if let Some(handle) = &mut voice.handle {
            if matches!(
                handle.state(),
                PlaybackState::Paused | PlaybackState::Pausing
            ) {
                handle.resume(Tween::default());
            }
        }
        true
    }

    /// Whether a sound is playing: started and not finished
    /// (`CBSoundBuffer::IsPlaying` counts a sound frozen by
    /// [`Audio::pause_all`] as playing).
    pub fn is_playing(&self, id: SoundId) -> bool {
        self.voices.get(&id).is_some_and(|v| {
            v.freeze_paused
                || v.handle.as_ref().is_some_and(|h| {
                    matches!(h.state(), PlaybackState::Playing | PlaybackState::Resuming)
                })
        })
    }

    /// Whether a sound is paused.
    pub fn is_paused(&self, id: SoundId) -> bool {
        self.voices.get(&id).is_some_and(|v| {
            v.handle.as_ref().is_some_and(|h| {
                matches!(
                    h.state(),
                    PlaybackState::Paused | PlaybackState::Pausing | PlaybackState::WaitingToResume
                )
            })
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

    /// Moves a sound to a time (`SetPosition`): at once when it plays,
    /// else for its next start.
    pub fn set_position_ms(&mut self, id: SoundId, ms: u64) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        match &mut voice.handle {
            Some(handle) if handle.state() != PlaybackState::Stopped => {
                handle.seek_to(ms as f64 / 1000.0);
            }
            _ => voice.start_ms = ms,
        }
        true
    }

    /// Sets where a loop restarts (`SetLoopStart`), in milliseconds.
    pub fn set_loop_start_ms(&mut self, id: SoundId, ms: u64) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.loop_start_ms = ms;
        if let Some(handle) = &mut voice.handle {
            handle.set_loop_region(loop_region(voice.looping, ms));
        }
        true
    }

    /// Sets whether a sound loops.
    pub fn set_looping(&mut self, id: SoundId, looping: bool) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.looping = looping;
        if let Some(handle) = &mut voice.handle {
            handle.set_loop_region(loop_region(looping, voice.loop_start_ms));
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

    /// Sets the sound's own volume in percent (`SetPrivateVolume`).
    pub fn set_private_volume(&mut self, id: SoundId, percent: i32) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.private_volume = percent;
        self.refresh_volume(id);
        true
    }

    fn refresh_volume(&mut self, id: SoundId) {
        let Some(voice) = self.voices.get(&id) else {
            return;
        };
        let db = self.voice_decibels(voice);
        if let Some(handle) = self.voices.get_mut(&id).and_then(|v| v.handle.as_mut()) {
            handle.set_volume(db, Tween::default());
        }
    }

    /// Sets the pan, `-1` left to `1` right (`SetPan`).
    pub fn set_pan(&mut self, id: SoundId, pan: f32) -> bool {
        let Some(voice) = self.voices.get_mut(&id) else {
            return false;
        };
        voice.pan = pan.clamp(-1.0, 1.0);
        if let Some(handle) = &mut voice.handle {
            handle.set_panning(Panning(voice.pan), Tween::default());
        }
        true
    }

    /// The global volume of a sound type, in percent.
    pub fn volume(&self, kind: SoundType) -> i32 {
        self.type_volume(kind)
    }

    /// Sets the global volume of a sound type (`SetVolumePercent`).
    pub fn set_volume(&mut self, kind: SoundType, percent: i32) {
        let percent = percent.clamp(0, 100);
        match kind {
            SoundType::Sfx => self.volume_sfx = percent,
            SoundType::Speech => self.volume_speech = percent,
            SoundType::Music => self.volume_music = percent,
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

    /// The master volume in percent.
    pub fn master_volume(&self) -> i32 {
        self.volume_master
    }

    /// Sets the master volume (`SetMasterVolumePercent`).
    pub fn set_master_volume(&mut self, percent: i32) {
        self.volume_master = percent.clamp(0, 100);
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

    /// Resumes the sounds [`Audio::pause_all`] paused.
    pub fn resume_all(&mut self) {
        for voice in self.voices.values_mut() {
            if voice.freeze_paused {
                voice.freeze_paused = false;
                if let Some(handle) = &mut voice.handle {
                    handle.resume(Tween::default());
                }
            }
        }
    }
}

/// The loop region of a sound: from the loop start to the end when it
/// loops, none otherwise.
fn loop_region(looping: bool, loop_start_ms: u64) -> Option<kira::sound::Region> {
    looping.then(|| (loop_start_ms as f64 / 1000.0..).into())
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
        audio.set_master_volume(150);
        assert_eq!(audio.master_volume(), 100);
        audio.pause_all(false);
        audio.tick(20);
        assert!(audio.is_playing(id), "a frozen sound counts as playing");
        assert!(audio.is_paused(id));
        audio.resume_all();
        audio.tick(20);
        assert!(!audio.is_paused(id));
        assert!(audio.stop(id));
        audio.tick(20);
        assert_eq!(audio.position_ms(id), 0);
        assert!(!audio.is_playing(id));
        assert!(audio.remove(id));
        assert!(!audio.contains(id));
        assert!(audio.is_empty());
    }
}
