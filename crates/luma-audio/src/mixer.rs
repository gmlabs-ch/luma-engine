//! The mixer: the voices playing, their volumes and pans, mixed into an
//! output buffer, or advanced by time when there is no device.
//!
//! The volume rules follow the original engine (`BSoundBuffer` and
//! `BSoundMgr` of WME Lite, MIT License, Copyright (c) 2011 Jan Nedoma);
//! see `NOTICE`.

use std::sync::Arc;

use crate::decode::Clip;

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

/// A sound loaded into the mixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundId(pub u64);

/// A loaded sound and its playback state.
#[derive(Debug)]
pub(crate) struct Voice {
    pub id: SoundId,
    pub clip: Arc<Clip>,
    pub kind: SoundType,
    pub playing: bool,
    pub paused: bool,
    /// Paused by `pause_all`, resumed by `resume_all`.
    pub freeze_paused: bool,
    pub looping: bool,
    /// Where a loop restarts, in frames.
    pub loop_start: usize,
    /// The frame being played.
    pub position: f64,
    /// The sound's own volume in percent (`m_PrivateVolume`).
    pub private_volume: i32,
    /// `-1` (left) to `1` (right).
    pub pan: f32,
}

/// The gain of a volume in percent, as the engine attenuates it: the
/// cube of the remaining fraction, in hundredths of a decibel, turned
/// back into a factor.
pub fn gain(volume_percent: f32) -> f32 {
    let remaining = 1.0 - (volume_percent / 100.0).clamp(0.0, 1.0);
    let attenuation = -10000.0 * remaining.powi(3);
    10f32.powf(attenuation / 2000.0)
}

/// The state shared between the game and the output thread.
#[derive(Debug)]
pub struct Mixer {
    pub(crate) voices: Vec<Voice>,
    pub(crate) volume_sfx: i32,
    pub(crate) volume_speech: i32,
    pub(crate) volume_music: i32,
    pub(crate) volume_master: i32,
    next_id: u64,
}

impl Default for Mixer {
    fn default() -> Self {
        Mixer {
            voices: Vec::new(),
            volume_sfx: 100,
            volume_speech: 100,
            volume_music: 100,
            volume_master: 100,
            next_id: 1,
        }
    }
}

impl Mixer {
    pub(crate) fn add(&mut self, clip: Arc<Clip>, kind: SoundType, private_volume: i32) -> SoundId {
        let id = SoundId(self.next_id);
        self.next_id += 1;
        self.voices.push(Voice {
            id,
            clip,
            kind,
            playing: false,
            paused: false,
            freeze_paused: false,
            looping: false,
            loop_start: 0,
            position: 0.0,
            private_volume,
            pan: 0.0,
        });
        id
    }

    pub(crate) fn voice(&self, id: SoundId) -> Option<&Voice> {
        self.voices.iter().find(|v| v.id == id)
    }

    pub(crate) fn voice_mut(&mut self, id: SoundId) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|v| v.id == id)
    }

    pub(crate) fn remove(&mut self, id: SoundId) -> bool {
        let before = self.voices.len();
        self.voices.retain(|v| v.id != id);
        self.voices.len() != before
    }

    fn type_volume(&self, kind: SoundType) -> i32 {
        match kind {
            SoundType::Sfx => self.volume_sfx,
            SoundType::Speech => self.volume_speech,
            SoundType::Music => self.volume_music,
        }
    }

    /// The gain of a voice: its type's volume weighted by its own, through
    /// the engine's curve, times the master volume.
    fn voice_gain(&self, voice: &Voice) -> f32 {
        let weighted = self.type_volume(voice.kind) as f32 / 100.0 * voice.private_volume as f32;
        gain(weighted) * (self.volume_master as f32 / 100.0).clamp(0.0, 1.0)
    }

    /// Moves a voice forward by `frames` of its own clip, looping or
    /// stopping at the end.
    fn advance(voice: &mut Voice, frames: f64) {
        let total = voice.clip.frames() as f64;
        voice.position += frames;
        if voice.position >= total {
            if voice.looping && total > 0.0 {
                let loop_start = (voice.loop_start as f64).min(total - 1.0);
                voice.position =
                    loop_start + (voice.position - total) % (total - loop_start).max(1.0);
            } else {
                voice.position = total;
                voice.playing = false;
            }
        }
    }

    /// Advances every playing voice by a time, for the null backend.
    pub(crate) fn advance_ms(&mut self, ms: u64) {
        for voice in &mut self.voices {
            if voice.playing && !voice.paused {
                let frames = ms as f64 * f64::from(voice.clip.sample_rate) / 1000.0;
                Mixer::advance(voice, frames);
            }
        }
    }

    /// Mixes the playing voices into an interleaved stereo buffer at the
    /// output rate, adding to what is there.
    pub(crate) fn mix(&mut self, out: &mut [f32], out_rate: u32, out_channels: usize) {
        if out_channels == 0 || out_rate == 0 {
            return;
        }
        let frames = out.len() / out_channels;
        let gains: Vec<f32> = self.voices.iter().map(|v| self.voice_gain(v)).collect();
        for (voice, gain) in self.voices.iter_mut().zip(gains) {
            if !voice.playing || voice.paused {
                continue;
            }
            let step = f64::from(voice.clip.sample_rate) / f64::from(out_rate);
            let left_gain = gain
                * if voice.pan > 0.0 {
                    1.0 - voice.pan
                } else {
                    1.0
                };
            let right_gain = gain
                * if voice.pan < 0.0 {
                    1.0 + voice.pan
                } else {
                    1.0
                };
            for frame in 0..frames {
                if !voice.playing {
                    break;
                }
                let index = voice.position as usize;
                let fraction = (voice.position - index as f64) as f32;
                let (l0, r0) = voice.clip.frame(index);
                let (l1, r1) = voice.clip.frame(index + 1);
                let left = l0 + (l1 - l0) * fraction;
                let right = r0 + (r1 - r0) * fraction;
                let base = frame * out_channels;
                out[base] += left * left_gain;
                if out_channels > 1 {
                    out[base + 1] += right * right_gain;
                }
                Mixer::advance(voice, step);
            }
        }
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
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
    }

    #[test]
    fn mixes_loops_and_stops() {
        let clip = Arc::new(Clip {
            sample_rate: 4,
            channels: 1,
            samples: vec![16384, 16384, 16384, 16384],
        });
        let mut mixer = Mixer::default();
        let id = mixer.add(clip.clone(), SoundType::Sfx, 100);
        mixer.voice_mut(id).unwrap().playing = true;
        let mut out = vec![0.0f32; 12];
        mixer.mix(&mut out, 4, 2);
        assert!((out[0] - 0.5).abs() < 1e-3);
        assert!((out[7] - 0.5).abs() < 1e-3, "{out:?}");
        assert_eq!(out[8], 0.0, "stopped after four frames");
        assert!(!mixer.voice(id).unwrap().playing);
        // Looping from frame 2.
        let v = mixer.voice_mut(id).unwrap();
        v.playing = true;
        v.position = 0.0;
        v.looping = true;
        v.loop_start = 2;
        let mut out = vec![0.0f32; 20];
        mixer.mix(&mut out, 4, 2);
        assert!(out.iter().all(|s| (*s - 0.5).abs() < 1e-3), "{out:?}");
        assert!(mixer.voice(id).unwrap().playing);
        // Panned fully right: the left channel is silent.
        mixer.voice_mut(id).unwrap().pan = 1.0;
        let mut out = vec![0.0f32; 4];
        mixer.mix(&mut out, 4, 2);
        assert_eq!(out[0], 0.0);
        assert!((out[1] - 0.5).abs() < 1e-3);
        // Time advances the null backend the same way.
        let v = mixer.voice_mut(id).unwrap();
        v.looping = false;
        v.position = 0.0;
        mixer.advance_ms(500);
        assert_eq!(mixer.voice(id).unwrap().position, 2.0);
        mixer.advance_ms(600);
        assert!(!mixer.voice(id).unwrap().playing);
        assert!(mixer.remove(id));
        assert!(!mixer.remove(id));
    }
}
