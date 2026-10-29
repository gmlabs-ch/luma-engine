//! Sound in the world: the sound effect each object owns, the game's
//! music channels, the crossfade between two of them, and the speech
//! files of sentences (see `docs/audio.md`).
//!
//! Written with reference to `BObject.cpp`, `BGame.cpp`, `AdGame.cpp` and
//! `BSoundMgr.cpp` of WME Lite (MIT License, Copyright (c) 2011 Jan
//! Nedoma); see `NOTICE`.

use std::collections::HashMap;
use std::sync::Arc;

use luma_audio::{Audio, Clip, SoundId, SoundType};
use luma_script::NativeId;

use crate::objects::Kind;
use crate::scene_view::key;
use crate::world::World;

/// The number of music channels (`NUM_MUSIC_CHANNELS`).
pub const MUSIC_CHANNELS: usize = 5;

/// The sound effect an object owns (`CBObject::m_SFX`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSound {
    /// The sound in the mixer.
    pub id: SoundId,
    /// The file it came from.
    pub filename: String,
    /// An event raised on the owner when the sound ends
    /// (`PlaySoundEvent`).
    pub event: Option<String>,
}

/// The sound state of an object: its sound, where the next one starts,
/// and its volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSfx {
    /// The loaded sound, if any.
    pub sound: Option<ObjectSound>,
    /// Where the sound starts, in milliseconds (`m_SFXStart`).
    pub start_ms: u64,
    /// The object's sound volume in percent (`m_SFXVolume`).
    pub volume: i32,
}

impl Default for ObjectSfx {
    fn default() -> Self {
        ObjectSfx {
            sound: None,
            start_ms: 0,
            volume: 100,
        }
    }
}

/// A music channel (`m_Music[i]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicChannel {
    /// The sound in the mixer.
    pub id: SoundId,
    /// The file it came from.
    pub filename: String,
}

/// A crossfade between two music channels (`MusicCrossfade`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Crossfade {
    start: u64,
    channel1: usize,
    channel2: usize,
    length: u64,
    swap: bool,
}

/// Everything the world plays.
#[derive(Debug)]
pub struct Sounds {
    /// The audio system.
    pub audio: Audio,
    clips: HashMap<String, Arc<Clip>>,
    /// The sound effect of each object.
    pub objects: HashMap<NativeId, ObjectSfx>,
    /// The music channels.
    pub music: Vec<Option<MusicChannel>>,
    pub(crate) music_start_time: Vec<u64>,
    pub(crate) crossfade: Option<Crossfade>,
    /// Directories searched for speech files, with a trailing separator.
    pub speech_dirs: Vec<String>,
}

impl Sounds {
    /// A sound state on an audio system.
    pub fn new(audio: Audio) -> Sounds {
        Sounds {
            audio,
            clips: HashMap::new(),
            objects: HashMap::new(),
            music: vec![None; MUSIC_CHANNELS],
            music_start_time: vec![0; MUSIC_CHANNELS],
            crossfade: None,
            speech_dirs: vec!["speech\\".to_string()],
        }
    }

    /// The pan of a point on screen (`CBSoundMgr::PosToPan`): the range
    /// from half a screen left of it to half a screen right of it, with
    /// the screen width `width`.
    pub fn pos_to_pan(x: i32, width: i32) -> f32 {
        let width = width as f32;
        let relative = ((x as f32 + 0.5 * width) / (width * 2.0)).clamp(0.0, 1.0);
        -1.0 + relative * 2.0
    }

    /// Adds a speech directory (`AddSpeechDir`), once.
    pub fn add_speech_dir(&mut self, dir: &str) -> bool {
        if dir.is_empty() {
            return false;
        }
        let mut dir = dir.to_string();
        if !dir.ends_with('\\') && !dir.ends_with('/') {
            dir.push('\\');
        }
        if !self
            .speech_dirs
            .iter()
            .any(|d| d.eq_ignore_ascii_case(&dir))
        {
            self.speech_dirs.push(dir);
        }
        true
    }

    /// Removes a speech directory (`RemoveSpeechDir`); whether it was
    /// there.
    pub fn remove_speech_dir(&mut self, dir: &str) -> bool {
        if dir.is_empty() {
            return false;
        }
        let mut dir = dir.to_string();
        if !dir.ends_with('\\') && !dir.ends_with('/') {
            dir.push('\\');
        }
        let before = self.speech_dirs.len();
        self.speech_dirs.retain(|d| !d.eq_ignore_ascii_case(&dir));
        self.speech_dirs.len() != before
    }
}

impl World {
    /// The audio system.
    pub fn audio(&self) -> &Audio {
        &self.sounds.audio
    }

    /// The sound state.
    pub fn sounds(&self) -> &Sounds {
        &self.sounds
    }

    /// The sound state, mutable.
    pub fn sounds_mut(&mut self) -> &mut Sounds {
        &mut self.sounds
    }

    /// Loads and decodes a sound file, once (`CBSoundMgr::AddSound`): a
    /// `.wav` is replaced by the `.ogg` of the same name when the game
    /// has one. Returns the clip and the file used.
    pub fn load_clip(&mut self, filename: &str) -> Option<(Arc<Clip>, String)> {
        let mut path = filename.to_string();
        if path.to_ascii_lowercase().ends_with(".wav") {
            let ogg = format!("{}.ogg", &path[..path.len() - 4]);
            if self.vfs.exists(&ogg) {
                path = ogg;
            }
        }
        let k = key(&path);
        if let Some(clip) = self.sounds.clips.get(&k) {
            return Some((clip.clone(), path));
        }
        let bytes = match self.vfs.read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.log(format!("Error opening sound file '{path}'"));
                return None;
            }
        };
        match luma_audio::decode(&bytes) {
            Ok(clip) => {
                let clip = Arc::new(clip);
                self.sounds.clips.insert(k, clip.clone());
                Some((clip, path))
            }
            Err(err) => {
                self.log(format!("Error loading sound '{path}': {err}"));
                None
            }
        }
    }

    /// The length of a sound file in milliseconds (`Game.GetSoundLength`).
    pub fn sound_length(&mut self, filename: &str) -> u64 {
        self.load_clip(filename)
            .map_or(0, |(clip, _)| clip.duration_ms())
    }

    fn can_own_sound(&self, owner: NativeId) -> bool {
        matches!(
            self.objects.get(owner).map(|n| &n.kind),
            Some(Kind::Game | Kind::Entity(_) | Kind::Actor)
        )
    }

    /// Loads an object's sound effect and plays it when `play_now`
    /// (`CBObject::PlaySFX`): without a file, the loaded sound plays
    /// again. Returns whether a sound is there.
    pub fn play_sfx(
        &mut self,
        owner: NativeId,
        filename: Option<&str>,
        looping: bool,
        play_now: bool,
        event: Option<String>,
        loop_start_ms: u64,
    ) -> bool {
        if !self.can_own_sound(owner) {
            return false;
        }
        let state = self.sounds.objects.entry(owner).or_default().clone();
        match filename {
            None => {
                let Some(sound) = state.sound.clone() else {
                    return false;
                };
                if state.start_ms != 0 {
                    self.sounds.audio.set_private_volume(sound.id, state.volume);
                    self.sounds.audio.set_position_ms(sound.id, state.start_ms);
                    if let Some(s) = self.sounds.objects.get_mut(&owner) {
                        s.start_ms = 0;
                    }
                }
                if play_now {
                    if let Some(s) = self.sounds.objects.get_mut(&owner) {
                        if let Some(sound) = &mut s.sound {
                            sound.event = event;
                        }
                    }
                    if loop_start_ms != 0 {
                        self.sounds.audio.set_loop_start_ms(sound.id, loop_start_ms);
                    }
                    self.sounds.audio.play(sound.id, looping);
                }
                true
            }
            Some(filename) => {
                self.stop_sfx(owner, true);
                let Some((clip, path)) = self.load_clip(filename) else {
                    return false;
                };
                let id = self.sounds.audio.add(clip, SoundType::Sfx, state.volume);
                if state.start_ms != 0 {
                    self.sounds.audio.set_position_ms(id, state.start_ms);
                }
                let sound = ObjectSound {
                    id,
                    filename: path,
                    event: if play_now { event } else { None },
                };
                let entry = self.sounds.objects.entry(owner).or_default();
                entry.sound = Some(sound);
                entry.start_ms = 0;
                if play_now {
                    if loop_start_ms != 0 {
                        self.sounds.audio.set_loop_start_ms(id, loop_start_ms);
                    }
                    self.sounds.audio.play(id, looping);
                }
                true
            }
        }
    }

    /// Stops an object's sound (`StopSFX`), unloading it when asked.
    /// Whether the object had one.
    pub fn stop_sfx(&mut self, owner: NativeId, unload: bool) -> bool {
        let Some(state) = self.sounds.objects.get_mut(&owner) else {
            return false;
        };
        let Some(sound) = state.sound.clone() else {
            return false;
        };
        self.sounds.audio.stop(sound.id);
        if unload {
            self.sounds.audio.remove(sound.id);
            if let Some(state) = self.sounds.objects.get_mut(&owner) {
                state.sound = None;
            }
        }
        true
    }

    /// The sound an object owns.
    pub fn sfx_of(&self, owner: NativeId) -> Option<&ObjectSound> {
        self.sounds.objects.get(&owner)?.sound.as_ref()
    }

    /// Pauses an object's sound (`PauseSFX`); whether it had one.
    pub fn pause_sfx(&mut self, owner: NativeId) -> bool {
        match self.sfx_of(owner).map(|s| s.id) {
            Some(id) => self.sounds.audio.pause(id),
            None => false,
        }
    }

    /// Resumes an object's sound (`ResumeSFX`); whether it had one.
    pub fn resume_sfx(&mut self, owner: NativeId) -> bool {
        match self.sfx_of(owner).map(|s| s.id) {
            Some(id) => self.sounds.audio.resume(id),
            None => false,
        }
    }

    /// Whether an object's sound is playing.
    pub fn is_sfx_playing(&self, owner: NativeId) -> bool {
        self.sfx_of(owner)
            .is_some_and(|s| self.sounds.audio.is_playing(s.id))
    }

    /// Sets where an object's sound plays from (`SetSFXTime`): at once
    /// when it is playing, else when it next starts.
    pub fn set_sfx_time(&mut self, owner: NativeId, ms: u64) -> bool {
        if !self.can_own_sound(owner) {
            return false;
        }
        let state = self.sounds.objects.entry(owner).or_default();
        state.start_ms = ms;
        if let Some(sound) = &state.sound {
            if self.sounds.audio.is_playing(sound.id) {
                self.sounds.audio.set_position_ms(sound.id, ms);
            }
        }
        true
    }

    /// The position of an object's sound in milliseconds, 0 when it is
    /// not playing.
    pub fn sfx_position(&self, owner: NativeId) -> u64 {
        match self.sfx_of(owner) {
            Some(s) if self.sounds.audio.is_playing(s.id) => self.sounds.audio.position_ms(s.id),
            _ => 0,
        }
    }

    /// Sets an object's sound volume (`SetSFXVolume`).
    pub fn set_sfx_volume(&mut self, owner: NativeId, percent: i32) -> bool {
        if !self.can_own_sound(owner) {
            return false;
        }
        let state = self.sounds.objects.entry(owner).or_default();
        state.volume = percent;
        if let Some(sound) = &state.sound {
            self.sounds.audio.set_private_volume(sound.id, percent);
        }
        true
    }

    /// An object's sound volume: its sound's, else the object's setting.
    pub fn sfx_volume(&self, owner: NativeId) -> i32 {
        match self.sounds.objects.get(&owner) {
            Some(state) => match &state.sound {
                Some(sound) => self.sounds.audio.private_volume(sound.id),
                None => state.volume,
            },
            None => 100,
        }
    }

    /// Forgets the sounds of objects that no longer exist.
    pub(crate) fn drop_orphan_sounds(&mut self) {
        let gone: Vec<NativeId> = self
            .sounds
            .objects
            .keys()
            .copied()
            .filter(|id| !self.objects.contains(*id))
            .collect();
        for owner in gone {
            if let Some(state) = self.sounds.objects.remove(&owner) {
                if let Some(sound) = state.sound {
                    self.sounds.audio.remove(sound.id);
                }
            }
        }
    }

    /// One frame of the objects' sounds (`CBObject::UpdateSounds`): a
    /// sound event fires when its sound ended, and the sounds of the
    /// objects that pan follow their position on screen.
    pub fn update_sounds(&mut self) {
        let (ox, width) = (self.offset_x, self.settings.width as i32);
        let owners: Vec<NativeId> = self.sounds.objects.keys().copied().collect();
        for owner in owners {
            let Some(sound) = self.sfx_of(owner).cloned() else {
                continue;
            };
            if let Some(event) = &sound.event {
                if !self.sounds.audio.is_playing(sound.id) {
                    let event = event.clone();
                    if let Some(s) = self
                        .sounds
                        .objects
                        .get_mut(&owner)
                        .and_then(|s| s.sound.as_mut())
                    {
                        s.event = None;
                    }
                    self.queue_event(owner, event);
                }
            }
            let panned = match self.objects.get(owner).map(|n| n.kind.clone()) {
                Some(Kind::Actor) => self
                    .actor(owner)
                    .filter(|a| a.def.active && a.def.sound_panning)
                    .map(|a| a.x),
                Some(Kind::Entity(r)) => self
                    .scene()
                    .and_then(|v| v.entity(r))
                    .filter(|e| e.active && e.sound_panning)
                    .map(|e| e.x),
                _ => None,
            };
            if let Some(x) = panned {
                self.sounds
                    .audio
                    .set_pan(sound.id, Sounds::pos_to_pan(x - ox, width));
            }
        }
        self.update_crossfade();
    }

    /// Plays music on a channel (`CBGame::PlayMusic`), replacing what
    /// played there.
    pub fn play_music(
        &mut self,
        channel: usize,
        filename: &str,
        looping: bool,
        loop_start_ms: u64,
        private_volume: i32,
    ) -> bool {
        if channel >= MUSIC_CHANNELS {
            self.log(format!(
                "**Error** Attempting to use music channel {channel} (max num channels: {MUSIC_CHANNELS})"
            ));
            return false;
        }
        self.stop_music(channel);
        let Some((clip, path)) = self.load_clip(filename) else {
            return false;
        };
        let id = self
            .sounds
            .audio
            .add(clip, SoundType::Music, private_volume);
        let start = std::mem::take(&mut self.sounds.music_start_time[channel]);
        self.sounds.audio.set_position_ms(id, start);
        if loop_start_ms != 0 {
            self.sounds.audio.set_loop_start_ms(id, loop_start_ms);
        }
        self.sounds.audio.play(id, looping);
        self.sounds.music[channel] = Some(MusicChannel { id, filename: path });
        true
    }

    /// Stops and unloads a channel's music (`StopMusic`); whether there
    /// was any.
    pub fn stop_music(&mut self, channel: usize) -> bool {
        if channel >= MUSIC_CHANNELS {
            return false;
        }
        match self.sounds.music[channel].take() {
            Some(music) => {
                self.sounds.audio.stop(music.id);
                self.sounds.audio.remove(music.id);
                true
            }
            None => false,
        }
    }

    /// The music of a channel.
    pub fn music(&self, channel: usize) -> Option<&MusicChannel> {
        self.sounds.music.get(channel)?.as_ref()
    }

    /// Pauses a channel (`PauseMusic`).
    pub fn pause_music(&mut self, channel: usize) -> bool {
        match self.music(channel).map(|m| m.id) {
            Some(id) => self.sounds.audio.pause(id),
            None => false,
        }
    }

    /// Resumes a channel (`ResumeMusic`).
    pub fn resume_music(&mut self, channel: usize) -> bool {
        match self.music(channel).map(|m| m.id) {
            Some(id) => self.sounds.audio.resume(id),
            None => false,
        }
    }

    /// Sets where a channel's music plays from (`SetMusicStartTime`): at
    /// once when playing, else when it next starts.
    pub fn set_music_start_time(&mut self, channel: usize, ms: u64) -> bool {
        if channel >= MUSIC_CHANNELS {
            return false;
        }
        self.sounds.music_start_time[channel] = ms;
        if let Some(id) = self.music(channel).map(|m| m.id) {
            if self.sounds.audio.is_playing(id) {
                self.sounds.audio.set_position_ms(id, ms);
            }
        }
        true
    }

    /// The position of a channel's music in milliseconds.
    pub fn music_position(&self, channel: usize) -> u64 {
        match self.music(channel) {
            Some(m) if self.sounds.audio.is_playing(m.id) => self.sounds.audio.position_ms(m.id),
            _ => 0,
        }
    }

    /// Whether a channel's music is playing.
    pub fn is_music_playing(&self, channel: usize) -> bool {
        self.music(channel)
            .is_some_and(|m| self.sounds.audio.is_playing(m.id))
    }

    /// Sets a channel's own volume (`SetMusicVolume`).
    pub fn set_music_volume(&mut self, channel: usize, percent: i32) -> bool {
        match self.music(channel).map(|m| m.id) {
            Some(id) => self.sounds.audio.set_private_volume(id, percent),
            None => false,
        }
    }

    /// A channel's own volume.
    pub fn music_volume(&self, channel: usize) -> i32 {
        self.music(channel)
            .map_or(0, |m| self.sounds.audio.private_volume(m.id))
    }

    /// Starts a crossfade from one channel to another over `length`
    /// milliseconds (`MusicCrossfade`); `false` while one runs.
    pub fn music_crossfade(
        &mut self,
        channel1: usize,
        channel2: usize,
        length: u64,
        swap: bool,
    ) -> bool {
        if self.sounds.crossfade.is_some() {
            return false;
        }
        self.sounds.crossfade = Some(Crossfade {
            start: self.clock,
            channel1,
            channel2,
            length,
            swap,
        });
        true
    }

    /// Whether a crossfade is running.
    pub fn is_crossfading(&self) -> bool {
        self.sounds.crossfade.is_some()
    }

    /// One frame of the crossfade (`CBGame::UpdateMusicCrossfade`): the
    /// first channel fades out while the second fades in, on the wall
    /// clock; at the end the first stops and the channels are swapped
    /// when asked.
    fn update_crossfade(&mut self) {
        let Some(fade) = self.sounds.crossfade.clone() else {
            return;
        };
        let (Some(first), Some(second)) = (
            self.music(fade.channel1).map(|m| m.id),
            self.music(fade.channel2).map(|m| m.id),
        ) else {
            self.sounds.crossfade = None;
            return;
        };
        for id in [first, second] {
            if !self.sounds.audio.is_playing(id) {
                self.sounds.audio.play(id, false);
            }
        }
        let elapsed = self.clock.saturating_sub(fade.start);
        if elapsed >= fade.length {
            self.sounds.crossfade = None;
            self.sounds.audio.set_private_volume(second, 100);
            self.sounds.audio.stop(first);
            self.sounds.audio.set_private_volume(first, 100);
            if fade.swap {
                self.sounds.music.swap(fade.channel1, fade.channel2);
                self.sounds
                    .music_start_time
                    .swap(fade.channel1, fade.channel2);
            }
        } else {
            let progress = (elapsed as f64 / fade.length as f64 * 100.0) as i32;
            self.sounds.audio.set_private_volume(first, 100 - progress);
            self.sounds.audio.set_private_volume(second, progress);
        }
    }

    /// The speech file of a string identifier (`CAdGame::FindSpeechFile`):
    /// `<dir><ID>.ogg` or `.wav` in the speech directories.
    pub fn find_speech_file(&self, key: &str) -> Option<String> {
        for dir in &self.sounds.speech_dirs {
            for ext in ["ogg", "wav"] {
                let path = format!("{dir}{key}.{ext}");
                if self.vfs.exists(&path) {
                    return Some(path);
                }
            }
        }
        None
    }

    /// Loads a speech sound for a sentence and starts it; returns the
    /// sound and its length.
    pub(crate) fn start_speech(&mut self, filename: &str) -> Option<(SoundId, u64)> {
        let (clip, _) = self.load_clip(filename)?;
        let length = clip.duration_ms();
        let id = self.sounds.audio.add(clip, SoundType::Speech, 100);
        self.sounds.audio.play(id, false);
        Some((id, length))
    }

    /// Ends the sentences whose speech sound finished, and unloads the
    /// sounds of the sentences that ended (`TimeIsUp` with a sound).
    pub(crate) fn update_speech(&mut self) {
        let mut finished: Vec<SoundId> = Vec::new();
        for (_, actor) in &mut self.actors {
            let talking = actor.is_talking();
            if let Some(sentence) = &mut actor.sentence {
                if let Some(id) = sentence.sound {
                    let ended =
                        !self.sounds.audio.is_playing(id) && !self.sounds.audio.is_paused(id);
                    if ended {
                        sentence.finish();
                    }
                    if !talking || ended {
                        finished.push(id);
                        sentence.sound = None;
                    }
                }
            }
        }
        if let Some(view) = self.scene.as_mut() {
            for sentence in view.sentences_mut() {
                if let Some(id) = sentence.sound {
                    if !self.sounds.audio.is_playing(id) && !self.sounds.audio.is_paused(id) {
                        sentence.finish();
                        finished.push(id);
                        sentence.sound = None;
                    }
                }
            }
        }
        for id in finished {
            self.sounds.audio.stop(id);
            self.sounds.audio.remove(id);
        }
    }

    /// Stops the speech sound of every sentence that ended at once.
    pub(crate) fn stop_speech(&mut self) {
        let mut stopped: Vec<SoundId> = Vec::new();
        for (_, actor) in &mut self.actors {
            if let Some(sentence) = &mut actor.sentence {
                if sentence.duration == 0 {
                    stopped.extend(sentence.sound.take());
                }
            }
        }
        if let Some(view) = self.scene.as_mut() {
            for sentence in view.sentences_mut() {
                if sentence.duration == 0 {
                    stopped.extend(sentence.sound.take());
                }
            }
        }
        for id in stopped {
            self.sounds.audio.stop(id);
            self.sounds.audio.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pans_across_the_screen() {
        assert_eq!(Sounds::pos_to_pan(0, 100), -0.5);
        assert_eq!(Sounds::pos_to_pan(50, 100), 0.0);
        assert_eq!(Sounds::pos_to_pan(100, 100), 0.5);
        assert_eq!(Sounds::pos_to_pan(-100, 100), -1.0);
        assert_eq!(Sounds::pos_to_pan(400, 100), 1.0);
    }

    #[test]
    fn speech_dirs() {
        let mut sounds = Sounds::new(Audio::null());
        assert_eq!(sounds.speech_dirs, ["speech\\"]);
        assert!(sounds.add_speech_dir("english"));
        assert!(sounds.add_speech_dir("English\\"));
        assert_eq!(sounds.speech_dirs, ["speech\\", "english\\"]);
        assert!(!sounds.add_speech_dir(""));
        assert!(sounds.remove_speech_dir("ENGLISH"));
        assert!(!sounds.remove_speech_dir("english"));
    }
}
