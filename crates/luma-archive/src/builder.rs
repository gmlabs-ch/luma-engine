//! Writing packages, in the layout produced by the engine's project manager.
//!
//! Used by the tests of this crate and available to tooling. The layout is
//! documented in `docs/formats/dcp.md` and was established from the
//! MIT-licensed WME Lite and Wintermute Engine 1.x sources (Copyright (c)
//! 2003-2013 Jan Nedoma and contributors). See `NOTICE` at the root of the
//! repository.

use std::io::{self, Seek, SeekFrom, Write};

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::package::{CURRENT_VERSION, MAGIC_1, MAGIC_2};

/// Key used to obfuscate entry names in version 2 packages.
const NAME_XOR: u8 = b'D';

/// A file queued for writing.
#[derive(Debug, Clone)]
pub struct FileSpec {
    name: String,
    data: Vec<u8>,
    compress: bool,
    modified: u32,
}

impl FileSpec {
    /// Sets the last-write timestamp recorded for the file (32-bit Unix time).
    pub fn modified(&mut self, modified: u32) -> &mut Self {
        self.modified = modified;
        self
    }
}

/// Builds a package in memory or writes it to a seekable writer.
///
/// File names may use `/` or `\`; they are stored with `\` as the project
/// manager does. Files added with [`add_file`](Self::add_file) are
/// compressed when that makes them smaller, which is also the project
/// manager's rule.
#[derive(Debug, Clone)]
pub struct PackageBuilder {
    name: String,
    description: String,
    version: u32,
    game_version: u32,
    priority: u8,
    cd: u8,
    creation_time: u32,
    files: Vec<FileSpec>,
}

impl PackageBuilder {
    /// Starts a package with the given internal name (the project folder
    /// name, for example `data`).
    pub fn new(name: impl Into<String>) -> Self {
        PackageBuilder {
            name: name.into(),
            description: String::new(),
            version: CURRENT_VERSION,
            game_version: 0,
            priority: 0,
            cd: 0,
            creation_time: 0,
            files: Vec::new(),
        }
    }

    /// Sets the description (truncated to 99 bytes on disk).
    pub fn description(&mut self, description: impl Into<String>) -> &mut Self {
        self.description = description.into();
        self
    }

    /// Sets the format version. Versions below `0x200` are written in the
    /// version 1 layout (no directory offset, clear names, no timestamps).
    pub fn version(&mut self, version: u32) -> &mut Self {
        self.version = version;
        self
    }

    /// Sets the game version number stored in the header.
    pub fn game_version(&mut self, game_version: u32) -> &mut Self {
        self.game_version = game_version;
        self
    }

    /// Sets the package priority.
    pub fn priority(&mut self, priority: u8) -> &mut Self {
        self.priority = priority;
        self
    }

    /// Sets the disc number.
    pub fn cd(&mut self, cd: u8) -> &mut Self {
        self.cd = cd;
        self
    }

    /// Sets the build time recorded in the header.
    pub fn creation_time(&mut self, creation_time: u32) -> &mut Self {
        self.creation_time = creation_time;
        self
    }

    /// Adds a file, compressed if that makes it smaller.
    pub fn add_file(&mut self, name: impl Into<String>, data: Vec<u8>) -> &mut FileSpec {
        self.push(name.into(), data, true)
    }

    /// Adds a file stored verbatim.
    pub fn add_stored_file(&mut self, name: impl Into<String>, data: Vec<u8>) -> &mut FileSpec {
        self.push(name.into(), data, false)
    }

    fn push(&mut self, name: String, data: Vec<u8>, compress: bool) -> &mut FileSpec {
        self.files.push(FileSpec {
            name: name.replace('/', "\\"),
            data,
            compress,
            modified: 0,
        });
        self.files.last_mut().expect("just pushed")
    }

    /// Builds the package in memory.
    pub fn build(&self) -> Vec<u8> {
        let mut cursor = io::Cursor::new(Vec::new());
        self.write(&mut cursor)
            .expect("writing to memory cannot fail");
        cursor.into_inner()
    }

    /// Writes the package.
    pub fn write<W: Write + Seek>(&self, w: &mut W) -> io::Result<()> {
        let v2 = self.version >= CURRENT_VERSION;

        w.write_all(&MAGIC_1.to_le_bytes())?;
        w.write_all(&MAGIC_2.to_le_bytes())?;
        w.write_all(&self.version.to_le_bytes())?;
        w.write_all(&self.game_version.to_le_bytes())?;
        w.write_all(&[self.priority, self.cd, 0, 0])?;
        w.write_all(&self.creation_time.to_le_bytes())?;
        let mut desc = [0u8; 100];
        for (dst, src) in desc.iter_mut().zip(self.description.bytes().take(99)) {
            *dst = src;
        }
        w.write_all(&desc)?;
        w.write_all(&1u32.to_le_bytes())?; // NumDirs

        // Prepare the data blobs first: version 1 writes the directory
        // before the data, version 2 after it.
        let mut blobs = Vec::with_capacity(self.files.len());
        for file in &self.files {
            let compressed = if file.compress {
                let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
                enc.write_all(&file.data)?;
                let out = enc.finish()?;
                (out.len() < file.data.len()).then_some(out)
            } else {
                None
            };
            blobs.push(compressed);
        }

        if v2 {
            let dir_offset_pos = w.stream_position()?;
            w.write_all(&0u32.to_le_bytes())?; // DirOffset, patched below
            let offsets = self.write_data(w, &blobs)?;
            let dir_offset = w.stream_position()?;
            self.write_directory(w, &blobs, &offsets, true)?;
            w.seek(SeekFrom::Start(dir_offset_pos))?;
            w.write_all(&offset_u32(dir_offset)?.to_le_bytes())?;
            w.seek(SeekFrom::End(0))?;
        } else {
            // Compute the directory size to know where data will start.
            let mut dir_len = 1 + self.name.len() + 1 + 1 + 4;
            for file in &self.files {
                dir_len += 1 + file.name.len() + 1 + 4 * 4;
            }
            let data_start = w.stream_position()? + dir_len as u64;
            let mut offsets = Vec::with_capacity(self.files.len());
            let mut pos = data_start;
            for (file, blob) in self.files.iter().zip(&blobs) {
                offsets.push(pos);
                pos += blob.as_ref().map_or(file.data.len(), Vec::len) as u64;
            }
            self.write_directory(w, &blobs, &offsets, false)?;
            self.write_data(w, &blobs)?;
        }
        Ok(())
    }

    fn write_data<W: Write + Seek>(
        &self,
        w: &mut W,
        blobs: &[Option<Vec<u8>>],
    ) -> io::Result<Vec<u64>> {
        let mut offsets = Vec::with_capacity(self.files.len());
        for (file, blob) in self.files.iter().zip(blobs) {
            offsets.push(w.stream_position()?);
            w.write_all(blob.as_deref().unwrap_or(&file.data))?;
        }
        Ok(offsets)
    }

    fn write_directory<W: Write>(
        &self,
        w: &mut W,
        blobs: &[Option<Vec<u8>>],
        offsets: &[u64],
        v2: bool,
    ) -> io::Result<()> {
        write_string(w, &self.name, 0)?;
        w.write_all(&[self.cd])?;
        w.write_all(&len_u32(self.files.len())?.to_le_bytes())?;
        for ((file, blob), &offset) in self.files.iter().zip(blobs).zip(offsets) {
            write_string(w, &file.name, if v2 { NAME_XOR } else { 0 })?;
            w.write_all(&offset_u32(offset)?.to_le_bytes())?;
            w.write_all(&len_u32(file.data.len())?.to_le_bytes())?;
            let comp_len = blob.as_ref().map_or(0, Vec::len);
            w.write_all(&len_u32(comp_len)?.to_le_bytes())?;
            w.write_all(&0u32.to_le_bytes())?; // Flags
            if v2 {
                w.write_all(&file.modified.to_le_bytes())?;
                w.write_all(&0u32.to_le_bytes())?; // TimeDate2
            }
        }
        Ok(())
    }
}

/// Writes a Pascal string with its NUL terminator, XORed with `xor`.
fn write_string<W: Write>(w: &mut W, s: &str, xor: u8) -> io::Result<()> {
    let bytes: Vec<u8> = s.chars().map(|c| latin1_byte(c) ^ xor).collect();
    let len = u8::try_from(bytes.len() + 1).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("name too long: {s:?}"))
    })?;
    w.write_all(&[len])?;
    w.write_all(&bytes)?;
    w.write_all(&[xor])?; // NUL terminator, XORed too
    Ok(())
}

/// Maps a character to its 8-bit code, the inverse of the decoding done
/// while reading. Characters above U+00FF are replaced by `?`.
fn latin1_byte(c: char) -> u8 {
    u8::try_from(u32::from(c)).unwrap_or(b'?')
}

fn offset_u32(offset: u64) -> io::Result<u32> {
    u32::try_from(offset)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "package larger than 4 GiB"))
}

fn len_u32(len: usize) -> io::Result<u32> {
    u32::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "file larger than 4 GiB"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_128_bytes_and_directory_offset_points_past_data() {
        let mut b = PackageBuilder::new("data");
        b.add_stored_file("a.txt", vec![1, 2, 3]);
        let bytes = b.build();
        assert_eq!(&bytes[0..4], &[0xDE, 0xAD, 0xC0, 0xDE]);
        assert_eq!(&bytes[4..8], b"JUNK");
        assert_eq!(u32::from_le_bytes(bytes[124..128].try_into().unwrap()), 1);
        let dir_offset = u32::from_le_bytes(bytes[128..132].try_into().unwrap());
        assert_eq!(dir_offset, 132 + 3);
        assert_eq!(&bytes[132..135], &[1, 2, 3]);
        // Directory: "data\0" as Pascal string, CD, one entry.
        assert_eq!(&bytes[135..141], b"\x05data\0");
        assert_eq!(bytes[141], 0);
        assert_eq!(u32::from_le_bytes(bytes[142..146].try_into().unwrap()), 1);
    }

    #[test]
    fn compresses_only_when_smaller() {
        let mut b = PackageBuilder::new("data");
        b.add_file("tiny.txt", b"x".to_vec());
        b.add_file("big.txt", vec![0; 10_000]);
        let pkg = crate::Package::from_reader(io::Cursor::new(b.build()), "t").unwrap();
        assert!(!pkg.find("tiny.txt").unwrap().is_compressed());
        assert!(pkg.find("big.txt").unwrap().is_compressed());
        assert_eq!(pkg.read_file("big.txt").unwrap(), vec![0; 10_000]);
    }

    #[test]
    fn rejects_overlong_names() {
        let mut b = PackageBuilder::new("data");
        b.add_stored_file("x".repeat(300), vec![]);
        let mut cursor = io::Cursor::new(Vec::new());
        assert!(b.write(&mut cursor).is_err());
    }
}
