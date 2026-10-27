//! A single `.dcp` package.
//!
//! The on-disk layout implemented here is documented in `docs/formats/dcp.md`
//! and was established from the MIT-licensed WME Lite and Wintermute Engine
//! 1.x sources (Copyright (c) 2003-2013 Jan Nedoma and contributors). See
//! `NOTICE` at the root of the repository.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use flate2::read::ZlibDecoder;

use crate::error::{Error, Result};

/// First magic number of a package (and of other WME binary files).
pub const MAGIC_1: u32 = 0xDEC0_ADDE;
/// Second magic number of a package: the ASCII bytes `JUNK`.
pub const MAGIC_2: u32 = 0x4B4E_554A;
/// Format version written by every WME 1.x release. Older versions are read
/// too; newer ones are refused.
pub const CURRENT_VERSION: u32 = 0x0000_0200;

/// Size of the fixed header.
const HEADER_LEN: u64 = 128;
/// Length of the `Desc` field.
const DESCRIPTION_LEN: usize = 100;
/// Key used to obfuscate entry names in version 2 packages.
const NAME_XOR: u8 = b'D';
/// Offset at which the engine starts looking for a package bound to an
/// executable.
const EMBEDDED_SEARCH_START: u64 = 1024 * 1024;

/// Fixed header of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// Format version, normally [`CURRENT_VERSION`].
    pub version: u32,
    /// Free-form version number set by the game's author.
    pub game_version: u32,
    /// Priority between packages: higher wins.
    pub priority: u8,
    /// Disc number, `0` for the hard disk.
    pub cd: u8,
    /// Whether this is a master index listing several packages.
    pub master_index: bool,
    /// Build time, as a 32-bit Unix timestamp.
    pub creation_time: u32,
    /// Human-readable description.
    pub description: String,
}

/// One directory of the directory table. A regular package has exactly one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    /// Stored package name, usually the project folder it was built from.
    pub name: String,
    /// Disc number of this directory's package.
    pub cd: u8,
    /// The files listed in this directory.
    pub entries: Vec<Entry>,
}

/// One file inside a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Name as stored, with `\` separators and the original case.
    pub name: String,
    /// Normalised name used for lookups, see [`normalize_name`].
    key: String,
    /// Absolute offset of the data in the underlying file.
    pub offset: u64,
    /// Uncompressed size.
    pub length: u32,
    /// Size on disk when compressed, `0` when stored.
    pub compressed_length: u32,
    /// Reserved flags, `0` in practice.
    pub flags: u32,
    /// Last-write time of the source file as a 32-bit Unix timestamp, `0`
    /// when unknown (version 1 packages).
    pub modified: u32,
}

impl Entry {
    /// Normalised name: upper-cased ASCII with `/` separators.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Name with `/` separators, suitable for building host paths.
    pub fn path_name(&self) -> String {
        self.name.replace('\\', "/")
    }

    /// Whether the data is zlib-compressed on disk.
    pub fn is_compressed(&self) -> bool {
        self.compressed_length != 0
    }

    /// Number of bytes the entry occupies in the package.
    pub fn stored_length(&self) -> u32 {
        if self.is_compressed() {
            self.compressed_length
        } else {
            self.length
        }
    }

    /// Last-write time of the source file, when recorded.
    pub fn modified_time(&self) -> Option<SystemTime> {
        (self.modified != 0)
            .then(|| SystemTime::UNIX_EPOCH + Duration::from_secs(u64::from(self.modified)))
    }
}

/// Normalises a file name the way the engine does for lookups: ASCII
/// upper-case, and `\` turned into `/`.
pub fn normalize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c == '\\' {
                '/'
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect()
}

trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// An open package: its parsed directory table and a handle to read data.
pub struct Package {
    label: String,
    header: Header,
    directories: Vec<Directory>,
    source: Mutex<Box<dyn ReadSeek>>,
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package")
            .field("label", &self.label)
            .field("header", &self.header)
            .field("directories", &self.directories)
            .finish_non_exhaustive()
    }
}

impl Package {
    /// Opens a `.dcp` file.
    pub fn open(path: impl AsRef<Path>) -> Result<Package> {
        let path = path.as_ref();
        let file = File::open(path)?;
        Self::from_reader_at(file, path.display().to_string(), 0)
    }

    /// Opens a package bound to an executable: scans `path` for the package
    /// signature, starting one megabyte into the file, and parses the
    /// package found there.
    pub fn open_embedded(path: impl AsRef<Path>) -> Result<Package> {
        let path = path.as_ref();
        let label = path.display().to_string();
        let mut file = File::open(path)?;
        let base = find_signature(&mut file)?.ok_or_else(|| Error::NotAPackage {
            source: label.clone(),
        })?;
        Self::from_reader_at(file, label, base)
    }

    /// Parses a package from any seekable reader. `label` names the source
    /// in error messages.
    pub fn from_reader<R>(reader: R, label: impl Into<String>) -> Result<Package>
    where
        R: Read + Seek + Send + 'static,
    {
        Self::from_reader_at(reader, label.into(), 0)
    }

    fn from_reader_at<R>(mut reader: R, label: String, base: u64) -> Result<Package>
    where
        R: Read + Seek + Send + 'static,
    {
        let file_len = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(base))?;
        let mut parser = Parser {
            reader: BufReader::new(&mut reader),
            label: &label,
            base,
            file_len,
        };
        let (header, directories) = parser.parse()?;
        Ok(Package {
            label,
            header,
            directories,
            source: Mutex::new(Box::new(reader)),
        })
    }

    /// Path or label the package was opened from.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The fixed header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The directory table.
    pub fn directories(&self) -> &[Directory] {
        &self.directories
    }

    /// Every entry of every directory, in package order.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.directories.iter().flat_map(|d| d.entries.iter())
    }

    /// Finds an entry by name, ignoring case and separator style.
    pub fn find(&self, name: &str) -> Option<&Entry> {
        let key = normalize_name(name);
        self.entries().find(|e| e.key == key)
    }

    /// Reads and, when needed, decompresses an entry.
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        let stored = {
            let mut source = self
                .source
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            source.seek(SeekFrom::Start(entry.offset))?;
            let mut stored = vec![0; entry.stored_length() as usize];
            source.read_exact(&mut stored)?;
            stored
        };
        if !entry.is_compressed() {
            return Ok(stored);
        }
        let declared = u64::from(entry.length);
        let mut data = Vec::new();
        ZlibDecoder::new(stored.as_slice())
            .take(declared + 1)
            .read_to_end(&mut data)
            .map_err(|err| Error::Decompress {
                name: entry.name.clone(),
                reason: err.to_string(),
            })?;
        if data.len() as u64 != declared {
            return Err(Error::Decompress {
                name: entry.name.clone(),
                reason: format!("inflated to {} bytes, expected {declared}", data.len()),
            });
        }
        Ok(data)
    }

    /// Reads a file by name.
    pub fn read_file(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self.find(name).ok_or_else(|| Error::NotFound {
            name: name.to_string(),
        })?;
        self.read(entry)
    }
}

/// Scans a file for the 8-byte package signature, starting one megabyte in,
/// and returns the offset of the signature if found.
fn find_signature<R: Read + Seek>(reader: &mut R) -> io::Result<Option<u64>> {
    const SIGNATURE: [u8; 8] = [0xDE, 0xAD, 0xC0, 0xDE, b'J', b'U', b'N', b'K'];
    const CHUNK: usize = 32 * 1024;

    let len = reader.seek(SeekFrom::End(0))?;
    if len <= EMBEDDED_SEARCH_START {
        return Ok(None);
    }
    reader.seek(SeekFrom::Start(EMBEDDED_SEARCH_START))?;
    let mut window: Vec<u8> = Vec::with_capacity(CHUNK + SIGNATURE.len());
    let mut window_start = EMBEDDED_SEARCH_START;
    let mut chunk = vec![0; CHUNK];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        window.extend_from_slice(&chunk[..read]);
        if let Some(pos) = window.windows(SIGNATURE.len()).position(|w| w == SIGNATURE) {
            return Ok(Some(window_start + pos as u64));
        }
        // Keep the tail so a signature straddling two chunks is still found.
        let keep = window.len().min(SIGNATURE.len() - 1);
        let drop = window.len() - keep;
        window.drain(..drop);
        window_start += drop as u64;
    }
}

struct Parser<'a, R: Read + Seek> {
    reader: BufReader<R>,
    label: &'a str,
    base: u64,
    file_len: u64,
}

impl<R: Read + Seek> Parser<'_, R> {
    fn parse(&mut self) -> Result<(Header, Vec<Directory>)> {
        let magic1 = self.u32()?;
        let magic2 = self.u32()?;
        if magic1 != MAGIC_1 || magic2 != MAGIC_2 {
            return Err(Error::NotAPackage {
                source: self.label.to_string(),
            });
        }
        let version = self.u32()?;
        if version > CURRENT_VERSION {
            return Err(Error::UnsupportedVersion {
                source: self.label.to_string(),
                version,
            });
        }
        let game_version = self.u32()?;
        let priority = self.u8()?;
        let cd = self.u8()?;
        let master_index = self.u8()? != 0;
        let _padding = self.u8()?;
        let creation_time = self.u32()?;
        let description = c_string(&self.bytes(DESCRIPTION_LEN)?);
        let num_dirs = self.u32()?;
        let header = Header {
            version,
            game_version,
            priority,
            cd,
            master_index,
            creation_time,
            description,
        };

        let v2 = version == CURRENT_VERSION;
        if v2 {
            let dir_offset = u64::from(self.u32()?);
            if dir_offset < HEADER_LEN + 4 || self.base + dir_offset >= self.file_len {
                return Err(self.corrupt(format!("directory offset {dir_offset} out of range")));
            }
            self.reader.seek(SeekFrom::Start(self.base + dir_offset))?;
        }

        let mut directories = Vec::new();
        for _ in 0..num_dirs {
            let name = self.pascal_string(0)?;
            let cd = self.u8()?;
            let num_entries = self.u32()?;
            let mut entries = Vec::new();
            for _ in 0..num_entries {
                entries.push(self.entry(v2)?);
            }
            directories.push(Directory { name, cd, entries });
        }
        Ok((header, directories))
    }

    fn entry(&mut self, v2: bool) -> Result<Entry> {
        let name = self.pascal_string(if v2 { NAME_XOR } else { 0 })?;
        let offset = u64::from(self.u32()?);
        let length = self.u32()?;
        let compressed_length = self.u32()?;
        let flags = self.u32()?;
        let modified = if v2 {
            let time1 = self.u32()?;
            let _time2 = self.u32()?;
            time1
        } else {
            0
        };
        let stored = if compressed_length != 0 {
            compressed_length
        } else {
            length
        };
        let end = self.base + offset + u64::from(stored);
        if end > self.file_len {
            return Err(self.corrupt(format!(
                "entry {name:?} ends at {end}, beyond the end of the file ({})",
                self.file_len
            )));
        }
        Ok(Entry {
            key: normalize_name(&name),
            name,
            offset: self.base + offset,
            length,
            compressed_length,
            flags,
            modified,
        })
    }

    fn u8(&mut self) -> Result<u8> {
        let mut buf = [0; 1];
        self.read(&mut buf)?;
        Ok(buf[0])
    }

    fn u32(&mut self) -> Result<u32> {
        let mut buf = [0; 4];
        self.read(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0; n];
        self.read(&mut buf)?;
        Ok(buf)
    }

    /// Reads a length-prefixed string, un-XORs it with `xor` and stops at
    /// the first NUL.
    fn pascal_string(&mut self, xor: u8) -> Result<String> {
        let len = usize::from(self.u8()?);
        let mut raw = self.bytes(len)?;
        for b in &mut raw {
            *b ^= xor;
        }
        Ok(c_string(&raw))
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<()> {
        self.reader.read_exact(buf).map_err(|err| {
            if err.kind() == io::ErrorKind::UnexpectedEof {
                self.corrupt("unexpected end of file in the directory table")
            } else {
                Error::Io(err)
            }
        })
    }

    fn corrupt(&self, reason: impl Into<String>) -> Error {
        Error::Corrupt {
            source: self.label.to_string(),
            reason: reason.into(),
        }
    }
}

/// Decodes a NUL-terminated 8-bit string. Bytes are mapped one to one to
/// `U+0000..U+00FF`, which is exact for ASCII and reversible otherwise.
fn c_string(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    raw[..end].iter().map(|&b| char::from(b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PackageBuilder;
    use std::io::Cursor;

    fn sample() -> PackageBuilder {
        let mut b = PackageBuilder::new("data");
        b.description("Sample package")
            .priority(3)
            .game_version(0x0109_0001)
            .creation_time(1_000_000);
        b.add_file("default.game", b"GAME { }".to_vec());
        b.add_file(
            "scenes\\Room\\room.scene",
            "SCENE { }".repeat(100).into_bytes(),
        )
        .modified(1_234_567_890);
        b.add_stored_file("sounds/beep.ogg", vec![1, 2, 3, 4, 5]);
        b
    }

    fn open(builder: &PackageBuilder) -> Package {
        Package::from_reader(Cursor::new(builder.build()), "sample").unwrap()
    }

    #[test]
    fn parses_header_and_directory() {
        let pkg = open(&sample());
        let h = pkg.header();
        assert_eq!(h.version, CURRENT_VERSION);
        assert_eq!(h.game_version, 0x0109_0001);
        assert_eq!(h.priority, 3);
        assert_eq!(h.cd, 0);
        assert!(!h.master_index);
        assert_eq!(h.creation_time, 1_000_000);
        assert_eq!(h.description, "Sample package");
        assert_eq!(pkg.directories().len(), 1);
        assert_eq!(pkg.directories()[0].name, "data");
        let names: Vec<_> = pkg.entries().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "default.game",
                "scenes\\Room\\room.scene",
                "sounds\\beep.ogg"
            ]
        );
    }

    #[test]
    fn reads_stored_and_compressed_entries() {
        let pkg = open(&sample());
        assert_eq!(pkg.read_file("default.game").unwrap(), b"GAME { }");
        let scene = pkg.find("SCENES/ROOM/ROOM.SCENE").unwrap();
        assert!(scene.is_compressed());
        assert_eq!(scene.length, 900);
        assert_eq!(scene.modified, 1_234_567_890);
        assert_eq!(pkg.read(scene).unwrap(), "SCENE { }".repeat(100).as_bytes());
        let beep = pkg.find("sounds\\BEEP.ogg").unwrap();
        assert!(!beep.is_compressed());
        assert_eq!(pkg.read(beep).unwrap(), [1, 2, 3, 4, 5]);
        assert!(pkg.find("missing.txt").is_none());
        assert!(matches!(
            pkg.read_file("missing.txt"),
            Err(Error::NotFound { .. })
        ));
    }

    #[test]
    fn entry_helpers() {
        let pkg = open(&sample());
        let scene = pkg.find("scenes\\room\\room.scene").unwrap();
        assert_eq!(scene.key(), "SCENES/ROOM/ROOM.SCENE");
        assert_eq!(scene.path_name(), "scenes/Room/room.scene");
        assert_eq!(
            scene.modified_time(),
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_234_567_890))
        );
        let game = pkg.find("default.game").unwrap();
        assert_eq!(game.modified_time(), None);
        assert_eq!(game.stored_length(), 8);
    }

    #[test]
    fn names_are_xored_on_disk() {
        let bytes = sample().build();
        // The clear name must not appear in the directory table, its XORed
        // form must.
        assert!(!bytes.windows(12).any(|w| w == b"default.game"));
        let xored: Vec<u8> = b"default.game\0".iter().map(|b| b ^ b'D').collect();
        assert!(bytes.windows(xored.len()).any(|w| w == xored));
    }

    #[test]
    fn rejects_bad_magic_and_newer_versions() {
        let mut bytes = sample().build();
        bytes[0] ^= 0xFF;
        let err = Package::from_reader(Cursor::new(bytes), "bad").unwrap_err();
        assert!(matches!(err, Error::NotAPackage { .. }), "{err}");

        let mut bytes = sample().build();
        bytes[8..12].copy_from_slice(&0x0300u32.to_le_bytes());
        let err = Package::from_reader(Cursor::new(bytes), "new").unwrap_err();
        assert!(
            matches!(err, Error::UnsupportedVersion { version: 0x300, .. }),
            "{err}"
        );
    }

    #[test]
    fn rejects_truncated_and_inconsistent_packages() {
        let bytes = sample().build();
        let truncated = bytes[..bytes.len() - 10].to_vec();
        let err = Package::from_reader(Cursor::new(truncated), "short").unwrap_err();
        assert!(matches!(err, Error::Corrupt { .. }), "{err}");

        let mut bytes = sample().build();
        bytes[128..132].copy_from_slice(&u32::MAX.to_le_bytes());
        let err = Package::from_reader(Cursor::new(bytes), "diroff").unwrap_err();
        assert!(matches!(err, Error::Corrupt { .. }), "{err}");

        let mut bytes = sample().build();
        let offset_pos = offset_field(&bytes, "default.game");
        bytes[offset_pos..offset_pos + 4].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        let err = Package::from_reader(Cursor::new(bytes), "range").unwrap_err();
        assert!(matches!(err, Error::Corrupt { .. }), "{err}");
    }

    /// Position of the `Offset` field of the entry named `name` in a
    /// version 2 package: right after the XORed, NUL-terminated name.
    fn offset_field(bytes: &[u8], name: &str) -> usize {
        let xored: Vec<u8> = format!("{name}\0").bytes().map(|b| b ^ NAME_XOR).collect();
        let pos = bytes
            .windows(xored.len())
            .position(|w| w == xored)
            .expect("entry name present");
        pos + xored.len()
    }

    #[test]
    fn detects_wrong_inflated_size() {
        let mut bytes = sample().build();
        let length_pos = offset_field(&bytes, "scenes\\Room\\room.scene") + 4;
        bytes[length_pos..length_pos + 4].copy_from_slice(&899u32.to_le_bytes());
        let pkg = Package::from_reader(Cursor::new(bytes), "len").unwrap();
        let err = pkg.read_file("scenes/room/room.scene").unwrap_err();
        assert!(matches!(err, Error::Decompress { .. }), "{err}");
    }

    #[test]
    fn reads_version_1_packages() {
        let mut b = sample();
        b.version(0x100);
        let pkg = open(&b);
        assert_eq!(pkg.header().version, 0x100);
        let names: Vec<_> = pkg.entries().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "default.game",
                "scenes\\Room\\room.scene",
                "sounds\\beep.ogg"
            ]
        );
        assert_eq!(pkg.read_file("default.game").unwrap(), b"GAME { }");
        assert_eq!(
            pkg.read_file("scenes/room/room.scene").unwrap(),
            "SCENE { }".repeat(100).as_bytes()
        );
        assert_eq!(pkg.find("sounds/beep.ogg").unwrap().modified, 0);
    }

    #[test]
    fn finds_packages_bound_to_an_executable() {
        let package = sample().build();
        let mut exe = vec![0x90; (EMBEDDED_SEARCH_START as usize) + 40_000];
        exe.extend_from_slice(&package);
        let base = exe.len() - package.len();
        let mut cursor = Cursor::new(exe);
        assert_eq!(find_signature(&mut cursor).unwrap(), Some(base as u64));
        let pkg = Package::from_reader_at(cursor, "exe".into(), base as u64).unwrap();
        assert_eq!(pkg.read_file("default.game").unwrap(), b"GAME { }");

        // Signature straddling a chunk boundary.
        let start = EMBEDDED_SEARCH_START as usize + 32 * 1024 - 3;
        let mut exe = vec![0x90; start];
        exe.extend_from_slice(&package);
        let mut cursor = Cursor::new(exe);
        assert_eq!(find_signature(&mut cursor).unwrap(), Some(start as u64));

        // No signature at all.
        let mut cursor = Cursor::new(vec![0x90; EMBEDDED_SEARCH_START as usize + 10]);
        assert_eq!(find_signature(&mut cursor).unwrap(), None);
        let mut cursor = Cursor::new(vec![0x90; 10]);
        assert_eq!(find_signature(&mut cursor).unwrap(), None);
    }

    #[test]
    fn normalizes_names() {
        assert_eq!(normalize_name("Scenes\\Room/a.b"), "SCENES/ROOM/A.B");
        assert_eq!(c_string(b"abc\0def"), "abc");
        assert_eq!(c_string(b"abc"), "abc");
        assert_eq!(c_string(b"\xE9t\xE9"), "été");
    }
}
