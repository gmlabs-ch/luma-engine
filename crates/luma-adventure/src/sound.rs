//! Sound in the world: the sound effect each object owns, the game's
//! music channels, the crossfade between two of them, and the speech
//! files of sentences (see `docs/audio.md`).
//!
//! Written with reference to `BObject.cpp`, `BGame.cpp`, `BSoundMgr.cpp`,
//! `AdGame.cpp`, `AdObject.cpp` and `AdSentence.cpp` of Wintermute Engine
//! 1.x (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use std::collections::HashMap;
use std::sync::Arc;

use luma_audio::{Audio, Clip, PAN_LEFT, PAN_RIGHT, SoundEffect, SoundId, SoundType};
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
}

/// The effect an object applies to its sounds (`m_SFXType` and
/// `m_SFXParam1..4`), with the engine's parameters; see
/// [`luma_audio::SoundEffect`] for what they become.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SfxEffect {
    /// `SoundFXEcho(wetDryMix, feedback, leftDelay, rightDelay)`.
    Echo {
        /// Wet/dry mix in percent.
        wet_dry_mix: f32,
        /// Feedback in percent.
        feedback: f32,
        /// Left delay in milliseconds.
        left_delay_ms: f32,
        /// Right delay in milliseconds.
        right_delay_ms: f32,
    },
    /// `SoundFXReverb(inGain, reverbMix, reverbTime, highFreqRTRatio)`.
    Reverb {
        /// Input gain in decibels.
        in_gain_db: f32,
        /// Reverb mix in decibels.
        reverb_mix_db: f32,
        /// Reverb time in milliseconds.
        reverb_time_ms: f32,
        /// High-frequency reverb time ratio.
        high_freq_rt_ratio: f32,
    },
}

impl From<SfxEffect> for SoundEffect {
    fn from(effect: SfxEffect) -> SoundEffect {
        match effect {
            SfxEffect::Echo {
                wet_dry_mix,
                feedback,
                left_delay_ms,
                right_delay_ms,
            } => SoundEffect::Echo {
                wet_dry_mix,
                feedback,
                left_delay_ms,
                right_delay_ms,
            },
            SfxEffect::Reverb {
                in_gain_db,
                reverb_mix_db,
                reverb_time_ms,
                high_freq_rt_ratio,
            } => SoundEffect::Reverb {
                in_gain_db,
                reverb_mix_db,
                reverb_time_ms,
                high_freq_rt_ratio,
            },
        }
    }
}

/// The sound state of an object: its sound, where the next one starts,
/// its volume and its effect.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectSfx {
    /// The loaded sound, if any.
    pub sound: Option<ObjectSound>,
    /// Where the sound starts, in milliseconds (`m_SFXStart`).
    pub start_ms: u64,
    /// The object's sound volume in percent (`m_SFXVolume`).
    pub volume: i32,
    /// The effect applied to the object's sounds (`m_SFXType`).
    pub effect: Option<SfxEffect>,
    /// An event raised on the object when its sound is not playing
    /// (`m_SoundEvent`, set by `PlaySoundEvent`). It belongs to the
    /// object, not to the sound: it outlives a `StopSound` and waits for
    /// the next sound loaded.
    pub event: Option<String>,
}

impl Default for ObjectSfx {
    fn default() -> Self {
        ObjectSfx {
            sound: None,
            start_ms: 0,
            volume: 100,
            effect: None,
            event: None,
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
    /// The sounds of sprite frames playing (`CBFrame::m_Sound`), by the
    /// object showing the sprite; beside the object's own sound, dropped
    /// when they end, and not kept in saved games.
    pub(crate) frame_sounds: HashMap<NativeId, Vec<SoundId>>,
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
            frame_sounds: HashMap::new(),
        }
    }

    /// The pan of a point on screen (`CBSoundMgr::PosToPan`), in
    /// DirectSound's unit (hundredths of a decibel taken off the far
    /// channel): a margin of five screen widths on each side of the
    /// screen of width `width`, linear from full left to full right
    /// across the eleven. The centre of the screen is 0, its edges about
    /// -909 and +909; the far channel only falls silent five screens
    /// away. Farther than that the value is out of DirectSound's range
    /// and the sound keeps the pan it had.
    pub fn pos_to_pan(x: i32, width: i32) -> i32 {
        let margin = i64::from(width) * 5;
        let (min, max) = (-margin, i64::from(width) + margin);
        if max == min {
            return 0;
        }
        let relative = (i64::from(x) + margin) as f32 / (max - min) as f32;
        (PAN_LEFT as f32 + relative * (PAN_RIGHT - PAN_LEFT) as f32) as i32
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

    /// Whether the object has the sound methods: every `CBObject` of
    /// the engine does (the game, the scene and its layers, regions and
    /// waypoint groups, actors, entities, items, windows and controls,
    /// particle emitters, `new Object`s).
    fn can_own_sound(&self, owner: NativeId) -> bool {
        matches!(
            self.objects.get(owner).map(|n| &n.kind),
            Some(
                Kind::Game
                    | Kind::Scene
                    | Kind::Layer(_)
                    | Kind::Region { .. }
                    | Kind::EntityRegion(_)
                    | Kind::WaypointGroup(_)
                    | Kind::Entity(_)
                    | Kind::Actor
                    | Kind::Actor3D
                    | Kind::Attachment3D(_)
                    | Kind::Control
                    | Kind::Item
                    | Kind::Emitter(_)
                    | Kind::Object
            )
        )
    }

    /// Whether the engine runs `UpdateSounds` on the object each frame:
    /// the game always, an actor or an entity while it is active. Only
    /// these raise the sound event, follow their position between the
    /// speakers and re-apply their effect; the other objects' sounds
    /// just play.
    fn updates_sounds(&self, owner: NativeId) -> bool {
        match self.objects.get(owner).map(|n| n.kind.clone()) {
            Some(Kind::Game) => true,
            Some(Kind::Actor) => self.actor(owner).is_some_and(|a| a.def.active),
            #[cfg(feature = "3d")]
            Some(Kind::Actor3D) => self.actor3d(owner).is_some_and(|a| a.active),
            Some(Kind::Entity(r)) => self.entity_def(r).is_some_and(|e| e.active),
            _ => false,
        }
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
                        s.event = event;
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
                // The engine asks for an object's sound to be streamed.
                let id = self
                    .sounds
                    .audio
                    .add_streamed(clip, SoundType::Sfx, state.volume);
                if state.start_ms != 0 {
                    self.sounds.audio.set_position_ms(id, state.start_ms);
                }
                // `ApplyFX` before the sound plays.
                self.sounds
                    .audio
                    .set_effect(id, state.effect.map(SoundEffect::from));
                let sound = ObjectSound { id, filename: path };
                let entry = self.sounds.objects.entry(owner).or_default();
                entry.sound = Some(sound);
                entry.start_ms = 0;
                if play_now {
                    // `SetSoundEvent`: a load alone leaves the event.
                    entry.event = event;
                    if loop_start_ms != 0 {
                        self.sounds.audio.set_loop_start_ms(id, loop_start_ms);
                    }
                    self.sounds.audio.play(id, looping);
                }
                true
            }
        }
    }

    /// Sets the effect of an object's sounds (`SoundFXEcho`,
    /// `SoundFXReverb`, `SoundFXNone`): kept for the sounds the object
    /// plays next, and applied to the one it has now, as the engine
    /// re-applies it every frame (`UpdateOneSound`).
    pub fn set_sfx_effect(&mut self, owner: NativeId, effect: Option<SfxEffect>) {
        if !self.can_own_sound(owner) {
            return;
        }
        let state = self.sounds.objects.entry(owner).or_default();
        state.effect = effect;
        let sound = state.sound.as_ref().map(|s| s.id);
        // Only the objects the engine updates each frame re-apply it to
        // the sound they have; the others use it for the next sound.
        if let Some(sound) = sound.filter(|_| self.updates_sounds(owner)) {
            self.sounds
                .audio
                .set_effect(sound, effect.map(SoundEffect::from));
        }
    }

    /// Stops an object's sound (`StopSFX`), unloading it when asked
    /// (the default of the engine, which `StopSound` uses for every
    /// object but a sound entity). Whether the object had one.
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
            .chain(self.sounds.frame_sounds.keys())
            .copied()
            .filter(|id| !self.objects.contains(*id))
            .collect();
        for owner in gone {
            if let Some(state) = self.sounds.objects.remove(&owner) {
                if let Some(sound) = state.sound {
                    self.sounds.audio.remove(sound.id);
                }
            }
            self.kill_frame_sounds(owner);
        }
    }

    /// The pan of an object's sounds when it pans them (`SoundPanning`,
    /// `CBObject::UpdateOneSound`): that of its position on screen,
    /// less the scroll offset; `None` when it does not. The game's own
    /// sound is not panned: in the engine the game object stands at the
    /// centre of the screen and its sound drifted to one side as the
    /// scene scrolled, an accident of sharing `UpdateSounds` with the
    /// scene's objects, not reproduced.
    fn sound_pan_of(&self, owner: NativeId) -> Option<i32> {
        let x = match self.objects.get(owner).map(|n| n.kind.clone()) {
            Some(Kind::Actor) => self
                .actor(owner)
                .filter(|a| a.def.active && a.def.sound_panning)
                .map(|a| a.x),
            #[cfg(feature = "3d")]
            Some(Kind::Actor3D) => self
                .actor3d(owner)
                .filter(|a| a.active && a.sound_panning)
                .map(|a| a.x),
            Some(Kind::Entity(r)) => self
                .entity_def(r)
                .filter(|e| e.active && e.sound_panning)
                .map(|e| e.x),
            _ => None,
        }?;
        // Scripts set both: no overflow on absurd values.
        Some(Sounds::pos_to_pan(
            x.saturating_sub(self.offset_x),
            self.settings.width as i32,
        ))
    }

    /// The speech sound of the sentence an object is saying.
    pub fn speech_sound_of(&self, owner: NativeId) -> Option<SoundId> {
        match self.objects.get(owner).map(|n| n.kind.clone())? {
            Kind::Actor => self.actor(owner)?.sentence.as_ref()?.sound,
            #[cfg(feature = "3d")]
            Kind::Actor3D => self.actor3d(owner)?.sentence.as_ref()?.sound,
            Kind::Entity(r) => self.view_of(r)?.speech_sound(r),
            _ => None,
        }
    }

    /// Centres an object's sound and the speech of its sentence
    /// (`CAdObject::ResetSoundPan`): what turning `SoundPanning` off
    /// does.
    pub(crate) fn reset_sound_pan(&mut self, owner: NativeId) {
        let speech = self.speech_sound_of(owner);
        let own = self.sfx_of(owner).map(|s| s.id);
        for id in speech.into_iter().chain(own) {
            self.sounds.audio.set_pan(id, 0);
        }
    }

    /// Plays the sound of a sprite frame that became current
    /// (`CBFrame::OneTimeDisplay`): a sound of its own beside the
    /// object's `PlaySound` one, given the object's pan when it pans its
    /// sounds and the object's effect (`CBObject::UpdateOneSound`) as it
    /// starts, dropped when it ends.
    pub(crate) fn play_frame_sound(&mut self, owner: NativeId, file: &str) {
        let Some((clip, _)) = self.load_clip(file) else {
            return;
        };
        let id = self.sounds.audio.add(clip, SoundType::Sfx, 100);
        if let Some(pan) = self.sound_pan_of(owner) {
            self.sounds.audio.set_pan(id, pan);
        }
        if let Some(effect) = self.sounds.objects.get(&owner).and_then(|s| s.effect) {
            self.sounds.audio.set_effect(id, Some(effect.into()));
        }
        self.sounds.audio.play(id, false);
        self.sounds.frame_sounds.entry(owner).or_default().push(id);
    }

    /// Stops the frame sounds of an object's sprite
    /// (`CBSprite::KillAllSounds`): on `Reset`, and when a frame marked
    /// `KILL_SOUND` becomes current.
    pub(crate) fn kill_frame_sounds(&mut self, owner: NativeId) {
        for id in self.sounds.frame_sounds.remove(&owner).unwrap_or_default() {
            self.sounds.audio.remove(id);
        }
    }

    /// The frame sounds of an object still playing.
    pub fn frame_sounds_of(&self, owner: NativeId) -> usize {
        self.sounds
            .frame_sounds
            .get(&owner)
            .map_or(0, |ids| ids.len())
    }

    /// Forgets the frame sounds that ended.
    fn drop_ended_frame_sounds(&mut self) {
        let audio = &mut self.sounds.audio;
        self.sounds.frame_sounds.retain(|_, ids| {
            ids.retain(|id| {
                // A sound the freeze holds has not ended.
                if audio.is_active(*id) {
                    true
                } else {
                    audio.remove(*id);
                    false
                }
            });
            !ids.is_empty()
        });
    }

    /// One frame of the objects' sounds (`CBObject::UpdateSounds`,
    /// `CAdObject::UpdateSounds`), for the objects the engine updates
    /// (the game, the active actors and entities): the sound event fires
    /// when the object's sound has ended, the object's sound and the
    /// speech of its sentence follow its position on screen and take its
    /// effect; then the frame sounds that ended are forgotten. A sound
    /// held by a frozen game has not ended: the engine, which only asks
    /// whether the sound is playing, fired the event at the freeze; that
    /// defect is not reproduced.
    pub fn update_sounds(&mut self) {
        self.update_crossfade();
        let owners: Vec<NativeId> = self.sounds.objects.keys().copied().collect();
        for owner in owners {
            if !self.updates_sounds(owner) {
                continue;
            }
            let Some(sound) = self.sfx_of(owner).map(|s| s.id) else {
                continue;
            };
            let audio = &self.sounds.audio;
            if !audio.is_playing(sound) && !audio.is_freeze_paused(sound) {
                let event = self
                    .sounds
                    .objects
                    .get_mut(&owner)
                    .and_then(|s| s.event.take());
                if let Some(event) = event {
                    self.queue_event(owner, event);
                }
            }
            if let Some(pan) = self.sound_pan_of(owner) {
                self.sounds.audio.set_pan(sound, pan);
            }
        }
        // The speech of the sentences being said.
        let mut speakers: Vec<NativeId> = self
            .actors
            .iter()
            .filter(|(_, a)| a.sentence.as_ref().is_some_and(|s| s.sound.is_some()))
            .map(|(id, _)| *id)
            .collect();
        #[cfg(feature = "3d")]
        speakers.extend(
            self.actors3d
                .iter()
                .filter(|(_, a)| a.sentence.as_ref().is_some_and(|s| s.sound.is_some()))
                .map(|(id, _)| *id),
        );
        let talking: Vec<crate::scene_view::EntityRef> = self
            .scene
            .iter()
            .chain([&self.game_view])
            .flat_map(|view| view.speaking_entities())
            .collect();
        for r in talking {
            speakers.extend(
                self.objects
                    .find(|k| matches!(k, Kind::Entity(e) if *e == r)),
            );
        }
        for owner in speakers {
            if !self.updates_sounds(owner) {
                continue;
            }
            let Some(sound) = self.speech_sound_of(owner) else {
                continue;
            };
            if let Some(pan) = self.sound_pan_of(owner) {
                self.sounds.audio.set_pan(sound, pan);
            }
            let effect = self.sounds.objects.get(&owner).and_then(|s| s.effect);
            self.sounds
                .audio
                .set_effect(sound, effect.map(SoundEffect::from));
        }
        self.drop_ended_frame_sounds();
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
            .add_streamed(clip, SoundType::Music, private_volume);
        let start = std::mem::take(&mut self.sounds.music_start_time[channel]);
        if start != 0 {
            self.sounds.audio.set_position_ms(id, start);
        }
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
    /// when asked. Nothing moves while the game is frozen (the clock
    /// goes on, so a fade whose time passed during the freeze ends at
    /// the unfreeze).
    fn update_crossfade(&mut self) {
        let Some(fade) = self.sounds.crossfade.clone() else {
            return;
        };
        if self.ui.frozen {
            return;
        }
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
            // The engine truncates each volume on its own.
            let progress = elapsed as f32 / fade.length as f32 * 100.0;
            self.sounds
                .audio
                .set_private_volume(first, (100.0 - progress) as i32);
            self.sounds
                .audio
                .set_private_volume(second, progress as i32);
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
        let id = self.sounds.audio.add_streamed(clip, SoundType::Speech, 100);
        self.sounds.audio.play(id, false);
        Some((id, length))
    }

    /// Ends the sentences whose speech sound has ended: neither playing,
    /// nor paused by a script, nor held by a frozen game (`TimeIsUp` in
    /// `CAdActor::Update`, `CAdEntity::Update`, `CAdActor3DX::Update`),
    /// and unloads their sounds. A sentence with a sound lasts exactly
    /// as long as the sound, whatever its duration. In the engine a
    /// sound the frozen game holds is simply not playing, so opening a
    /// menu during a line ended the sentence of an actor or an entity
    /// and cut its speech; that defect is not reproduced: the speech is
    /// held with the game and goes on at the unfreeze.
    pub(crate) fn update_speech(&mut self) {
        let mut finished: Vec<SoundId> = Vec::new();
        let audio = &self.sounds.audio;
        let ended = |id: SoundId| {
            !audio.is_playing(id) && !audio.is_paused(id) && !audio.is_freeze_paused(id)
        };
        for (_, actor) in &mut self.actors {
            let talking = actor.is_talking();
            if let Some(sentence) = &mut actor.sentence {
                if let Some(id) = sentence.sound {
                    let ended = ended(id);
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
        #[cfg(feature = "3d")]
        if !self.ui.frozen {
            for (_, actor) in &mut self.actors3d {
                let talking = actor.is_talking();
                if let Some(sentence) = &mut actor.sentence {
                    if let Some(id) = sentence.sound {
                        let ended = ended(id);
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
        }
        let views = self.scene.iter_mut().chain([&mut self.game_view]);
        for view in views {
            for sentence in view.sentences_mut() {
                if let Some(id) = sentence.sound {
                    if ended(id) {
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
        #[cfg(feature = "3d")]
        for (_, actor) in &mut self.actors3d {
            if let Some(sentence) = &mut actor.sentence {
                if sentence.duration == 0 {
                    stopped.extend(sentence.sound.take());
                }
            }
        }
        let views = self.scene.iter_mut().chain([&mut self.game_view]);
        for view in views {
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
        // `CBSoundMgr::PosToPan`: five screens of margin on each side.
        assert_eq!(Sounds::pos_to_pan(400, 800), 0);
        assert_eq!(Sounds::pos_to_pan(0, 800), -909);
        assert_eq!(Sounds::pos_to_pan(800, 800), 909);
        assert_eq!(Sounds::pos_to_pan(-4000, 800), -10000);
        assert_eq!(Sounds::pos_to_pan(4800, 800), 10000);
        // Farther than the margin the value leaves DirectSound's range.
        assert!(Sounds::pos_to_pan(-4800, 800) < -10000);
        assert!(Sounds::pos_to_pan(5600, 800) > 10000);
        assert_eq!(Sounds::pos_to_pan(10, 0), 0);
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
