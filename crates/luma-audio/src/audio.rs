//! The audio system: an output device driving the mixer, or a null
//! backend that advances sounds by game time when no device is there
//! (continuous integration, headless runs).
//!
//! Written with reference to `BSoundMgr.cpp`, `BSoundBuffer.cpp` and
//! `BSound.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan
//! Nedoma); see `NOTICE`.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::decode::Clip;
use crate::error::{Error, Result};
use crate::mixer::{Mixer, SoundId, SoundType};

enum Backend {
    /// A device pulls samples from the mixer on its own thread.
    Device {
        _stream: cpal::Stream,
        description: String,
    },
    /// No device: [`Audio::tick`] advances the sounds.
    Null,
}

/// Sounds loaded and playing.
pub struct Audio {
    mixer: Arc<Mutex<Mixer>>,
    backend: Backend,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("device", &self.device())
            .finish()
    }
}

impl Audio {
    /// Opens the default output device; without one, a null backend.
    pub fn new() -> Audio {
        match Audio::with_device() {
            Ok(audio) => audio,
            Err(_) => Audio::null(),
        }
    }

    /// Opens the default output device, or fails.
    pub fn with_device() -> Result<Audio> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| Error::Device("no output device".to_string()))?;
        let supported = device
            .default_output_config()
            .map_err(|e| Error::Device(e.to_string()))?;
        let config: cpal::StreamConfig = supported.config();
        let rate = config.sample_rate;
        let channels = usize::from(config.channels);
        let mixer = Arc::new(Mutex::new(Mixer::default()));
        let shared = mixer.clone();
        let error = |e: cpal::Error| eprintln!("audio stream: {e}");
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config,
                move |out: &mut [f32], _| {
                    out.fill(0.0);
                    if let Ok(mut mixer) = shared.lock() {
                        mixer.mix(out, rate, channels);
                    }
                },
                error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                config,
                move |out: &mut [i16], _| {
                    let mut buffer = vec![0.0f32; out.len()];
                    if let Ok(mut mixer) = shared.lock() {
                        mixer.mix(&mut buffer, rate, channels);
                    }
                    for (o, s) in out.iter_mut().zip(buffer) {
                        *o = (s * 32767.0) as i16;
                    }
                },
                error,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                config,
                move |out: &mut [u16], _| {
                    let mut buffer = vec![0.0f32; out.len()];
                    if let Ok(mut mixer) = shared.lock() {
                        mixer.mix(&mut buffer, rate, channels);
                    }
                    for (o, s) in out.iter_mut().zip(buffer) {
                        *o = ((s * 32767.0) as i32 + 32768) as u16;
                    }
                },
                error,
                None,
            ),
            other => return Err(Error::Device(format!("sample format {other:?}"))),
        }
        .map_err(|e| Error::Device(e.to_string()))?;
        stream.play().map_err(|e| Error::Device(e.to_string()))?;
        let description = format!(
            "{} ({} Hz, {} channel(s))",
            device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "unknown device".to_string()),
            rate,
            channels
        );
        Ok(Audio {
            mixer,
            backend: Backend::Device {
                _stream: stream,
                description,
            },
        })
    }

    /// An audio system without a device: sounds play silently, advanced
    /// by [`Audio::tick`].
    pub fn null() -> Audio {
        Audio {
            mixer: Arc::new(Mutex::new(Mixer::default())),
            backend: Backend::Null,
        }
    }

    /// The output device, or `None` for the null backend.
    pub fn device(&self) -> Option<&str> {
        match &self.backend {
            Backend::Device { description, .. } => Some(description),
            Backend::Null => None,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Mixer> {
        match self.mixer.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Advances the sounds by `ms` on the null backend; a device needs
    /// nothing.
    pub fn tick(&self, ms: u64) {
        if matches!(self.backend, Backend::Null) {
            self.lock().advance_ms(ms);
        }
    }

    /// Loads a decoded sound (`CBSoundMgr::AddSound`), stopped, with its
    /// own volume in percent.
    pub fn add(&self, clip: Arc<Clip>, kind: SoundType, private_volume: i32) -> SoundId {
        self.lock().add(clip, kind, private_volume)
    }

    /// Unloads a sound.
    pub fn remove(&self, id: SoundId) -> bool {
        self.lock().remove(id)
    }

    /// Whether the sound is loaded.
    pub fn contains(&self, id: SoundId) -> bool {
        self.lock().voice(id).is_some()
    }

    /// The number of sounds loaded.
    pub fn len(&self) -> usize {
        self.lock().voices.len()
    }

    /// Whether no sound is loaded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Starts or restarts a sound from its position (`Play`).
    pub fn play(&self, id: SoundId, looping: bool) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        voice.looping = looping;
        voice.paused = false;
        if voice.position >= voice.clip.frames() as f64 {
            voice.position = 0.0;
        }
        voice.playing = true;
        true
    }

    /// Stops a sound (`Stop`); its position goes back to the start.
    pub fn stop(&self, id: SoundId) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        voice.playing = false;
        voice.paused = false;
        voice.freeze_paused = false;
        voice.position = 0.0;
        true
    }

    /// Pauses a sound (`Pause`).
    pub fn pause(&self, id: SoundId) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        if voice.playing {
            voice.paused = true;
        }
        true
    }

    /// Resumes a paused sound (`Resume`).
    pub fn resume(&self, id: SoundId) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        voice.paused = false;
        voice.freeze_paused = false;
        true
    }

    /// Whether a sound is playing: started and not finished, paused or
    /// not (`CBSoundBuffer::IsPlaying` counts a frozen sound as playing).
    pub fn is_playing(&self, id: SoundId) -> bool {
        self.lock()
            .voice(id)
            .is_some_and(|v| v.playing && (!v.paused || v.freeze_paused))
    }

    /// Whether a sound is paused.
    pub fn is_paused(&self, id: SoundId) -> bool {
        self.lock().voice(id).is_some_and(|v| v.paused)
    }

    /// The length of a sound in milliseconds.
    pub fn length_ms(&self, id: SoundId) -> u64 {
        self.lock().voice(id).map_or(0, |v| v.clip.duration_ms())
    }

    /// The position of a sound in milliseconds.
    pub fn position_ms(&self, id: SoundId) -> u64 {
        self.lock().voice(id).map_or(0, |v| {
            (v.position * 1000.0 / f64::from(v.clip.sample_rate.max(1))) as u64
        })
    }

    /// Moves a sound to a time (`SetPosition`).
    pub fn set_position_ms(&self, id: SoundId, ms: u64) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        let frame = voice.clip.frame_at(ms).min(voice.clip.frames());
        voice.position = frame as f64;
        true
    }

    /// Sets where a loop restarts (`SetLoopStart`), in milliseconds.
    pub fn set_loop_start_ms(&self, id: SoundId, ms: u64) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        voice.loop_start = voice.clip.frame_at(ms).min(voice.clip.frames());
        true
    }

    /// Sets whether a sound loops.
    pub fn set_looping(&self, id: SoundId, looping: bool) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        voice.looping = looping;
        true
    }

    /// The sound's own volume in percent.
    pub fn private_volume(&self, id: SoundId) -> i32 {
        self.lock().voice(id).map_or(0, |v| v.private_volume)
    }

    /// Sets the sound's own volume in percent (`SetPrivateVolume`).
    pub fn set_private_volume(&self, id: SoundId, percent: i32) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        voice.private_volume = percent;
        true
    }

    /// Sets the pan, `-1` left to `1` right (`SetPan`).
    pub fn set_pan(&self, id: SoundId, pan: f32) -> bool {
        let mut mixer = self.lock();
        let Some(voice) = mixer.voice_mut(id) else {
            return false;
        };
        voice.pan = pan.clamp(-1.0, 1.0);
        true
    }

    /// The global volume of a sound type, in percent.
    pub fn volume(&self, kind: SoundType) -> i32 {
        let mixer = self.lock();
        match kind {
            SoundType::Sfx => mixer.volume_sfx,
            SoundType::Speech => mixer.volume_speech,
            SoundType::Music => mixer.volume_music,
        }
    }

    /// Sets the global volume of a sound type (`SetVolumePercent`).
    pub fn set_volume(&self, kind: SoundType, percent: i32) {
        let mut mixer = self.lock();
        let percent = percent.clamp(0, 100);
        match kind {
            SoundType::Sfx => mixer.volume_sfx = percent,
            SoundType::Speech => mixer.volume_speech = percent,
            SoundType::Music => mixer.volume_music = percent,
        }
    }

    /// The master volume in percent.
    pub fn master_volume(&self) -> i32 {
        self.lock().volume_master
    }

    /// Sets the master volume (`SetMasterVolumePercent`).
    pub fn set_master_volume(&self, percent: i32) {
        self.lock().volume_master = percent.clamp(0, 100);
    }

    /// Pauses every playing sound, music too when asked
    /// (`CBSoundMgr::PauseAll`), to be resumed by [`Audio::resume_all`].
    pub fn pause_all(&self, including_music: bool) {
        let mut mixer = self.lock();
        for voice in &mut mixer.voices {
            if voice.playing && !voice.paused && (voice.kind != SoundType::Music || including_music)
            {
                voice.paused = true;
                voice.freeze_paused = true;
            }
        }
    }

    /// Resumes the sounds [`Audio::pause_all`] paused.
    pub fn resume_all(&self) {
        let mut mixer = self.lock();
        for voice in &mut mixer.voices {
            if voice.freeze_paused {
                voice.paused = false;
                voice.freeze_paused = false;
            }
        }
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
    fn null_backend_plays_by_time() {
        let audio = Audio::null();
        assert!(audio.device().is_none());
        let clip = Arc::new(Clip::silence(1000, 1, 500));
        let id = audio.add(clip, SoundType::Speech, 80);
        assert_eq!(audio.length_ms(id), 500);
        assert!(!audio.is_playing(id));
        assert!(audio.play(id, false));
        assert!(audio.is_playing(id));
        audio.tick(200);
        assert_eq!(audio.position_ms(id), 200);
        assert!(audio.pause(id));
        assert!(!audio.is_playing(id));
        assert!(audio.is_paused(id));
        audio.tick(200);
        assert_eq!(audio.position_ms(id), 200);
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
        assert!(audio.is_playing(id), "a frozen sound counts as playing");
        assert!(audio.is_paused(id));
        audio.resume_all();
        assert!(!audio.is_paused(id));
        assert!(audio.stop(id));
        assert_eq!(audio.position_ms(id), 0);
        assert!(audio.remove(id));
        assert!(!audio.contains(id));
        assert!(audio.is_empty());
    }
}
