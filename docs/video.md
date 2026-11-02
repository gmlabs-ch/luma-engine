# Video playback

How Luma Engine plays the videos a game asks for with `Game.PlayTheora`
(full-frame cut-scenes) and `Entity.PlayTheora` (a video in an entity's
place): Ogg files holding a Theora picture stream and, usually, a Vorbis
sound track. The rules follow the original engine (`VidTheoraPlayer.cpp`,
`VidSubtitler.cpp`, `BGame.cpp` and `AdEntity.cpp` of Wintermute Engine
1.x, MIT) so that intros and cut-scenes behave as their authors expect.

Implemented by the `luma-video` core crate (demuxing, decoding,
colour conversion) and `luma_adventure::video` (playback, timing,
skipping, subtitles, drawing).

## Decoding

`luma_video::Video::open` reads an Ogg file from memory: the first
packets of each logical stream say what they carry (`\x80theora`,
`\x01vorbis`); the three Theora header packets set up the decoder and
give the picture size (`picw` × `pich`) and the frame rate. Every data
packet of the Theora stream then yields one frame (`next_frame`); a
zero-length packet repeats the previous frame, as the format says.

The container is read with the [`ogg`](https://crates.io/crates/ogg)
crate (BSD-3-Clause) and the pictures decoded with
[`oxideav-theora`](https://crates.io/crates/oxideav-theora) (MIT, pure
Rust, with `oxideav-core`). The decoder was checked pixel-exact against
ffmpeg's native Theora decoder on streams with key and predicted frames
from 64×48 to 320×240; it decodes 320×240 at several hundred frames per
second in release builds.

Frames come out as opaque RGBA. The YUV to RGB conversion is the
engine's own: 13-bit fixed-point BT.601 tables (`Y' = 1.164 (Y − 16)`,
`R = Y' + 1.596 V'`, `G = Y' − 0.391 U' − 0.813 V'`, `B = Y' + 2.018 U'`
with `U' = U − 128`, `V' = V − 128`), the chroma of a 4:2:0 frame shared
by each 2×2 block. Studio white comes out as 254 with these tables, as in
the engine.

The sound track is not decoded here: `luma_audio::decode` reads the
Vorbis stream from the same bytes (Symphonia skips the Theora stream),
and the player plays it as a sound effect.

`luma_video::synthetic_clip` encodes a short clip in code (the same
crate's encoder, muxed with `ogg`) for tests and demos: no video file is
needed in the repository.

## `Game.PlayTheora`

```
Game.PlayTheora(filename, type, x, y, freezeMusic, dropFrames, subtitleFile)
```

| Argument | Default | Meaning |
|----------|---------|---------|
| `filename` | | The `.ogg` or `.ogv` file, through the game's packages. |
| `type` | `1` | `0` (`VID_PLAY_POS`): at `(x, y)` at 100 %; `1` (`VID_PLAY_STRETCH`): scaled to fit the screen, keeping the aspect ratio, centred; `2` (`VID_PLAY_CENTER`): centred at 100 %. Other values mean `1`. |
| `x`, `y` | `0` | The position for `VID_PLAY_POS`. |
| `freezeMusic` | `true` | Whether the music pauses with the game. |
| `dropFrames` | `true` | Whether late frames are skipped to keep up with the sound. |
| `subtitleFile` | | A `.sub` file; without one, the video's name with `.sub`. |

The call returns `true` when the file opened, `false` otherwise (the
missing file is logged and the script goes on).

`Game.PlayVideo` is the deprecated older name: in the original engine it
played **AVI** files through DirectShow and whatever codecs the Windows
machine had. Luma decodes Ogg Theora only: `PlayVideo` plays an Ogg file
given to it as `PlayTheora` does (same arguments, no subtitle file, the
deprecation warning logged), and refuses an `.avi` file with a log line
(`Game.PlayVideo: AVI files are not supported`) and `false`, so that the
script goes on as after a missing file. A game that ships AVI videos
needs them converted to Ogg Theora; the file name in the script stays
the same if the `.ogg` keeps it.

While a video plays:

- the game is **frozen** (`Game.Freeze`, see
  [formats/window.md](formats/window.md)): the freezable scripts and the
  sounds pause (the music too when `freezeMusic`), the timer stops; the
  video runs on the wall clock;
- the scripts do not run at all, not even the non-freezable ones: the
  script that called `PlayTheora` resumes after the video ends, so a
  `ChangeScene` written after the call happens when the video is over;
- only the video is drawn, with its subtitles: no scene, no windows, no
  cursor;
- a click skips the video, with the button `VIDEO_SKIP_BUTTON` of
  `default.game` names (`left`, the default, `right` or `both`), and so
  does the `Escape` key;
- the video's sound plays at the sound effects' volume.

When the video ends, or is skipped, the game unfreezes and the frame
after shows the scene again.

### Timing

Playback starts on the first frame decoded (and, with a sound track,
when its first block is ready). The **movie time** is the sound's
position when there is one, else the wall clock since the start. Each
frame is shown when its time (`index × fps_den / fps_num`) is reached;
frames whose time has already passed are decoded but not shown, unless
`dropFrames` is `false`, in which case every frame is shown in turn.

## `Entity.PlayTheora`

```
entity.PlayTheora(filename, looping, alphaFile, startTime)
```

| Argument | Default | Meaning |
|----------|---------|---------|
| `filename` | | The `.ogg` or `.ogv` file. |
| `looping` | `false` | Whether the video starts again at its end. |
| `alphaFile` | | An image whose alpha channel masks the frames, pixel by pixel (the frame stays opaque outside the image). |
| `startTime` | `0` | Where to start in the video, in milliseconds. |

The video takes the place of the entity's sprite (`CAdEntity::Display`):
drawn at the entity's position (its top-left corner there, no hotspot),
in the entity's layer, scaled by the entity's `Scale` when it has one
(100 % otherwise, whatever the scene's scale levels), tinted with the
entity's `AlphaColor`. The entity keeps its hit area, its scripts and
its events. The game runs on: nothing freezes, the scripts go on and the
call returns at once (`true`, or `false` with a runtime error logged
when the file cannot be opened). The sound track plays as the entity's
sound would, at its `SoundVolume`.

The video advances on the **game timer**, so it stops while the game is
frozen by a system window or a cut-scene and resumes after; a paused
video (`PauseTheora`) holds its frame and its sound. When it ends, a
looping video starts again from its first frame (the sound with it);
another one is dropped and the entity shows its sprite again from the
next frame. `StopTheora` drops it at once. An entity in a window's
entity container plays its video the same way, at the container's
position.

| Method | Effect |
|--------|--------|
| `StopTheora()` | Stops and drops the video; `true` when there was one. |
| `IsTheoraPlaying()` | Whether a video plays and is not paused. |
| `PauseTheora()`, `ResumeTheora()` | Hold / release the video and its sound; `true` when the state changed. |
| `IsTheoraPaused()` | Whether the video is paused. |

Deviations from the original engine: the video is not kept in saved
games (the engine saves the player and reopens the file on load; after
a load in Luma the entity shows its sprite), and only one video plays
per entity, as there.

## Subtitles

A `.sub` file is a text file with one subtitle per line:

```
{12}{48}First line|second line
{60}{90}Another subtitle
```

The two numbers are the first and last **frame** of the subtitle; `|`
breaks the line; a subtitle whose end is `0` stays until the next one.
As in the engine, a subtitle shows from the end of the previous one
rather than from its own start (the first one from the start of the
video); the start frame only decides when the next subtitle takes over.
Lines without a start, or with `{1}{1}`, are ignored. The subtitles show
while `Game.VideoSubtitles` is on (`VIDEO_SUBTITLES` of `default.game`),
centred at the bottom of the screen, five pixels above the edge, with
the game's `VIDEO_FONT`, or its `SYSTEM_FONT` without one.

## Not yet

- The `.avi` player of the old `PlayVideo` (see above: by design).
- `SetTheoraTexture` on 3D actors, with the rest of the 3D module.
- An entity's video in saved games.
- The White Chamber's own videos (`assets\data0001.ogg` and the others)
  are not in the itch.io release used for testing; the engine logs the
  missing file and the game goes on to the next scene.
