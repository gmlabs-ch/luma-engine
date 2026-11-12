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

At startup the engine (`CBFileManager::InitPaths`, `RegisterPackages`)
scans every file matching `*.dcp` in its package search paths, in this
order:

1. the game's directory (`./`);
2. the paths listed under `PackagePaths` in the `[Resource]` section of
   the game's `wme.ini` (or of the registry), separated by `;`;
3. the `data` sub-directory.

WME Lite adds the macOS application bundle and device-specific packages
(`xdevice_iphone.dcp`...); Wintermute 1.x, which Luma follows, has
neither.

A file that cannot be opened, or whose header is not a package's (wrong
magic numbers, a version above `0x200`), is logged and **ignored**: the
game starts with the other packages. A version below `0x200` is accepted
with a warning. `Vfs::open_game` does the same and keeps the lines in
`Vfs::warnings`.

A package can also be **bound to the game executable**: the package bytes
are appended to the `.exe`, and the engine locates them by scanning the
executable for the 8-byte signature `DE AD C0 DE 4A 55 4E 4B` (`Magic1`
followed by `Magic2`), starting one megabyte into the file. Every offset in
such a package is relative to the start of the embedded header. The bound
package is registered **last**, after the `.dcp` files. The original scans
its own executable, which is the game's; Luma scans every `*.exe` of the
game directory.

### Where a package's data is read from

The engine does not keep the file it read a directory from. To read an
entry it opens `<name>.dcp`, `name` being the **name stored in the
directory record**, in the first package search path that has such a file
(`CBFileManager::OpenPackage`), and reads at the entry's offset there.
This is how a master index works: its directories carry the names of the
other packages, and its entries point into their files. It also means
that the original cannot read a package whose file was renamed.

Luma does the same (`Vfs::add_package`), with one addition: when no file
has the directory's name, the data is read from the file the directory
came from, so a renamed package still works. A bound package is always
read from its executable.

### Paths of `wme.ini`

`CustomPaths` (loose files) and `PackagePaths` (packages) are read from
the `[Resource]` section of `wme.ini` in the game directory. Luma only
accepts paths that stay inside the game directory; an absolute path or
one that climbs out with `..` is refused and reported: the file comes
with the game and must not make the engine read elsewhere.

## Resolution

The engine builds a single table mapping **upper-cased** entry names to
their location. Lookups are case-insensitive and accept both `/` and `\`
as separators. While registering packages, when two entries share a name,
the later one replaces the earlier one only if its package has a **lower
`CD` number** or a **higher `Priority`**; on a tie, the first registered
entry wins. Packages are registered in directory listing order, which the
engine leaves to the operating system.

When a file is requested (`CBFileManager::OpenFileRaw`), the engine
looks:

1. for a **loose file** on disk, in the paths listed under `CustomPaths`,
   then in the game directory. The `data` sub-directory is a directory of
   packages only: a loose file there is not found;
2. in the package table;
3. in its **built-in resources** (`CBResourceFile`): a few files linked
   into the executable, named like any other file. They are the fallback
   system font `system_font.fnt` with its image `system_font.bmp`, the
   placeholders `invalid.bmp` and `invalid_debug.bmp` shown for a missing
   image or sprite, the default shadow `shadow.png`, and the waypoint
   marker `wpt.sprite` with `wpt.bmp`. A resource name is matched whole,
   ignoring case, without any directory.

Loose files therefore always shadow packaged ones, which is how patches
and translations are commonly distributed, and a game can replace a
built-in file by shipping one of the same name.

Luma's built-in files have the same names and roles but are its own:
images generated in code (`luma-adventure/src/resources.rs`), the font
from a 5x7 dot-matrix alphabet drawn for Luma. The original's bitmaps are
not copied; they are in the engine's repository under its MIT licence,
but nothing there says where the font's glyphs come from.

A name is relative to the game directory. The original also opens an
absolute path, or one that leaves the game directory, straight from the
disk; Luma refuses both (a game's files are untrusted), and resolves a
`..` that stays inside the game directory.

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

Any other loose file is read as is. A container that cannot be inflated
is passed over: the engine goes on to the package table and the built-in
resources.

## Notes for implementers

- Entry names must be treated as opaque bytes for matching (upper-cased
  ASCII, separators normalised) and only decoded for display or extraction.
  When extracting, convert `\` to the host separator and reject names with
  `..` components.
- `Offset + CompLength` (or `+ Length` when stored) must not exceed the
  size of the file the data is read from, which for a master index is
  not the file that holds the directory: check it when the entry is read,
  before allocating. `Length` of a compressed entry must match the
  inflated size.
- `NumDirs`, `NumEntries` and the Pascal string lengths are trusted by the
  original engine; a reader should bound them by the file size.
