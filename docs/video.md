# Video playback

How Luma Engine plays the videos a game asks for with `Game.PlayTheora`
(full-frame cut-scenes) and `Entity.PlayTheora` (a video in an entity's
place): Ogg files holding a Theora picture stream and, usually, a Vorbis
sound track; and with the older `Game.PlayVideo`: AVI files (see
[`Game.PlayVideo`](#gameplayvideo-avi-files)). The rules follow the
original engine (`VidTheoraPlayer.cpp`, `VidPlayer.cpp`, `BSoundAVI.cpp`,
`VidSubtitler.cpp`, `BGame.cpp` and `AdEntity.cpp` of Wintermute Engine
1.x, MIT) so that intros and cut-scenes behave as their authors expect.

Implemented by the `luma-video` core crate (demuxing, decoding,
colour conversion) and `luma_adventure::video` (playback, timing,
skipping, subtitles, drawing). Both kinds of video are a
`luma_video::FrameSource` to the player (frames in order, each with its
time), which shares everything between them but the clock: placement,
the texture, the subtitles, the freeze, skipping.

## Decoding

`luma_video::Video::open` reads an Ogg file from memory: the first
packets of each logical stream say what they carry (`\x80theora`,
`\x01vorbis`); the three Theora header packets set up the decoder and
give the picture size (`picw` × `pich`), the size of the coded frame
that holds it (multiples of 16), where the picture is in it, and the
frame rate. Every data
packet of the Theora stream then yields one frame (`next_frame`); a
zero-length packet repeats the previous frame, as the format says. The
streams may come in any order: in some files the sound's first page is
ahead of the picture's.

### Frame times

A frame is shown at the time of its **frame number**,
`number × fps_den / fps_num` seconds (the start of the frame's
interval; one period later for a stream older than bitstream 3.2.1, see
below), and the number comes from the granule positions of the Ogg
pages, not from counting packets:

- the last packet that ends on a page takes the frame number of the
  page's granule position (Theora specification, appendix A.2.3: the
  count of frames up to that packet, key frame count in the high bits
  and frames since the key frame in the low `KFGSHIFT` bits; one less
  is the frame's number, except before bitstream 3.2.1 where the
  position already held the number);
- any other packet comes one frame after the packet before it, the
  first one being frame 0;
- a position that marks no frame (all ones: no packet ends on the page;
  or a header's zero), that would go back in time, or that jumps more
  than 3600 frames ahead (two minutes at 30 frames per second: Luma's
  own bound, so that a damaged file cannot make the video wait without
  end) is ignored.

In a regular stream the two agree and frame `n` is the `n`th packet.
They differ in streams whose encoder was fed an irregular source and
left frames out without writing zero-length packets in their place:
the positions then skip the missing frames. Such a video played by
counting packets runs too fast (one of The White Chamber's has 57
packets spread over some 115 frame periods and played in half its
time). Two packets may also carry the same number: both are then due at
the same instant, and the later one takes the earlier one's place at
once.

**What the original engine does.** It asks its decoder library for the
time of the decoder's own granule position after each packet
(`theora_granule_time(&m_TheoraState, m_TheoraState.granulepos)` in
`CVidTheoraPlayer::DecodeTheora`). The library it links is libtheora
1.0 (`src/external_lib/libtheora` of the engine's repository, BSD
licence; read to settle this). In that version:

- the decoder **counts the packets** it is given, from 0
  (`th_decode_packetin`: `curframe_num`), and builds its position from
  that count alone. It never looks at the granule positions of the Ogg
  pages. A zero-length packet counts as a frame;
- `th_granule_time` is `(th_granule_frame + 1)` periods, and
  `th_granule_frame` takes one off for a stream of bitstream 3.2.1 or
  later. Since the decoder's position holds the packet's index, a
  packet of index `n` is timed at `n` periods, **the start of its
  interval**, in a 3.2.1 stream (every file a current encoder writes),
  and at `n + 1` periods, the end of its interval, in an older stream.

Luma follows the second point (`VideoInfo::shown_at_ms`). It does
**not** follow the first, on purpose: it numbers the frames from the
page positions as described above. In a regular stream both give the
same times. In a stream that leaves frames out without zero-length
packets, the original shows the pictures too early (it plays the White
Chamber video mentioned above in half its time, ending well before its
sound), where Luma keeps them on the stream's clock. This is a
deliberate difference, to be confirmed.

`Frame::index` still counts the packets; `Frame::time_ms` is the time
above.

### The comment header

The second header packet, the comment header (Theora specification,
section 6.3: a vendor string and `NAME=value` user comments, each with a
32-bit length), is not parsed. The specification asks for UTF-8 there,
but old encoders wrote Latin-1 text (a `©` in the encoder's name), which
the decoder crate refuses along with the whole stream. Nothing in the
comments is needed to decode or to play, so the decoder is given a bare
comment header instead (`\x81theora`, an empty vendor string, no
comment), whatever the stream's own holds, malformed lengths included;
`Video::headers` still returns the packet as the file has it. The
original engine's decoder library did not validate the text either
(inferred from such files playing there, not read from its sources).

### Pictures

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

### The sound track

The sound is not decoded here. `luma_video::sound_track` copies the
packets of the file's first Vorbis stream into an Ogg file of its own,
on the same pages and with the same granule positions, and closes it
with an end-of-stream page; `luma_audio::decode` decodes that, and the
player plays it as a sound effect.

The copy is there because the sound decoder (Symphonia) reads the whole
container and reports an error at the end of a file whose streams were
never closed, which loses the whole track although every packet
decoded. Some game videos are like that (cut short, or written by a
muxer that never marked the last pages). The original engine feeds the
Vorbis packets to its decoder one by one and never looks for the mark
(`CVidTheoraPlayer::DecodeVorbis`). A file damaged or cut in the middle
of a page keeps the sound up to the damage.

The Vorbis comment header needs no such care as the Theora one:
Symphonia reads its text leniently.

### Test clips

`luma_video::synthetic_clip` encodes a short clip in code (the same
crate's encoder, muxed with `ogg`) for tests and demos: no video file is
needed in the repository. `synthetic_clip_at` gives the frames chosen
frame numbers, for a stream that leaves frames out.

`cargo test -p luma-video --test local_videos -- --ignored --nocapture`
decodes every video of a local test game (the directory named by
`LUMA_TEST_VIDEOS`, or the White Chamber source pack under
`testdata/local/`) and prints the number of frames, the time of the last
one and the length of the sound, to compare with `ffprobe`; it passes
when the files are absent.

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

`Game.PlayVideo` is the older call, for AVI files: see
[below](#gameplayvideo-avi-files).

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

### Placement

The engine sizes its texture after the **coded frame** of the video
(`theora_info.width` × `height`, multiples of 16), not after the picture
inside it, and places and zooms that (`CVidTheoraPlayer::Play`). Luma
does the same arithmetic, so a picture is where the original shows it:
a 640×360 video is coded as 640×368; stretched on a 1280×720 screen it
is zoomed 195.65 % (not 200 %) and centred as a 640×368 frame; centred,
it is placed as a 640×368 frame. The picture is then drawn at its place
in the coded frame (its offset, zoomed). The positions are truncated
towards zero as in the engine.

One difference, deliberate: the engine drew the whole coded frame, so
the padding rows and columns around the picture showed (up to 15 pixels
of whatever the encoder left there, usually a smear of the picture's
edge). Luma draws the picture only.

### Timing

Playback starts on the first frame decoded (and, with a sound track,
when its first block is ready). The **movie time** is the sound's
position when there is one, else the wall clock since the start. Each
frame is shown when its time is reached (the time of its frame number,
see [Frame times](#frame-times)); frames whose time has already passed
are decoded but not shown, unless `dropFrames` is `false`, in which case
every frame is shown in turn. The video ends on the step after its last
frame was shown.

A sound track is often a little shorter than the pictures (by half a
second in some videos). Once the sound has ended, the movie time goes on
on the clock from the sound's last position, so that the remaining
frames show at their times and the video ends. This is Luma's own rule:
the original engine plays the sound through a streaming buffer whose
behaviour at the end of the data was not read from its sources.

## `Game.PlayVideo`: AVI files

```
Game.PlayVideo(filename, type, x, y, freezeMusic, subtitleFile)
```

The video player of the engine before it had Theora (`CVidPlayer`),
kept for the games made then; the call logs
`Warning: Game.PlayVideo() is now deprecated. Use Game.PlayTheora() instead.`
each time, as in the engine. The arguments are those of `PlayTheora`
without `dropFrames`: the **sixth** is the subtitle file.

Everything said above of a video that plays holds here too: the game
freezes (the music with it when `freezeMusic`), no script runs until the
video is over, only the video is drawn, a click with the
`VIDEO_SKIP_BUTTON` or the `Escape` key skips it, `Game.IsVideoPlaying`
is true meanwhile and `Game.StopVideo` stops it. The call returns `true`
when the video started. What differs from the Theora player:

- **Placement** is by the picture's own size (`biWidth` × `biHeight`):
  an AVI has no coded frame around its picture. The three types and
  their arithmetic are the same.
- **Timing** (`CVidPlayer::Update`). The movie time is the **wall clock
  since the call, never the sound's position**. At each step the frame
  shown is the one of that instant, `time × rate / scale` rounded down
  (`AVIStreamTimeToSample`; the rounding is inferred, the function being
  Windows' own), whatever was shown before: after a long step the frames
  between are passed over. The video ends as soon as the time reaches
  the end of the picture stream (frames × scale / rate), whether or not
  the sound is over; the sound stops with it.
- **Subtitles.** The same `.sub` format, from the file given or the
  video's name with `.sub`, loaded when `Game.VideoSubtitles` is on at
  that moment; the subtitle's frame numbers are the AVI's frames. Two
  details of this player's own copy of the code are kept: a
  `{1}{1}text` line is a subtitle here (the Theora player drops it),
  and the subtitle is drawn even if `VideoSubtitles` is turned off
  during the video.
- **Sound.** The track plays as a sound effect, from the start of the
  video, without any tie to the pictures.
- A frame that cannot be decoded leaves the picture as it was (logged
  once), and the video goes on.

### Codecs

The original engine decoded nothing itself: Windows did, with the codecs
installed on the player's machine, so that a game's video played on one
machine and not on another. Luma decodes the AVI container
([formats/avi.md](formats/avi.md)) and these codecs itself:

| Pictures | FourCC |
|----------|--------|
| Uncompressed, 1 to 32 bits per pixel | `BI_RGB` (0), `BI_BITFIELDS` (3), `DIB `, `RGB `, `RAW ` |
| Microsoft RLE, 4 and 8 bits | `BI_RLE8` (1), `BI_RLE4` (2), `RLE `, `MRLE` |
| Motion JPEG | `MJPG`, `JPEG` |
| Microsoft Video 1, 8 and 16 bits | `CRAM`, `MSVC`, `WHAM` |
| Cinepak | `cvid` |

| Sound | Format tag |
|-------|-----------|
| PCM, 8 to 32 bits, mono or stereo | 0x0001 |
| Microsoft ADPCM | 0x0002 |
| IMA ADPCM | 0x0011 |

**Not decoded**, among the codecs games of those years shipped:

| Codec | FourCC or tag | Why |
|-------|---------------|-----|
| MPEG-4 part 2 (DivX, Xvid) | `DIVX`, `DX50`, `XVID`, `MP4V`, `FMP4` | No pure Rust decoder under a licence the engine can take (the known ones are LGPL or GPL). |
| Microsoft MPEG-4 | `MP42`, `MP43`, `DIV3`, `DIV4` | The same. |
| Intel Indeo | `IV31`, `IV32`, `IV41`, `IV50` | No public specification and no usable decoder. |
| H.264, WMV, others | `H264`, `WMV1` to `WMV3`, ... | Rare in AVI files of the time; no decoder in the engine. |
| MP3 | 0x0055 | No MP3 decoder is built into the audio engine today. |
| AC-3, WMA, Vorbis in AVI, others | 0x2000, 0x0160 to 0x0163, ... | No decoder. |

A video whose **picture** codec is not decoded does not play: the call
logs

```
Error: Unsupported AVI format (file 'intro.avi'): video codec 'DIVX' is not supported
```

and returns `false` at once, so the script goes on as after a video that
ended (the engine did the same on a machine without the codec). A video
whose **sound** codec is not decoded plays without sound, after

```
Error initializing sound buffer for AVI file 'intro.avi': unsupported codec: sound format tag 0x0055 (MP3)
```

(the engine, too, played on when its sound buffer could not be made).
A game with such videos needs them converted, to Ogg Theora (the script
then calls `PlayVideo` or `PlayTheora` on the new file, see below) or to
one of the codecs above.

### Differences from the original, deliberate

- **Which files play.** The engine played what the machine's codecs
  played; Luma plays the list above on every machine.
- **Sound codecs.** The engine gave the stream's format to DirectSound
  as it was, which takes PCM only (inferred from `CBSoundAVI`: the
  buffer is created with the stream's own format): a video with ADPCM
  sound played silent there. Luma decodes the two ADPCM codecs.
- **Sound volume.** The engine marked the AVI's sound as a sound effect
  but never set a volume on its buffer, so it played at full level
  whatever the player's sound effects volume. Luma plays it at the sound
  effects' volume.
- **Where the file is.** Video for Windows opened a file on the disk, so
  the engine could not play an AVI stored in a package. Luma reads the
  file through the game's file system, packages included.
- **An Ogg Theora file given to `PlayVideo`** failed in the engine. In
  Luma it plays, as with `PlayTheora` (frames dropped when late, the
  sixth argument the subtitle file), so that a game whose videos were
  converted needs no change to its scripts when the new file keeps the
  old name. The file is recognised by its content, not its extension.
- **Subtitle files with Unix line ends** lose no character (see
  [Subtitles](#subtitles)); the AVI player's parser had the same defect.
- The first sample of a stream that does not start at 0 (`dwStart`) is
  honoured as described in [formats/avi.md](formats/avi.md#frames-and-time);
  what Video for Windows did with such a file was not checked.

## `Entity.PlayTheora`

```
entity.PlayTheora(filename, looping, alphaFile, startTime)
```

| Argument | Default | Meaning |
|----------|---------|---------|
| `filename` | | The `.ogg` or `.ogv` file. |
| `looping` | `false` | Whether the video starts again at its end. |
| `alphaFile` | | An image whose alpha channel masks the frames, pixel by pixel (the frame stays opaque outside the image). No colour key applies (`SetAlphaImage`): an image without an alpha channel leaves the frames opaque. |
| `startTime` | `0` | Where to start in the video, in milliseconds. Playback starts at the first **key frame** at or after that time, and the sound from the same place (`SeekToTime`); see below. |

The video takes the place of the entity's sprite (`CAdEntity::Display`):
drawn at the entity's position (the top-left corner of its coded frame
there, see [Placement](#placement); no hotspot),
in the entity's layer, scaled by the `Scale` the entity has when
`PlayTheora` is called (100 % without one, whatever the scene's scale
levels; `CVidTheoraPlayer::Play` fixes the zoom, so a `Scale` changed
during the video does not rescale it), tinted with the entity's
`AlphaColor`. The entity keeps its hit area, its scripts and
its events. The game runs on: nothing freezes, the scripts go on and the
call returns at once (`true`, or `false` with a runtime error logged
when the file cannot be opened). The sound track plays as the entity's
sound would, at its `SoundVolume`.

The video advances on the **game timer**, so it stops while the game is
frozen by a system window or a cut-scene and resumes after; a paused
video (`PauseTheora`) holds its frame and its sound. When it ends, a
looping video starts again from its first frame (the sound with it;
`ResetStream`); another one is dropped and the entity shows its sprite again from the
next frame. `StopTheora` drops it at once. An entity in a window's
entity container plays its video the same way, at the container's
position.

| Method | Effect |
|--------|--------|
| `StopTheora()` | Stops and drops the video; `true` when there was one. |
| `IsTheoraPlaying()` | Whether a video plays and is not paused. |
| `PauseTheora()`, `ResumeTheora()` | Hold / release the video and its sound; `true` when the state changed. |
| `IsTheoraPaused()` | Whether the video is paused. |

A start time does not start in the middle of a group of pictures: the
engine reads on to the next key frame and takes that frame's time as
the start (`CVidTheoraPlayer::SeekToTime`, `m_SeekingKeyframe`), so the
video starts up to a key-frame interval later than asked. Luma starts
at the first key frame whose time is not before the start time. The
original is a little coarser, in ways that depend on the layout of the
file and are not reproduced: it looks for the first Ogg page whose last
frame reaches the time, drops what it had read of the file after that
page (it reads in blocks of 4096 bytes) and takes the first key frame
that follows; and it then counts time and subtitle frames from that
point as if the video began there.

A looping video keeps its sound volume and its subtitles from one
round to the next. (The original opened the file again at each round
and played it without a volume, `ResetStream`, so that from the second
round on the sound track had the sound effects' global percentage as
its own volume instead of the entity's `SoundVolume`: a defect, not
reproduced.)

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

Every video player has its subtitles (`CVidTheoraPlayer::Initialize`):
the game's video, from the file given to `PlayTheora` or the video's
name with `.sub`, and an entity's video, from the video's name with
`.sub`. They are loaded when the video opens, and only if
`Game.VideoSubtitles` is on at that moment (`VIDEO_SUBTITLES` of
`default.game`); turning it on during the video shows nothing, turning
it off hides them. They are drawn centred at the bottom of the screen,
five pixels above the edge, with the game's `VIDEO_FONT`, or its
`SYSTEM_FONT` without one; an entity's too, wherever the entity is
(Luma draws those after the scene, over it; the engine with the entity,
so that an object in front could cover them).

Each text goes through the string table (`CVidSubtitle`): a subtitle
written `/SUB0001/fallback` shows the translation of `SUB0001`. The text
is read in the game's encoding like any other text the font draws (see
[formats/font.md](formats/font.md#text-encoding)).

The file is read as the engine reads it (`CVidSubtitler::LoadSubtitles`):

- a line ends at a line feed (or a NUL byte); a carriage return before
  it is dropped, so Windows and Unix line ends read alike. (The
  original dropped the byte before the line end whatever it was, so a
  file with Unix line ends lost the last character of each line: a
  defect, not reproduced.)
- the numbers are read as C's `atoi` reads them: `{12a}` is 12, a number
  that is not one is 0;
- text in braces after the second pair is skipped; a `{` inside braces
  belongs to them; a `}` outside braces is text;
- a line is ignored when it has no first number, a first number of -1,
  no text, or `{1}{1}`.

## Not yet

- The AVI codecs listed as not decoded under
  [Codecs](#codecs); MP3 sound is the one most likely to be met.
- `SetTheoraTexture` on 3D actors, with the rest of the 3D module.
- An entity's video in saved games.
- The White Chamber's own videos (`assets\data0001.ogg` and the others)
  are not in the itch.io release: there the engine logs the missing file
  and the game goes on to the next scene. With the eight files of the
  game's source pack they all play, with the frame counts and durations
  the files state; their sound was checked by its decoded length, not
  by ear.
