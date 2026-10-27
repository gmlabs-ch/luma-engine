# Game packages (`.dcp`)

A Wintermute Engine game ships its data in one or more *packages*: flat
archives with the `.dcp` extension, written by the engine's project manager
when a game is published. This document describes the on-disk format and how
the engine resolves files across packages and loose files.

Sources: the MIT-licensed [WME Lite](https://github.com/MnemonicWME/wmelite)
and [Wintermute Engine 1.x](https://github.com/MnemonicWME/wintermute-engine)
sources (`dcpackage.h`, `BFileManager.cpp`, `BPkgFile.cpp`, `BDiskFile.cpp`
and the project manager's `PackageBuilder.cpp`), and analysis of game files.

Implemented by the `luma-archive` crate.

## Conventions

- All integers are **little-endian**.
- `u8`, `u32`: unsigned integers of 1 and 4 bytes.
- `bool`: one byte, `0` = false, anything else = true.
- "Pascal string": a `u8` length followed by that many bytes. The length
  **includes a terminating NUL byte**, so a package named `data` is stored as
  `05 'd' 'a' 't' 'a' 00`. A length of `0` means "no string". Readers should
  stop at the first NUL rather than trust the length, since some old
  project manager versions wrote padding after it.
- Strings are 8-bit, in the Windows code page of the machine that built the
  package. Almost all names are plain ASCII.

## Layout

```
+---------------------------+  offset 0
| Header (128 bytes)        |
+---------------------------+  offset 128
| DirOffset (u32)           |  version 2 only
+---------------------------+  offset 132
| File data                 |  raw or zlib-compressed blobs, back to back
|  ...                      |
+---------------------------+  offset DirOffset
| Directory table           |
|   NumDirs directories,    |
|   each with its entries   |
+---------------------------+  end of file
```

Version 1 packages (`PackageVersion` below `0x200`) have no `DirOffset`:
the directory table starts immediately after the header, at offset 128, and
file data follows it.

## Header

| Offset | Size | Field            | Description |
|-------:|-----:|------------------|-------------|
|      0 |    4 | `Magic1`         | `0xDEC0ADDE`, the generic WME file magic (bytes `DE AD C0 DE`). |
|      4 |    4 | `Magic2`         | `0x4B4E554A`, the ASCII bytes `JUNK`. |
|      8 |    4 | `PackageVersion` | Format version. `0x200` for every package built since WME 1.x; the engine refuses newer versions and warns on older ones. |
|     12 |    4 | `GameVersion`    | Free-form version number set by the game's author in the project manager. |
|     16 |    1 | `Priority`       | Package priority, see [Resolution](#resolution). |
|     17 |    1 | `CD`             | Disc number the package lives on; `0` means the hard disk. Overridden to `0` by the engine unless `MasterIndex` is set. |
|     18 |    1 | `MasterIndex`    | `bool`. Set on a *master index* package, which lists the contents of several packages (one directory each) spread over several discs. Never set on a regular package. |
|     19 |    1 | *(padding)*      | Alignment byte, unused. |
|     20 |    4 | `CreationTime`   | Package build time as a 32-bit Unix `time_t`. |
|     24 |  100 | `Desc`           | Human-readable description, NUL-padded (at most 99 characters). |
|    124 |    4 | `NumDirs`        | Number of directories in the directory table. `1` for a regular package. |

The header is a packed C structure (`TPackageHeader`), 128 bytes in the
32-bit Windows build that writes packages. Portable readers must read the
fields one by one and must not rely on the host's `sizeof`.

Version 2 adds a `u32` `DirOffset` right after the header: the absolute
offset of the directory table, measured from the start of the package. In
the usual layout, file data starts at offset 132 and the directory table
comes last, so a package can be built in one pass.

## Directory table

Each of the `NumDirs` directories is laid out as:

| Size | Field        | Description |
|-----:|--------------|-------------|
|  var | `Name`       | Pascal string. Name of the package (the project folder it was built from, for example `data`). Not obfuscated. |
|    1 | `CD`         | Disc number of this directory's package; only meaningful in a master index. |
|    4 | `NumEntries` | `u32`. Number of file entries that follow. |

The engine uses the package's *file name* (without `.dcp`) rather than this
stored name when it needs to reopen the package, because the two can differ.

## File entries

Each entry describes one file:

| Size | Field        | Description |
|-----:|--------------|-------------|
|  var | `Name`       | Pascal string, **XOR-obfuscated** in version 2 (see below). Path of the file relative to the package root, with Windows `\` separators, for example `scenes\room\room.scene`. |
|    4 | `Offset`     | `u32`. Offset of the file's data from the start of the package. |
|    4 | `Length`     | `u32`. Uncompressed size in bytes. |
|    4 | `CompLength` | `u32`. Size of the data on disk when compressed, or `0` when the file is stored as is. |
|    4 | `Flags`      | `u32`. Always `0` in packages built by the project manager. Reserved. |
|    4 | `TimeDate1`  | `u32`, version 2 only. Last-write time of the source file as a 32-bit Unix `time_t`. |
|    4 | `TimeDate2`  | `u32`, version 2 only. Always `0`, unused. |

### Name obfuscation

In version 2, every byte of the entry name, **including its terminating
NUL**, is XORed with `0x44` (ASCII `'D'`). A decoded name therefore ends with
a NUL byte, which readers strip; on disk the last byte of the string is
`0x44`. Directory names are not obfuscated.

### Compression

A file is compressed when `CompLength` is not `0`. The data is a complete
**zlib stream** (RFC 1950, as produced by zlib's `compress()`), inflating to
exactly `Length` bytes. The project manager only keeps the compressed form
when it is smaller than the original and the file is not excluded from
compression by a packager filter; otherwise the file is stored verbatim with
`CompLength = 0`.

The engine streams compressed files with `inflate()`; seeking backwards
restarts the stream from the beginning.

## Where packages are found

At startup the engine scans every file matching `*.dcp` in its package
search paths:

1. the game's directory (`./`);
2. the `data` sub-directory;
3. on macOS, the application bundle and its `Contents/Resources` folder;
4. any extra paths listed under `PackagePaths` in the `[Resource]` section
   of the game's `wme.ini` or registry settings.

A package whose file name starts with `xdevice_` is device-specific
(`xdevice_iphone.dcp`, `xdevice_ipad.dcp`, ...) and is only loaded by the
matching device type.

A package can also be **bound to the game executable**: the package bytes
are appended to the `.exe`, and the engine locates them by scanning the
executable for the 8-byte signature `DE AD C0 DE 4A 55 4E 4B` (`Magic1`
followed by `Magic2`), starting one megabyte into the file. Every offset in
such a package is relative to the start of the embedded header.

## Resolution

The engine builds a single table mapping **upper-cased** entry names to
their location. Lookups are case-insensitive and accept both `/` and `\`
as separators. While registering packages, when two entries share a name,
the later one replaces the earlier one only if its package has a **lower
`CD` number** or a **higher `Priority`**; on a tie, the first registered
entry wins. Packages are registered in directory listing order, which the
engine leaves to the operating system.

When a file is requested, the engine looks first for a **loose file** on
disk, in the game directory and in the paths listed under `CustomPaths`,
then in the package table. Loose files therefore always shadow packaged
ones, which is how patches and translations are commonly distributed.

### Compressed loose files

A loose file may itself be zlib-compressed, in a small container the engine
also uses for save games:

| Offset | Size | Field        | Description |
|-------:|-----:|--------------|-------------|
|      0 |    4 | `Magic1`     | `0xDEC0ADDE`. |
|      4 |    4 | `Magic2`     | `0x504D435A`, the ASCII bytes `ZCMP`. |
|      8 |    4 | `DataOffset` | Offset of the zlib stream, normally `20`. |
|     12 |    4 | `CompSize`   | Size of the zlib stream. |
|     16 |    4 | `UncompSize` | Size of the inflated data. |

Any other loose file is read as is.

## Notes for implementers

- Entry names must be treated as opaque bytes for matching (upper-cased
  ASCII, separators normalised) and only decoded for display or extraction.
  When extracting, convert `\` to the host separator and reject names with
  `..` components.
- `Offset + CompLength` (or `+ Length` when stored) must not exceed the
  package size; `Length` of a compressed entry must match the inflated size.
- `NumDirs`, `NumEntries` and the Pascal string lengths are trusted by the
  original engine; a reader should bound them by the file size.
