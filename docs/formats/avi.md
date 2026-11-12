# AVI videos (`.avi`)

The files `Game.PlayVideo` plays: a RIFF container holding one picture
stream and, usually, one sound stream. This note says what Luma reads of
the container and of each codec it decodes. How the video is then played
(placement, timing, subtitles, skipping) is in [video.md](../video.md).

AVI is not a Wintermute format. The original engine handed the file to
Windows (Video for Windows: `AVIFileOpen`, `AVIStreamGetFrame`), which
decoded it with whatever codecs the machine had. Nothing here comes from
the engine's sources, then, but from public descriptions of the formats:

- the container: Microsoft's "AVI RIFF File Reference" and its
  descriptions of `BITMAPINFOHEADER` and `WAVEFORMATEX`; the OpenDML
  "AVI File Format Extensions" for the `AVIX` segments;
- uncompressed pictures and Microsoft RLE: Microsoft's bitmap
  documentation ("Bitmap Compression");
- Motion JPEG: the JPEG standard (ITU-T T.81, annex K.3 for the default
  Huffman tables) and Microsoft's "JPEG DIB Format" note;
- Microsoft Video 1 and Cinepak: the MultimediaWiki pages "Microsoft
  Video 1" and "Cinepak", and Tim Ferguson's "Cinepak (CVID) stream
  format for AVI and QT";
- Microsoft ADPCM and IMA ADPCM: Microsoft's "Multimedia Data Standards
  Update" and the IMA's "Recommended Practices for Enhancing Digital
  Audio Compatibility in Multimedia Systems".

No GPL or LGPL decoder was read. The decoders were then checked against
files made by another program (see [Checks](#checks)).

Implemented by `luma_video::AviFile` (the container), `luma_video::AviVideo`
(the frames) and the modules under `crates/luma-video/src/avi/`.

**An AVI file is untrusted.** Every size and offset is checked against
the bytes at hand, a chunk longer than its list is cut to it, the picture
is at most 8192 pixels a side and 16 million pixels in all, the video at
most a day long, and a damaged frame is an error for that frame alone.
The tests cut each kind of file at every length and change every byte of
it; nothing may panic.

## The container

A RIFF file is a tree of chunks: a four-character id, a 32-bit
little-endian size, then that many bytes, padded to an even length. A
`RIFF` or `LIST` chunk starts with a four-character type and holds
chunks.

```
RIFF 'AVI '
  LIST 'hdrl'
    'avih'                the main header
    LIST 'strl'           one per stream, in stream order
      'strh'              the stream header
      'strf'              the stream format
  LIST 'movi'
    '00dc' '01wb' ...     the data, one chunk per frame or block of sound
    LIST 'rec '           (optional) chunks grouped for reading
  'idx1'                  (optional) the index
RIFF 'AVIX'               (OpenDML) more segments, each with a LIST 'movi'
```

What is read:

| Chunk | Fields read |
|-------|-------------|
| `avih` | `dwMicroSecPerFrame` (the first field), used only when the video stream states no rate. |
| `strh` | `fccType` (`vids` or `auds`), `fccHandler`, `dwScale`, `dwRate`, `dwStart`, `dwLength`, `dwSampleSize`. |
| `strf` of a `vids` stream | A `BITMAPINFOHEADER`: `biWidth`, `biHeight` (negative: rows from the top), `biBitCount`, `biCompression`, `biClrUsed`; then the three colour masks of a `BI_BITFIELDS` format and, up to 8 bits per pixel, the colour table (blue, green, red, unused), `biClrUsed` entries or 2^bits. |
| `strf` of an `auds` stream | A `WAVEFORMATEX`: `wFormatTag`, `nChannels`, `nSamplesPerSec`, `nBlockAlign`, `wBitsPerSample`, and the `cbSize` bytes that follow. |
| `movi` | Every chunk whose id starts with the two digits (hexadecimal) of the first video stream or of the first audio stream; `rec ` lists are followed, four deep at most. A video chunk with the type `pc` is a palette change, any other is a frame. |
| `idx1` | Entries of 16 bytes: id, flags, offset, size. Only the flag `AVIIF_KEYFRAME` (0x10) is used, to mark the key frames. The offsets count from the `movi` type, or from the start of the file in some files: the first entry tells which (the chunk it names is looked for at both places). Without an index every frame counts as a key frame. |

Only the **first video stream and the first audio stream** are played,
as the engine asked for them (`AVIFileGetStream(..., 0)`); other streams
are ignored. `strd`, `strn`, `JUNK`, `INFO` and the OpenDML index chunks
(`indx`, `ix##`) are skipped: the frames are found by walking the `movi`
lists, which needs no index and also reads a file cut short.

### Frames and time

A frame is a chunk of the video stream in `movi`, in file order. The
stream has `dwRate / dwScale` frames per second; if either is 0, the
main header's `dwMicroSecPerFrame` gives the rate; with neither the file
is refused. Frame `n` is the stream's sample `dwStart + n`, and sample
`s` starts at `s × 1000 × dwScale / dwRate` milliseconds
(`AVIStreamSampleToTime`); the stream ends at the start of the sample
after the last frame (`AVIStreamEndTime`). A `dwStart` other than 0
makes the pictures start late.

The number of frames is the number of chunks found, not `dwLength` or
the main header's `dwTotalFrames`: a file cut short plays what it has.
(Inferred: Video for Windows builds its own table of the chunks; what it
reports for a header that disagrees with the file was not checked.)

A **chunk without data** is a frame the encoder dropped: the picture
before it stays. A **palette change** (`##pc`) holds the first entry, the
number of entries (0 means 256), two bytes of flags, then the entries as
red, green, blue, flags; it applies to the frames after it.

### Sound

The data of the audio stream's chunks, one after the other, is the
sound; where the chunks lie among the frames (the interleave) does not
matter, since the whole track is decoded when the video opens.
`dwStart` of the audio stream is ignored.

## Picture codecs

The codec is `biCompression`; `fccHandler` is only named in the log when
it differs.

| `biCompression` | Codec | Notes |
|-----------------|-------|-------|
| 0 (`BI_RGB`), `DIB `, `RGB `, `RAW ` | Uncompressed | 1, 4, 8, 16, 24 or 32 bits per pixel. |
| 3 (`BI_BITFIELDS`) | Uncompressed, with colour masks | 16 or 32 bits per pixel. |
| 1 (`BI_RLE8`), 2 (`BI_RLE4`), `RLE `, `MRLE` | Microsoft RLE | 8 or 4 bits per pixel. |
| `MJPG`, `JPEG` | Motion JPEG | The JPEG pictures the `image` crate decodes. |
| `CRAM`, `MSVC`, `WHAM` | Microsoft Video 1 | 8 or 16 bits per pixel. |
| `cvid` | Cinepak | Colour and grey streams. |

Anything else is refused with the codec's name:
`video codec 'DIVX' is not supported`. See
[video.md](../video.md#codecs) for the list of what games of the time
used and what is missing.

### Uncompressed

Rows are padded to four bytes and run from the **bottom** of the picture
unless `biHeight` is negative. Pixels: 1, 4 and 8 bits are indices into
the colour table (the high bits of a byte first); 16 bits are five bits
of red, green and blue (`x1r5g5b5`) or what the masks say; 24 bits are
blue, green, red; 32 bits are blue, green, red and an unused byte, or
what the masks say. A channel narrower than 8 bits is widened by
repeating its high bits (31 of 5 bits becomes 255). The last row may
lack its padding; a shorter chunk is an error.

### Microsoft RLE

Pairs of bytes, rows from the bottom:

| Bytes | Meaning |
|-------|---------|
| `n v`, `n > 0` | `n` pixels of colour `v`; with 4 bits per pixel, the two halves of `v` in turn, the high one first. |
| `0 0` | End of the row. |
| `0 1` | End of the picture. |
| `0 2 dx dy` | Move right by `dx` and up by `dy`. |
| `0 n`, `n >= 3` | `n` pixels as they are (one byte each, or half a byte each with 4 bits), padded to an even number of bytes. |

In a video a frame only writes the pixels that changed: the pixels a
move jumps over, and those after the end of the picture, keep the colour
the frame before gave them. Pixels outside the picture are dropped.

### Motion JPEG

Each frame is a JPEG picture, decoded by the `image` crate (its JPEG
decoder, `zune-jpeg`). Frames written for AVI usually leave out the
Huffman tables (`DHT`); the picture is then given the standard's default
tables (annex K.3) before its first scan, as the JPEG DIB format asks.
A picture of half the stream's height is one field of an interlaced
frame (the second field follows it in the chunk and is not read): each
of its rows is shown twice.

### Microsoft Video 1

The picture is cut into blocks of 4×4 pixels, read row by row from the
**bottom**, left to right; the four rows of a block also come from the
bottom. Each block starts with two bytes `a`, `b`:

| Depth | Bytes | Meaning |
|-------|-------|---------|
| both | `b` from 0x84 to 0x87 | `(b − 0x84) × 256 + a` blocks, this one included, stay as the frame before had them. |
| 8 | `b < 0x80` | Two colours follow. The 16 bits of `b:a` pick one per pixel, the low bit first; a set bit takes the first colour. |
| 8 | `b >= 0x90` | Eight colours follow, two per quarter of the block (bottom left, bottom right, top left, top right), picked by the same 16 bits. |
| 8 | otherwise | The block is filled with colour `a`. |
| 16 | `b < 0x80` | Two colours follow (16 bits each, low byte first). If the first has its top bit set, six more follow and the block has eight colours as above; else two. |
| 16 | otherwise | The block is filled with the colour `b:a`. |

The 8-bit colours are indices into the colour table; the 16-bit ones
five bits of red, green and blue. The data may end early: the blocks not
reached stay.

### Cinepak

All numbers are big-endian. A frame starts with a byte of flags, its
length (three bytes), the width, the height and the number of strips
(two bytes each); at most 32 strips are accepted. A strip has an id, its
size, and its top, left, bottom and right edges (two bytes each); a
strip whose top is 0 follows the strip before it, and its bottom is then
its height.

Each strip has two codebooks of 256 entries, `V1` and `V4`. An entry is
four luminance values (top left, top right, bottom left, bottom right)
and two signed chrominance values `u`, `v`; a pixel is

```
red = y + 2v    green = y − u/2 − v    blue = y + 2u
```

(`u/2` rounded down), each held to 0..255. Unless bit 0 of the frame's
flags is set, a strip starts with the codebooks of the strip before it;
the codebooks are kept from frame to frame.

The strip's chunks have an id and a size (two bytes each, the size
counting those four bytes):

| Id (high byte) | Meaning |
|----------------|---------|
| `0x20` to `0x27` | A codebook. Bit 1: the `V1` codebook, else `V4`. Bit 2: grey entries of four bytes (no chrominance), else six bytes. Bit 0: an update: a flag per entry says whether it is given. |
| `0x30` | The blocks of a key strip: a flag per 4×4 block says whether it has one `V1` entry (0) or four `V4` entries (1). |
| `0x31` | The blocks of a strip built on the last frame: a flag says whether the block changes; if so a second flag says `V1` or `V4`. |
| `0x32` | A strip of `V1` blocks only, one byte each. |

Flags come 32 at a time, the high bit first. A `V1` entry is stretched
over the block (each value covers 2×2 pixels); the four `V4` entries
each give a quarter of the block. Blocks run from the top of the strip,
left to right.

## Sound codecs

| `wFormatTag` | Codec | Notes |
|--------------|-------|-------|
| 0x0001 | PCM | 8, 16, 24 or 32 bits, one or two channels. |
| 0x0002 | Microsoft ADPCM | Decoded to 16 bits. |
| 0x0011 | IMA ADPCM | Decoded to 16 bits. |
| 0xFFFE | Extensible | Read as the format its sub-format names. |

The sound is handed to the audio engine as a WAV file of PCM samples
(`AviFile::sound_track`). Any other tag is refused with its number:
`sound format tag 0x0055 (MP3)`; the video then plays without sound.

**Microsoft ADPCM.** A block (`nBlockAlign` bytes) starts, per channel,
with the number of a predictor (one byte each), then the step, the
sample before the codes and the one before that (16 bits each, all the
channels' steps first, then their samples). The predictors are pairs of
coefficients: the seven standard ones, or those the format lists after
its `wSamplesPerBlock` and count. The two samples are output, the older
first; then each 4-bit code (the high half of a byte first, the channels
in turn) gives a sample:

```
predicted = (sample1 × c1 + sample2 × c2) / 256
sample    = predicted + code × step          (code signed, −8..7)
step      = max(16, table[code] × step / 256)
```

with `table` = 230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512,
409, 307, 230, 230, 230.

**IMA ADPCM.** A block starts, per channel, with a sample (16 bits), a
step index and an unused byte. Then come groups of four bytes per
channel, each holding eight codes, the low half of a byte first. For a
code, with `step` the entry of the standard table of 89 steps at the
index:

```
difference = step/8 + (step if bit 2) + (step/2 if bit 1) + (step/4 if bit 0)
sample     = sample − difference if bit 3, else sample + difference
index      = index + (−1, −1, −1, −1, 2, 4, 6, 8)[code & 7]
```

the sample held to 16 bits and the index to 0..88.

## Checks

The tests build their AVI files in code (`luma_video::build_avi`,
`synthetic_avi`), with hand-written streams for each codec.

The decoders were also compared with another decoder on files made for
that (not in the repository): with `LUMA_TEST_AVI` naming a directory,

```
cargo test -p luma-video --test local_avi -- --ignored --nocapture
```

decodes every `.avi` in it and, when `x.avi.rgb24` and `x.avi.s16le` lie
next to `x.avi` (ffmpeg's decode of the pictures and of the sound),
prints the differences. On clips encoded by ffmpeg from synthetic
sources, Luma's output was **identical** for uncompressed 8, 15, 24 and
32 bits, Microsoft RLE 8 bits, Microsoft Video 1 16 bits, PCM and both
ADPCM codecs (mono and stereo), within 1 of 255 for Cinepak (one and
three strips) and within 3 for Motion JPEG (a different JPEG decoder).

Not compared with another decoder, for want of an encoder: Microsoft
Video 1 with 8 bits per pixel, Microsoft RLE with 4 bits, uncompressed
pictures of 1 and 4 bits, grey Cinepak, palette changes, interlaced
Motion JPEG. They are covered by hand-written tests only.
