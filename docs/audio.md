# Sound

How Luma Engine plays the sounds, music and speech of a game: the
`luma-audio` crate decodes and mixes, the adventure layer applies the
original engine's rules for who owns a sound and how loud it plays. The
rules follow WME Lite (`BSoundMgr`, `BSoundBuffer`, `BSound`,
`BObject`, `BGame`, `AdGame`, MIT) so that games sound as their authors
mixed them.

## Files

Sounds are WAV (PCM of 8 to 32 bits, or 32-bit float) or Ogg Vorbis
files, found through the game's packages like any other file. A `.wav`
named by a script or a file is replaced by the `.ogg` of the same name
when the game has one, as the engine does (games shipped WAV during
development and Ogg in release). A file is decoded once, in full, and
kept; the original engine streams large files, which Luma Engine does
not do yet.

## Output

`luma_audio::Audio` mixes the playing sounds with
[Kira](https://crates.io/crates/kira) (resampling, click-free volume and
pan changes, loop regions) and decodes them with
[Symphonia](https://crates.io/crates/symphonia), both pure Rust. With the
crate's `device` feature, the output goes to the default device through
`cpal` (on Linux that builds against ALSA). Without the feature, or
without a device (continuous integration, a headless run, `luma-player
--mute`), the sounds still play on Kira's mock backend, silently, and
advance with the game clock: a sentence timed by its speech file lasts
as long, `IsSoundPlaying` turns false at the same frame. Tests use this
backend.

## Volumes

Three global volumes, one per kind of sound (`SetGlobalSFXVolume`,
`SetGlobalSpeechVolume`, `SetGlobalMusicVolume`), and a master volume
(`SetGlobalMasterVolume`), all in percent and 100 by default. Each sound
also has its own volume (`SetSoundVolume` on its object,
`SetMusicChannelVolume`, the `PrivVolume` argument of `PlayMusic`, the
`SOUND_VOLUME` of a sound entity).

The gain of a sound is the engine's: the global volume of its kind is
weighted by the sound's own volume (`global / 100 * own`, in percent);
the remaining fraction is cubed and turned into an attenuation of up to
100 dB; that attenuation becomes the factor applied to the samples
(floored at Kira's silence of -60 dB). At 50 % a sound plays at about
24 % of its amplitude, at 75 % about 67 %. The master volume then
multiplies the result linearly, on the main mixer track.

## Panning

An object's sound follows the object across the screen when its
`SoundPanning` is on (the default, `SOUND_PANNING` in scene files): the
pan goes from full left half a screen left of the view to full right
half a screen right of it, with the centre of the screen in the middle.
The game's own sound (`Game.PlaySound`) is not panned.

## Objects' sounds

Every object (the game, actors, entities) owns at most one sound effect:

| Method | Effect |
|--------|--------|
| `LoadSound(file)` | Loads the sound without playing it. |
| `PlaySound(file, looping, loopStart)`, `PlaySound(looping, loopStart)` | Loads and plays, or plays the loaded sound again; `loopStart` is where a loop restarts, in milliseconds. |
| `PlaySoundEvent(file, event)`, `PlaySoundEvent(event)` | Plays and raises `event` on the object when the sound ends. |
| `StopSound()`, `PauseSound()`, `ResumeSound()` | What they say; a stopped sound stays loaded. |
| `IsSoundPlaying()` | Whether it plays (a sound paused by a system window counts as playing). |
| `SetSoundPosition(ms)`, `GetSoundPosition()` | Where it plays from: at once when playing, else when it next starts. |
| `SetSoundVolume(percent)`, `GetSoundVolume()` | The sound's own volume. |
| `SoundFXEcho(wetDryMix, feedback, leftDelay, rightDelay)`, `SoundFXReverb(inGain, reverbMix, reverbTime, highFreqRTRatio)`, `SoundFXNone()` | The effect applied to the object's sounds, now and next (see [Sound effects](#sound-effects)). |

A sound entity (`SUBTYPE="sound"`) loads its `SOUND` when the scene
loads, with `SOUND_START_TIME` and `SOUND_VOLUME`, and a script plays it
with `PlaySound()`. The sounds of a scene's objects go with the scene;
the game's and its actors' stay.

A sprite frame's `SOUND` (see [formats/sprite.md](formats/sprite.md))
plays when the frame becomes current, as a sound of its own beside the
object's `PlaySound` one (`CBFrame::OneTimeDisplay`): at full volume,
panned with the object's position when it pans its sounds, once. A
frame marked `KILL_SOUND` stops the sprite's earlier frame sounds when
it becomes current, and `Reset` stops them too (`CBSprite::
KillAllSounds`). Frame sounds that ended are forgotten each frame; they
are not kept in saved games.

## Sound effects

Every object keeps one effect for its sounds (`m_SFXType` and its four
parameters), set by `SoundFXEcho`, `SoundFXReverb` or cleared by
`SoundFXNone`: the engine applied it to the object's sound when the
sound was loaded and again every frame (`CBObject::UpdateOneSound`), so
a change takes effect on the sound playing. Luma applies it when a
sound is loaded and when the effect changes; the effect is kept in
saved games with the object's sound. The parameters are those of the
DirectSound effects the engine used (`DSFXEcho`, `DSFXWavesReverb`), with
the engine's defaults for arguments left out; Kira's delay and reverb
take them as follows:

| Engine parameter | Default | Kira |
|------------------|---------|------|
| Echo: wet/dry mix, 0-100 % | 0 | the delay's mix (`0` dry, `1` wet). |
| Echo: feedback, 0-100 % | 0 | the delay's feedback, as decibels of the fraction (`50` → −6 dB). |
| Echo: left and right delays, 1-2000 ms | 333 | one delay time, the mean of the two: Kira's delay has one line, not one per channel. |
| Reverb: input gain, −96-0 dB | 0 | the volume of the track the sound plays on. |
| Reverb: reverb mix, −96-0 dB | 0 | the reverb's mix, `wet / (1 + wet)` with `wet = 10^(dB/20)` (0 dB gives equal dry and wet). |
| Reverb: reverb time, 0.001-3000 ms | 1000 | the reverb's feedback: the decay by 60 dB in that time of a comb filter of 35 ms (Freeverb's tuning): 1000 ms → 0.785, 3000 ms → 0.92, 100 ms → 0.09, capped at 0.98. |
| Reverb: high-frequency ratio, 0.001-0.999 | 0.001 | the reverb's damping, `1 − ratio`. |

A sound with an effect plays on a sub-track of its own carrying the
effect; the volumes and the panning are the sound's, as without one.
Changing the parameters of an effect of the same kind applies in place;
switching between echo, reverb and none restarts the sound where it was
(paused when it was paused), as the engine pauses and resumes the buffer
around `SetFX`. The echo with one delay line and the reverb's mapping
are approximations of the DirectSound effects, not their algorithms.

## Music

Five channels (`Game.PlayMusicChannel(channel, file, looping,
loopStart, volume)`; `PlayMusic` and the other `...Music` methods use
channel 0). Music loops by default. `StopMusicChannel` unloads the
channel, `PauseMusicChannel` and `ResumeMusicChannel` hold it,
`SetMusicChannelPosition` moves it (or sets where the next music on the
channel starts), `GetMusicChannel` names its file, `IsMusicChannelPlaying`
and `GetMusicChannelPosition` report.

`MusicCrossfade(channel1, channel2, length, swap)` fades the first
channel out and the second in over `length` milliseconds on the wall
clock, starting what does not play; at the end the first stops, both
volumes go back to 100, and the channels swap places when `swap` is on
(the default). A second crossfade while one runs is an error.

## Speech

`Talk(text, sound, duration, ...)` plays `sound` with the sentence. With
no sound given and a text of the form `/ID/fallback`, the engine looks
for `ID.ogg` or `ID.wav` in the speech directories (`AddSpeechDir`,
`RemoveSpeechDir`; `speech\` by default). The sound plays as speech,
the sentence lasts the sound when no duration is given, and ends when
the sound does; skipping the sentence stops the sound. See
[formats/font.md](formats/font.md) for the rest of `Talk`.

## Not yet

- Streaming of long files (Kira can stream; the engine decodes in full
  for now); the `.talk` files timing talk sprites to a sound.
- Pausing every sound while a system window is open (no windows yet).
