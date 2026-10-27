//! The virtual file system: packages and loose directories layered the way
//! the original engine resolves them.
//!
//! The lookup rules mirror those of the MIT-licensed WME Lite and Wintermute
//! Engine 1.x file manager (Copyright (c) 2003-2013 Jan Nedoma and
//! contributors) and are documented in `docs/formats/dcp.md`. See `NOTICE`
//! at the root of the repository.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::ZlibDecoder;

use crate::error::{Error, Result};
use crate::package::{Directory, Entry, MAGIC_1, Package, normalize_name};

/// Second magic number of a compressed loose file: the ASCII bytes `ZCMP`.
const COMPRESSED_FILE_MAGIC: u32 = 0x504D_435A;

/// Where a packaged file is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Location {
    package: usize,
    directory: usize,
    entry: usize,
}

/// A packaged file resolved through a [`Vfs`].
#[derive(Debug, Clone, Copy)]
pub struct PackagedFile<'a> {
    /// The package holding the winning copy of the file.
    pub package: &'a Package,
    /// The directory of that package listing the file.
    pub directory: &'a Directory,
    /// The file entry.
    pub entry: &'a Entry,
}

impl PackagedFile<'_> {
    /// Reads the file.
    pub fn read(&self) -> Result<Vec<u8>> {
        self.package.read(self.entry)
    }
}

/// A set of packages and loose directories with the engine's lookup rules.
///
/// - Loose files are searched first, directory by directory in the order
///   they were added, ignoring case.
/// - Packaged files are then looked up in a single table. When several
///   packages contain the same name, the copy from the package with the
///   lowest disc number wins, then the one with the highest priority, then
///   the one registered first.
#[derive(Debug, Default)]
pub struct Vfs {
    directories: Vec<PathBuf>,
    packages: Vec<Package>,
    index: HashMap<String, Location>,
}

impl Vfs {
    /// An empty file system.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a game directory the way the engine does: loose files are
    /// looked up in the directory and its `data` sub-directory, and every
    /// `*.dcp` file found in those two places is registered, in file name
    /// order.
    pub fn open_game(dir: impl AsRef<Path>) -> Result<Vfs> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{}: not a directory", dir.display()),
            )));
        }
        let mut vfs = Vfs::new();
        for root in [dir.to_path_buf(), dir.join("data")] {
            if !root.is_dir() {
                continue;
            }
            vfs.add_directory(&root);
            for path in package_files(&root)? {
                vfs.add_package(Package::open(&path)?);
            }
        }
        Ok(vfs)
    }

    /// Adds a directory of loose files, searched after the ones already added.
    pub fn add_directory(&mut self, dir: impl Into<PathBuf>) -> &mut Self {
        self.directories.push(dir.into());
        self
    }

    /// Registers a package and merges its entries into the lookup table.
    pub fn add_package(&mut self, package: Package) -> &mut Self {
        let package_idx = self.packages.len();
        let priority = package.header().priority;
        let master_index = package.header().master_index;
        for (directory_idx, directory) in package.directories().iter().enumerate() {
            // Only a master index spreads files over several discs.
            let cd = if master_index { directory.cd } else { 0 };
            for (entry_idx, entry) in directory.entries.iter().enumerate() {
                let location = Location {
                    package: package_idx,
                    directory: directory_idx,
                    entry: entry_idx,
                };
                match self.index.get(entry.key()) {
                    None => {
                        self.index.insert(entry.key().to_string(), location);
                    }
                    Some(existing) => {
                        let (existing_cd, existing_priority) = self.rank(*existing);
                        if cd < existing_cd || priority > existing_priority {
                            self.index.insert(entry.key().to_string(), location);
                        }
                    }
                }
            }
        }
        self.packages.push(package);
        self
    }

    fn rank(&self, location: Location) -> (u8, u8) {
        let package = &self.packages[location.package];
        let cd = if package.header().master_index {
            package.directories()[location.directory].cd
        } else {
            0
        };
        (cd, package.header().priority)
    }

    /// The registered packages, in registration order.
    pub fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// The loose-file directories, in search order.
    pub fn directories(&self) -> &[PathBuf] {
        &self.directories
    }

    fn resolve(&self, location: Location) -> PackagedFile<'_> {
        let package = &self.packages[location.package];
        let directory = &package.directories()[location.directory];
        PackagedFile {
            package,
            directory,
            entry: &directory.entries[location.entry],
        }
    }

    /// Every packaged file after conflict resolution, sorted by normalised
    /// name.
    pub fn packaged_files(&self) -> Vec<PackagedFile<'_>> {
        let mut keys: Vec<&String> = self.index.keys().collect();
        keys.sort_unstable();
        keys.into_iter()
            .map(|key| self.resolve(self.index[key]))
            .collect()
    }

    /// Finds the winning packaged copy of a file, ignoring loose files.
    pub fn find_packaged(&self, name: &str) -> Option<PackagedFile<'_>> {
        self.index
            .get(&normalize_name(name))
            .map(|&location| self.resolve(location))
    }

    /// Finds a loose file in the registered directories, ignoring case.
    pub fn find_loose(&self, name: &str) -> Option<PathBuf> {
        let components = safe_components(name).ok()?;
        self.directories
            .iter()
            .find_map(|dir| find_case_insensitive(dir, &components))
    }

    /// Whether the file exists, loose or packaged.
    pub fn exists(&self, name: &str) -> bool {
        self.find_loose(name).is_some() || self.find_packaged(name).is_some()
    }

    /// Reads a file: the loose copy if there is one, else the packaged one.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        if let Some(path) = self.find_loose(name) {
            return read_loose(&path);
        }
        match self.find_packaged(name) {
            Some(file) => file.read(),
            None => Err(Error::NotFound {
                name: name.to_string(),
            }),
        }
    }
}

/// Lists the `*.dcp` files of a directory, sorted by file name.
fn package_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("dcp"))
        })
        .collect();
    paths.sort();
    Ok(paths)
}

/// Splits a game-relative name into components, rejecting anything that
/// could escape the game directory.
pub(crate) fn safe_components(name: &str) -> Result<Vec<&str>> {
    let invalid = || Error::InvalidName {
        name: name.to_string(),
    };
    if name.is_empty() || name.starts_with(['/', '\\']) || name.contains(':') {
        return Err(invalid());
    }
    let components: Vec<&str> = name
        .split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    if components.is_empty() || components.contains(&"..") {
        return Err(invalid());
    }
    Ok(components)
}

/// Walks `dir` down `components`, matching each one ignoring ASCII case,
/// and returns the path of the file if it exists.
fn find_case_insensitive(dir: &Path, components: &[&str]) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    for (i, component) in components.iter().enumerate() {
        let last = i + 1 == components.len();
        let exact = current.join(component);
        let candidate = if exact.exists() {
            exact
        } else {
            fs::read_dir(&current).ok()?.flatten().find_map(|entry| {
                let file_name = entry.file_name();
                file_name
                    .to_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(component))
                    .then(|| entry.path())
            })?
        };
        if last {
            return candidate.is_file().then_some(candidate);
        }
        if !candidate.is_dir() {
            return None;
        }
        current = candidate;
    }
    None
}

/// Reads a loose file, inflating it if it uses the engine's compressed
/// container.
fn read_loose(path: &Path) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    File::open(path)?.read_to_end(&mut data)?;
    if data.len() < 20 {
        return Ok(data);
    }
    let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().expect("4 bytes"));
    if word(0) != MAGIC_1 || word(4) != COMPRESSED_FILE_MAGIC {
        return Ok(data);
    }
    let name = path.display().to_string();
    let data_offset = word(8) as usize;
    let comp_size = word(12) as usize;
    let uncomp_size = word(16) as u64;
    let stream = data
        .get(data_offset..data_offset + comp_size)
        .ok_or_else(|| Error::Decompress {
            name: name.clone(),
            reason: "compressed data extends beyond the end of the file".into(),
        })?;
    let mut out = Vec::new();
    ZlibDecoder::new(stream)
        .take(uncomp_size + 1)
        .read_to_end(&mut out)
        .map_err(|err| Error::Decompress {
            name: name.clone(),
            reason: err.to_string(),
        })?;
    if out.len() as u64 != uncomp_size {
        return Err(Error::Decompress {
            name,
            reason: format!("inflated to {} bytes, expected {uncomp_size}", out.len()),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PackageBuilder;
    use std::io::{Cursor, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn package(name: &str, priority: u8, files: &[(&str, &str)]) -> Package {
        let mut b = PackageBuilder::new(name);
        b.priority(priority);
        for (file, content) in files {
            b.add_file(*file, content.as_bytes().to_vec());
        }
        Package::from_reader(Cursor::new(b.build()), name).unwrap()
    }

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "luma-archive-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn higher_priority_wins_and_first_wins_on_tie() {
        let mut vfs = Vfs::new();
        vfs.add_package(package(
            "base",
            0,
            &[("a.txt", "base a"), ("b.txt", "base b")],
        ));
        vfs.add_package(package(
            "patch",
            1,
            &[("A.TXT", "patch a"), ("c.txt", "patch c")],
        ));
        vfs.add_package(package("other", 0, &[("b.txt", "other b")]));
        assert_eq!(vfs.read("a.txt").unwrap(), b"patch a");
        assert_eq!(vfs.read("B.txt").unwrap(), b"base b");
        assert_eq!(vfs.read("c.txt").unwrap(), b"patch c");
        assert!(matches!(vfs.read("d.txt"), Err(Error::NotFound { .. })));
        let names: Vec<_> = vfs
            .packaged_files()
            .iter()
            .map(|f| format!("{}:{}", f.package.label(), f.entry.name))
            .collect();
        assert_eq!(names, ["patch:A.TXT", "base:b.txt", "patch:c.txt"]);
    }

    #[test]
    fn lower_disc_number_wins_in_master_indexes() {
        // A master index whose second directory is on disc 2, while a
        // regular package of lower priority sits on the hard disk.
        let mut b = PackageBuilder::new("cd2");
        b.priority(5).cd(2);
        b.add_file("movie.ogv", b"cd2".to_vec());
        let bytes = b.build();
        // Flip MasterIndex on so the directory's CD is honoured.
        let mut master = bytes.clone();
        master[18] = 1;
        let mut vfs = Vfs::new();
        vfs.add_package(Package::from_reader(Cursor::new(master), "master").unwrap());
        vfs.add_package(package("hd", 0, &[("movie.ogv", "hd")]));
        assert_eq!(vfs.read("movie.ogv").unwrap(), b"hd");

        // Without the master index flag the CD is ignored and priority wins.
        let mut vfs = Vfs::new();
        vfs.add_package(Package::from_reader(Cursor::new(bytes), "regular").unwrap());
        vfs.add_package(package("hd", 0, &[("movie.ogv", "hd")]));
        assert_eq!(vfs.read("movie.ogv").unwrap(), b"cd2");
    }

    #[test]
    fn loose_files_shadow_packages_and_match_ignoring_case() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("Scenes/Room")).unwrap();
        fs::write(dir.join("Scenes/Room/room.scene"), b"loose").unwrap();
        let mut vfs = Vfs::new();
        vfs.add_directory(&dir);
        vfs.add_package(package(
            "data",
            0,
            &[("scenes\\room\\room.scene", "packaged")],
        ));
        assert_eq!(vfs.read("SCENES\\ROOM\\ROOM.SCENE").unwrap(), b"loose");
        assert_eq!(vfs.read("scenes/room/room.scene").unwrap(), b"loose");
        assert!(vfs.exists("scenes/room/ROOM.scene"));
        assert_eq!(
            vfs.find_packaged("scenes/room/room.scene")
                .unwrap()
                .read()
                .unwrap(),
            b"packaged"
        );
        assert!(vfs.find_loose("scenes/room/missing.scene").is_none());
        assert!(
            vfs.find_loose("scenes/room").is_none(),
            "directories are not files"
        );
        assert!(vfs.find_loose("../etc/passwd").is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_compressed_loose_files() {
        let dir = temp_dir();
        let payload = b"hello hello hello hello hello".repeat(10);
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&payload).unwrap();
        let stream = enc.finish().unwrap();
        let mut file = Vec::new();
        file.extend_from_slice(&MAGIC_1.to_le_bytes());
        file.extend_from_slice(&COMPRESSED_FILE_MAGIC.to_le_bytes());
        file.extend_from_slice(&20u32.to_le_bytes());
        file.extend_from_slice(&(stream.len() as u32).to_le_bytes());
        file.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        file.extend_from_slice(&stream);
        fs::write(dir.join("save.dat"), &file).unwrap();
        fs::write(dir.join("plain.dat"), b"plain").unwrap();
        let mut vfs = Vfs::new();
        vfs.add_directory(&dir);
        assert_eq!(vfs.read("save.dat").unwrap(), payload);
        assert_eq!(vfs.read("plain.dat").unwrap(), b"plain");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn opens_a_game_directory() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("data")).unwrap();
        let mut b = PackageBuilder::new("data");
        b.add_file("default.game", b"GAME".to_vec());
        fs::write(dir.join("data.dcp"), b.build()).unwrap();
        let mut b = PackageBuilder::new("extra");
        b.priority(1);
        b.add_file("default.game", b"EXTRA".to_vec());
        b.add_file("extra.txt", b"x".to_vec());
        fs::write(dir.join("data/Extra.DCP"), b.build()).unwrap();
        fs::write(dir.join("data/loose.txt"), b"loose").unwrap();
        fs::write(dir.join("readme.txt"), b"not a package").unwrap();

        let vfs = Vfs::open_game(&dir).unwrap();
        assert_eq!(vfs.packages().len(), 2);
        assert_eq!(vfs.read("default.game").unwrap(), b"EXTRA");
        assert_eq!(vfs.read("extra.txt").unwrap(), b"x");
        assert_eq!(vfs.read("loose.txt").unwrap(), b"loose");
        assert_eq!(vfs.read("readme.txt").unwrap(), b"not a package");
        assert!(Vfs::open_game(dir.join("nope")).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn safe_components_rejects_escapes() {
        assert_eq!(safe_components("a\\b/c.txt").unwrap(), ["a", "b", "c.txt"]);
        assert_eq!(safe_components("./a//b").unwrap(), ["a", "b"]);
        for bad in ["", "/abs", "\\abs", "C:\\x", "a/../b", "..", "."] {
            assert!(safe_components(bad).is_err(), "{bad:?}");
        }
    }
}
