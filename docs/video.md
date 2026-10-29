# Video playback

How Luma Engine plays the videos a game asks for with `Game.PlayTheora`:
Ogg files holding a Theora picture stream and, usually, a Vorbis sound
track. The rules follow the original engine (`VidTheoraPlayer.cpp`,
`VidSubtitler.cpp` and `BGame.cpp` of Wintermute Engine 1.x, MIT) so
that intros and cut-scenes behave as their authors expect.

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
missing file is logged and the script goes on). `Game.PlayVideo` is the
deprecated older name and does the same (without the subtitle argument).

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

- Looping videos, `SeekToTime`, alpha-masked videos and the `.avi`
  player of the old `PlayVideo`.
- The White Chamber's own videos (`assets\data0001.ogg` and the others)
  are not in the itch.io release used for testing; the engine logs the
  missing file and the game goes on to the next scene.
