//! The sound methods of the native objects: the sound effect every
//! object can play, and the game's music channels, volumes and speech
//! directories.
//!
//! Written with reference to the `ScCallMethod` methods of `BObject.cpp`,
//! `BGame.cpp`, `AdEntity.cpp` and `AdGame.cpp` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.

use luma_audio::SoundType;
use luma_script::{NativeId, Return, Value};

use crate::sound::{MUSIC_CHANNELS, SfxEffect};
use crate::world::World;

impl World {
    /// The sound methods every object has (`CBObject`); `None` for other
    /// names.
    pub(crate) fn object_sound_call(
        &mut self,
        id: NativeId,
        name: &str,
        args: &[Value],
    ) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        Some(match name {
            "LoadSound" => {
                let file = arg(0).get_string(self);
                Return::value(self.play_sfx(id, Some(&file), false, false, None, 0))
            }
            "PlaySound" => {
                // PlaySound(file, looping, loopStart) or PlaySound(looping, loopStart).
                let (file, looping, loop_start) = match arg(0) {
                    Value::Bool(looping) => (None, looping, arg(1).get_int(self)),
                    Value::Null => (None, arg(1).get_bool(self), arg(2).get_int(self)),
                    v => (
                        Some(v.get_string(self)),
                        !arg(1).is_null() && arg(1).get_bool(self),
                        arg(2).get_int(self),
                    ),
                };
                let loop_start = u64::try_from(loop_start).unwrap_or(0);
                Return::value(self.play_sfx(id, file.as_deref(), looping, true, None, loop_start))
            }
            "PlaySoundEvent" => {
                // PlaySoundEvent(file, event) or PlaySoundEvent(event).
                let (file, event) = if arg(1).is_null() {
                    (None, arg(0).get_string(self))
                } else {
                    (Some(arg(0).get_string(self)), arg(1).get_string(self))
                };
                Return::value(self.play_sfx(id, file.as_deref(), false, true, Some(event), 0))
            }
            "StopSound" => {
                // `StopSFX()` deletes the sound; only a sound entity
                // keeps it loaded (`CAdEntity::ScCallMethod`).
                let keep = matches!(
                    self.native(id).map(|n| n.kind.clone()),
                    Some(crate::objects::Kind::Entity(r))
                        if self.entity_def(r).is_some_and(|e| {
                            e.kind == luma_formats::scene::EntityKind::Sound
                        })
                );
                Return::value(self.stop_sfx(id, !keep))
            }
            "PauseSound" => Return::value(self.pause_sfx(id)),
            "ResumeSound" => Return::value(self.resume_sfx(id)),
            "IsSoundPlaying" => Return::value(self.is_sfx_playing(id)),
            "SetSoundPosition" => {
                let ms = u64::try_from(arg(0).get_int(self)).unwrap_or(0);
                Return::value(self.set_sfx_time(id, ms))
            }
            "GetSoundPosition" => Return::value(self.sfx_position(id) as i32),
            "SetSoundVolume" => {
                let percent = arg(0).get_int(self);
                Return::value(self.set_sfx_volume(id, percent))
            }
            "GetSoundVolume" => Return::value(self.sfx_volume(id)),
            "SoundFXNone" => {
                self.set_sfx_effect(id, None);
                Return::null()
            }
            "SoundFXEcho" | "SoundFXReverb" => {
                // The engine's defaults for the arguments left out.
                let param = |world: &World, n: usize, default: f32| match arg(n) {
                    Value::Null => default,
                    v => v.get_float(world) as f32,
                };
                let effect = if name == "SoundFXEcho" {
                    SfxEffect::Echo {
                        wet_dry_mix: param(self, 0, 0.0),
                        feedback: param(self, 1, 0.0),
                        left_delay_ms: param(self, 2, 333.0),
                        right_delay_ms: param(self, 3, 333.0),
                    }
                } else {
                    SfxEffect::Reverb {
                        in_gain_db: param(self, 0, 0.0),
                        reverb_mix_db: param(self, 1, 0.0),
                        reverb_time_ms: param(self, 2, 1000.0),
                        high_freq_rt_ratio: param(self, 3, 0.001),
                    }
                };
                self.set_sfx_effect(id, Some(effect));
                Return::null()
            }
            _ => return None,
        })
    }

    /// The game's music, volume and speech methods (`CBGame`, `CAdGame`);
    /// `None` for other names.
    pub(crate) fn game_sound_call(&mut self, name: &str, args: &[Value]) -> Option<Return> {
        let arg = |n: usize| args.get(n).cloned().unwrap_or(Value::Null);
        let channel_of = |world: &World, name: &str, plain: &str| -> (usize, usize) {
            // The channel form takes the channel first; the plain form
            // uses channel 0 and keeps the argument positions.
            if name == plain {
                (0, 0)
            } else {
                (
                    usize::try_from(arg(0).get_int(world)).unwrap_or(MUSIC_CHANNELS),
                    1,
                )
            }
        };
        Some(match name {
            "PlayMusic" | "PlayMusicChannel" => {
                let (channel, at) = channel_of(self, name, "PlayMusic");
                let file = arg(at).get_string(self);
                let looping = arg(at + 1).is_null() || arg(at + 1).get_bool(self);
                let loop_start = u64::try_from(arg(at + 2).get_int(self)).unwrap_or(0);
                let volume = if arg(at + 3).is_null() {
                    100
                } else {
                    arg(at + 3).get_int(self)
                };
                Return::value(self.play_music(channel, &file, looping, loop_start, volume))
            }
            "StopMusic" | "StopMusicChannel" => {
                let (channel, _) = channel_of(self, name, "StopMusic");
                Return::value(self.stop_music(channel))
            }
            "PauseMusic" | "PauseMusicChannel" => {
                let (channel, _) = channel_of(self, name, "PauseMusic");
                Return::value(self.pause_music(channel))
            }
            "ResumeMusic" | "ResumeMusicChannel" => {
                let (channel, _) = channel_of(self, name, "ResumeMusic");
                Return::value(self.resume_music(channel))
            }
            "GetMusic" | "GetMusicChannel" => {
                let (channel, _) = channel_of(self, name, "GetMusic");
                Return::value(
                    self.music(channel)
                        .map(|m| m.filename.clone())
                        .map_or(Value::Null, Value::from),
                )
            }
            "SetMusicPosition" | "SetMusicChannelPosition" | "SetMusicPositionChannel" => {
                let (channel, at) = channel_of(self, name, "SetMusicPosition");
                let ms = u64::try_from(arg(at).get_int(self)).unwrap_or(0);
                Return::value(self.set_music_start_time(channel, ms))
            }
            "GetMusicPosition" | "GetMusicChannelPosition" => {
                let (channel, _) = channel_of(self, name, "GetMusicPosition");
                Return::value(self.music_position(channel) as i32)
            }
            "IsMusicPlaying" | "IsMusicChannelPlaying" => {
                let (channel, _) = channel_of(self, name, "IsMusicPlaying");
                Return::value(self.is_music_playing(channel))
            }
            "SetMusicVolume" | "SetMusicChannelVolume" => {
                let (channel, at) = channel_of(self, name, "SetMusicVolume");
                let percent = arg(at).get_int(self);
                Return::value(self.set_music_volume(channel, percent))
            }
            "GetMusicVolume" | "GetMusicChannelVolume" => {
                let (channel, _) = channel_of(self, name, "GetMusicVolume");
                Return::value(self.music_volume(channel))
            }
            "MusicCrossfade" => {
                let channel1 = usize::try_from(arg(0).get_int(self)).unwrap_or(MUSIC_CHANNELS);
                let channel2 = usize::try_from(arg(1).get_int(self)).unwrap_or(MUSIC_CHANNELS);
                let length = u64::try_from(arg(2).get_int(self)).unwrap_or(0);
                let swap = arg(3).is_null() || arg(3).get_bool(self);
                if self.is_crossfading() {
                    self.report_script_error(
                        "Runtime error. Game.MusicCrossfade: Music crossfade is already in progress.",
                    );
                    return Some(Return::value(false));
                }
                Return::value(self.music_crossfade(channel1, channel2, length, swap))
            }
            "GetSoundLength" => {
                let file = arg(0).get_string(self);
                Return::value(self.sound_length(&file) as i32)
            }
            "SetGlobalSFXVolume" | "SetGlobalSpeechVolume" | "SetGlobalMusicVolume" => {
                let kind = match name {
                    "SetGlobalSFXVolume" => SoundType::Sfx,
                    "SetGlobalSpeechVolume" => SoundType::Speech,
                    _ => SoundType::Music,
                };
                let percent = arg(0).get_int(self);
                self.sounds.audio.set_volume(kind, percent);
                Return::null()
            }
            "SetGlobalMasterVolume" => {
                let percent = arg(0).get_int(self);
                self.sounds.audio.set_master_volume(percent);
                Return::null()
            }
            "GetGlobalSFXVolume" => Return::value(self.sounds.audio.volume(SoundType::Sfx)),
            "GetGlobalSpeechVolume" => Return::value(self.sounds.audio.volume(SoundType::Speech)),
            "GetGlobalMusicVolume" => Return::value(self.sounds.audio.volume(SoundType::Music)),
            "GetGlobalMasterVolume" => Return::value(self.sounds.audio.master_volume()),
            "AddSpeechDir" => {
                let dir = arg(0).get_string(self);
                Return::value(self.sounds.add_speech_dir(&dir))
            }
            "RemoveSpeechDir" => {
                let dir = arg(0).get_string(self);
                Return::value(self.sounds.remove_speech_dir(&dir))
            }
            _ => return None,
        })
    }
}

impl World {
    /// The sound class of a volume property, `None` for the master volume.
    fn volume_kind(name: &str) -> Option<SoundType> {
        match name {
            "SFXVolume" => Some(SoundType::Sfx),
            "SpeechVolume" => Some(SoundType::Speech),
            "MusicVolume" => Some(SoundType::Music),
            _ => None,
        }
    }

    /// `Game.SFXVolume`, `SpeechVolume`, `MusicVolume`, `MasterVolume`
    /// read: the global volume in percent. The original engine warns at
    /// every access that the property form is obsolete; Luma warns once,
    /// when the property is written.
    pub(crate) fn volume_property(&self, name: &str) -> Value {
        Value::Int(match World::volume_kind(name) {
            Some(kind) => self.sounds.audio.volume(kind),
            None => self.sounds.audio.master_volume(),
        })
    }

    /// The volume properties written.
    pub(crate) fn set_volume_property(&mut self, name: &str, percent: i32) {
        self.log_once(&format!("**Warning** The {name} attribute is obsolete"));
        match World::volume_kind(name) {
            Some(kind) => self.sounds.audio.set_volume(kind, percent),
            None => self.sounds.audio.set_master_volume(percent),
        }
    }
}
