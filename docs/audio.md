# Sound

How Luma Engine plays the sounds, music and speech of a game: the
`luma-audio` crate decodes and mixes, the adventure layer applies the
original engine's rules for who owns a sound, when it plays and how loud.
The rules follow Wintermute Engine 1.x (`BSoundMgr`, `BSoundBuffer`,
`BSoundOGG`, `BSound`, `BObject`, `BGame`, `AdGame`, `AdObject`, MIT) so
that games sound and behave as their authors tested them. Where 1.x and
WME Lite differ (what a paused sound is, where a sound pans), 1.x wins.

## Files

Sounds are WAV (PCM of 8 to 32 bits, or 32-bit float) or Ogg Vorbis
files, found through the game's packages like any other file. A `.wav`
named by a script or a file is replaced by the `.ogg` of the same name
when the game has one, as the engine does (games shipped WAV during
development and Ogg in release). A file is decoded once, in full, and
kept; the original engine streams large files, which Luma Engine does
not do. It still knows which sounds the original streams, because only
those loop from a loop start (see [Looping](#looping)).

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

The gain of a sound is the engine's (`CBSoundBuffer::SetVolume`): the
global volume of its kind (kept as 0 to 200 for 0 to 100 %) times the
sound's own volume in percent, divided by 100 and truncated to a whole
number; the fraction that remains to 200 is cubed and multiplied by
-100 dB, truncated to a hundredth of a decibel. At 50 % a sound plays
12.5 dB down (about 24 % of its amplitude), at 75 % about 1.6 dB down.
Kira is silent from -60 dB, where DirectSound went on to -100 dB.

The engine does not clamp any of these, and neither does Luma, because
scripts read the values back:

- A global volume is a byte: `SetGlobalSFXVolume(150)` keeps 150
  (`GetGlobalSFXVolume` answers 150), `-1` is 255, 256 is 0.
- A value DirectSound refused leaves the loudness where it was: a
  global volume above 100, a sound's own volume above 100 (with a full
  global volume) or below 0 change the number read back and nothing
  heard. A sound whose first volume is refused plays at full volume.
- The master volume keeps the low 16 bits of 655 times the byte: 100 is
  65500, and 150 wraps to 32714, read back as 49 and heard at half
  volume. It multiplies the mix linearly, on the main mixer track (the
  original set the system's wave volume and put it back at exit).

## Panning

An object's sound follows the object across the screen when its
`SoundPanning` is on (the default, `SOUND_PANNING` in scene files). The
pan is the original's (`CBSoundMgr::PosToPan`), in DirectSound's unit,
hundredths of a decibel taken off the far channel while the near one is
left as it is: a margin of five screen widths on each side of the
screen, linear across the eleven widths from -10000 to +10000. The
centre of the screen is 0; an object at an edge of the screen has the
far channel 9 dB down; the far channel only falls silent five screens
away. Farther than that DirectSound refused the value, and the sound
keeps the pan it had. Kira pans with an equal-power law; Luma gives it
the panning that yields the same ratio between the channels and lowers
the sound by what that law adds to the near channel, so the two
channels come out as DirectSound's.

The position is the object's `X` less the scroll offset. The engine
runs this every frame for the game, the active actors and the active
entities (`UpdateSounds`): their own sound and the speech of the
sentence they are saying (`CAdObject::UpdateSounds`). The game itself
stands at the centre of the screen, so `Game.PlaySound` is centred
until the scene scrolls and then drifts with the scroll offset. A
sprite frame's sound takes its object's pan when it starts. Setting
`SoundPanning` to false centres the object's sound and its speech
(`ResetSoundPan`). The sounds of other objects (windows, items,
regions...) are never panned.

## Playing, pausing, resuming

These are the rules of `CBSound` and `CBSoundBuffer` in 1.x; several are
not what one would guess, and scripts were written against them.

- **Play always starts over.** `PlaySound()` on a sound that is playing
  or paused stops it and starts it again at its start position.
- **The start position stays.** `SetSoundPosition(ms)` on a sound that
  is playing moves it at once. On a sound that is not playing, the time
  becomes the sound's start position, used by the next play *and by
  every play after it*: the engine never clears it. Only a later
  `SetSoundPosition` given while the sound plays clears it (the engine
  applies an object's pending position at the next `PlaySound()`
  without a file, so that is when it happens). `StopSound` does not
  clear it either. `SetMusicPosition` before `PlayMusic` works the same
  way for the music loaded next.
- **Pause marks the sound** even when it is not playing: `PauseSound()`
  on a stopped or finished sound, then `ResumeSound()`, starts it from
  its beginning. `ResumeSound()` answers false and does nothing on a
  sound that `PauseSound()` did not pause.
- **A frozen game holds the playing sounds** (a system window, a
  video): `CBSoundMgr::PauseAll`, music included or not. A held sound is
  **not playing** for the engine, which reads the device's status
  (`CBSoundBuffer::IsPlaying`), and it is not paused either (that is the
  script's flag). So, while the game is frozen:
  - `IsSoundPlaying()` and `IsMusicPlaying()` answer false for held
    sounds;
  - an event waiting for a sound to end (`PlaySoundEvent`) fires when
    the game freezes;
  - a sentence with a speech sound ends (see [Speech](#speech));
  - `ResumeSound()` does not resume a held sound;
  - a sound played during the freeze plays.

  The unfreeze resumes what the freeze held (`ResumeAll`). A sound
  stopped in between keeps its mark and **starts again from its
  beginning** at the unfreeze; Luma reproduces this, since scripts can
  see it (`IsSoundPlaying`).
- **Saved games.** In the original a game saved while frozen (a save
  menu) records its held sounds as not playing and without their
  position, so after loading, the music the freeze held starts again
  from its beginning at the unfreeze. Luma does not reproduce this: a
  held sound is saved as playing, with its position, and goes on where
  it was. This is a deliberate difference, to be reviewed with the
  saved games.

## Looping

`PlaySound(file, looping, loopStart)` and `PlayMusic(file, looping,
loopStart)` give where a loop restarts, in milliseconds. In the original
only a **streamed** sound uses it (`LoadSegment` refills the stream from
the loop start); a sound held whole in a buffer loops whole, from 0. A
sound is streamed when both hold (`CBSoundBuffer::LoadFromFile`,
`CreateSoundBuffer`):

- the engine asks for it (an object's `PlaySound` or `LoadSound`
  sound, music, speech) or the file is larger than 1 MB (a sprite
  frame's sound);
- the sound is at least one streaming block long: 3 seconds / 16, that
  is 187.5 ms (`Game.SoundBufferSize` seconds in the original, which
  Luma does not read: always 3).

So a short effect played with a loop start loops whole. A loop start
past the end of the sound is taken as 0.

## Objects' sounds

Every object of the engine owns at most one sound effect
(`CBObject::m_SFX`): the game, the scene and its layers, regions and
waypoint groups, actors, entities, items, windows and their controls,
particle emitters, `new Object`s.

| Method | Effect |
|--------|--------|
| `LoadSound(file)` | Loads the sound without playing it. |
| `PlaySound(file, looping, loopStart)`, `PlaySound(looping, loopStart)` | Loads and plays, or plays the loaded sound again from its start position; `loopStart` is where a loop restarts, in milliseconds (see [Looping](#looping)). |
| `PlaySoundEvent(file, event)`, `PlaySoundEvent(event)` | Plays and raises `event` on the object when the sound is no longer playing. |
| `StopSound()` | Stops the sound and unloads it: `PlaySound()` without a file then fails, `GetSoundVolume` reads the object's volume. Only a sound entity (`SUBTYPE="sound"`) keeps its sound loaded. False when the object has no sound. |
| `PauseSound()`, `ResumeSound()` | See [Playing, pausing, resuming](#playing-pausing-resuming). |
| `IsSoundPlaying()` | Whether it plays; false for a sound a frozen game holds. |
| `SetSoundPosition(ms)`, `GetSoundPosition()` | Where it plays from: at once when playing, else when it next starts; the position read is 0 when it is not playing. |
| `SetSoundVolume(percent)`, `GetSoundVolume()` | The sound's own volume. |
| `SoundFXEcho(wetDryMix, feedback, leftDelay, rightDelay)`, `SoundFXReverb(inGain, reverbMix, reverbTime, highFreqRTRatio)`, `SoundFXNone()` | The effect applied to the object's sounds, now and next (see [Sound effects](#sound-effects)). |

The sound event belongs to the object, not to the sound: a `PlaySound`
clears it, a `LoadSound` or a `StopSound` leaves it waiting. Only the
objects the engine updates every frame raise it, follow their position
and take a changed effect on the sound they have: the game, the active
actors and the active entities. The other objects' sounds just play
(their effect is applied when a sound is loaded).

A sound entity (`SUBTYPE="sound"`) loads its `SOUND` when the scene
loads, with `SOUND_START_TIME` and `SOUND_VOLUME`, and a script plays it
with `PlaySound()`. The sounds of a scene's objects go with the scene;
the game's and its actors' stay.

A sprite frame's `SOUND` (see [formats/sprite.md](formats/sprite.md))
plays when the frame becomes current, as a sound of its own beside the
object's `PlaySound` one (`CBFrame::OneTimeDisplay`): at full volume,
with the object's pan when it pans its sounds and the object's sound
effect, both as they are when it starts, once. A
frame marked `KILL_SOUND` stops the sprite's earlier frame sounds when
it becomes current, and `Reset` stops them too (`CBSprite::
KillAllSounds`). Frame sounds that ended are forgotten each frame; they
are not kept in saved games.

## Sound effects

Every object keeps one effect for its sounds (`m_SFXType` and its four
parameters), set by `SoundFXEcho`, `SoundFXReverb` or cleared by
`SoundFXNone`: the engine applied it to the object's sound when the
sound was loaded and again every frame (`CBObject::UpdateOneSound`), so
a change takes effect on the sound playing, and on the speech of the
sentence the object is saying. Luma applies it when a sound is loaded,
when the effect changes and, for speech, every frame; the effect is
kept in saved games with the object's sound. The parameters are those of the
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
clock, starting what does not play (from its start: a channel a script
paused starts over); at the end the first stops, both volumes go back
to 100, and the channels swap places when `swap` is on (the default). A
second crossfade while one runs is an error. Nothing moves while the
game is frozen (`CBGame::UpdateMusicCrossfade`); the clock goes on, so
the fade jumps ahead at the unfreeze.

## Speech

`Talk(text, sound, duration, ...)` plays `sound` with the sentence. With
no sound given and a text of the form `/ID/fallback`, the engine looks
for `ID.ogg` or `ID.wav` in the speech directories (`AddSpeechDir`,
`RemoveSpeechDir`; `speech\` by default). The sound plays as speech,
panned with the object and with the object's sound effect. A sentence
with a sound ends when the sound is neither playing nor paused by a
script, whatever duration was given (`TimeIsUp` in `CAdActor::Update`
and `CAdEntity::Update`); skipping the sentence stops the sound.
Because a sound held by a frozen game is not playing, **a freeze ends
the sentences that have a speech sound** (opening a system window while
an actor speaks cuts the line), except those of 3D actors, which the
engine does not update while frozen. In the original the cut speech
then starts again from its beginning at the unfreeze, with no sentence
on screen, until the object next speaks; Luma unloads the speech with
its sentence and does not reproduce that. See
[formats/font.md](formats/font.md) for the rest of `Talk`.

## Not yet

- Streaming of long files (Kira can stream; the engine decodes in full);
  the `.talk` files timing talk sprites to a sound.
- `Game.SoundBufferSize` (the length of the streaming buffer) is not
  read; `GetSoundPosition` reports the position heard, where the
  original reports the read position of a streamed Ogg file, up to
  three seconds ahead.
- Pausing every sound while the game window is in the background.
