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
use std::sync::Mutex;

use flate2::read::ZlibDecoder;

use crate::error::{Error, Result};
use crate::package::{CURRENT_VERSION, Directory, Entry, MAGIC_1, Package, normalize_name};

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
    /// The `.dcp` file the data is read from when it is not the file the
    /// directory was read from (`CBFileManager::OpenPackage`).
    source: Option<&'a Path>,
    vfs: &'a Vfs,
}

impl PackagedFile<'_> {
    /// Reads the file.
    pub fn read(&self) -> Result<Vec<u8>> {
        match self.source {
            None => self.package.read(self.entry),
            Some(path) => self.vfs.read_external(path, self.entry),
        }
    }
}

/// A set of packages and loose directories with the engine's lookup rules
/// (`CBFileManager`).
///
/// - Loose files are searched first, directory by directory in the order
///   they were added, ignoring case.
/// - Packaged files are then looked up in a single table. When several
///   packages contain the same name, the copy from the package with the
///   lowest disc number wins, then the one with the highest priority, then
///   the one registered first.
/// - Built-in resources come last (`CBResourceFile`).
#[derive(Debug, Default)]
pub struct Vfs {
    /// The game directory, when the file system was opened on one.
    root: Option<PathBuf>,
    directories: Vec<PathBuf>,
    /// Where a package is looked for by the name of its directory record.
    package_paths: Vec<PathBuf>,
    packages: Vec<Package>,
    /// For each package and each of its directories, the other file its
    /// data is read from, if any.
    sources: Vec<Vec<Option<PathBuf>>>,
    index: HashMap<String, Location>,
    /// Built-in files by upper-cased name.
    resources: HashMap<String, Vec<u8>>,
    /// What the original engine logs while it scans the packages.
    warnings: Vec<String>,
    /// The files opened to read the data of other packages' directories.
    external: Mutex<HashMap<PathBuf, File>>,
}

/// The `[Resource]` section of a `wme.ini` shipped with a game: the
/// `;`-separated `CustomPaths` and `PackagePaths` lists
/// (`CBFileManager::InitPaths` reads them through the registry, which
/// looks in `.\wme.ini` first).
fn resource_paths(ini: &str) -> (Vec<String>, Vec<String>) {
    let mut in_section = false;
    let (mut custom, mut packages) = (Vec::new(), Vec::new());
    for line in ini.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_section = name.trim().eq_ignore_ascii_case("Resource");
            continue;
        }
        if !in_section || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        let list = || {
            value
                .split(';')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string)
                .collect::<Vec<String>>()
        };
        if key.trim().eq_ignore_ascii_case("CustomPaths") {
            custom = list();
        } else if key.trim().eq_ignore_ascii_case("PackagePaths") {
            packages = list();
        }
    }
    (custom, packages)
}

impl Vfs {
    /// An empty file system.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a game directory the way the engine does
    /// (`CBFileManager::InitPaths`, `RegisterPackages`).
    ///
    /// - Loose files are looked up in the `CustomPaths` of the game's
    ///   `wme.ini`, then in the game directory.
    /// - Packages are every `*.dcp` file of the game directory, of the
    ///   `PackagePaths` of `wme.ini`, then of the `data` sub-directory, in
    ///   file name order within a directory.
    /// - Last come the packages bound to an executable: every `*.exe` of
    ///   the game directory is scanned for an appended package (the
    ///   original scans its own executable, which is the game's).
    ///
    /// A package that cannot be opened or has a bad header is skipped
    /// and reported in [`Vfs::warnings`], as the original logs it and goes
    /// on. The paths of `wme.ini` must stay inside the game directory:
    /// others are refused and reported (the file comes with the game and
    /// must not make the engine read elsewhere).
    pub fn open_game(dir: impl AsRef<Path>) -> Result<Vfs> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{}: not a directory", dir.display()),
            )));
        }
        let mut vfs = Vfs::new();
        vfs.root = Some(dir.to_path_buf());
        let (custom, package_paths) = find_case_insensitive(dir, &["wme.ini"])
            .and_then(|path| fs::read(path).ok())
            .map(|bytes| resource_paths(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default();
        let inside = |vfs: &mut Vfs, kind: &str, path: &str| -> Option<PathBuf> {
            match safe_components(path) {
                Ok(components) => Some(components.iter().fold(dir.to_path_buf(), |p, c| p.join(c))),
                Err(_) => {
                    vfs.warnings.push(format!(
                        "  {kind} '{path}' of wme.ini is outside the game directory. Ignoring."
                    ));
                    None
                }
            }
        };
        for path in &custom {
            if let Some(path) = inside(&mut vfs, "CustomPaths entry", path) {
                vfs.add_directory(path);
            }
        }
        vfs.add_directory(dir);
        vfs.package_paths.push(dir.to_path_buf());
        for path in &package_paths {
            if let Some(path) = inside(&mut vfs, "PackagePaths entry", path) {
                vfs.package_paths.push(path);
            }
        }
        vfs.package_paths.push(dir.join("data"));

        for root in vfs.package_paths.clone() {
            if !root.is_dir() {
                continue;
            }
            for path in files_with_extension(&root, "dcp")? {
                match Package::open(&path) {
                    Ok(package) => {
                        if package.header().version != CURRENT_VERSION {
                            vfs.warnings.push(format!(
                                "  Warning: package file '{}' is outdated.",
                                path.display()
                            ));
                        }
                        vfs.add_package(package);
                    }
                    Err(Error::Io(err)) => vfs.warnings.push(format!(
                        "  Error opening package file '{}': {err}. Ignoring.",
                        path.display()
                    )),
                    Err(err) => vfs.warnings.push(format!(
                        "  Invalid header in package file '{}': {err}. Ignoring.",
                        path.display()
                    )),
                }
            }
        }
        for path in files_with_extension(dir, "exe")? {
            match Package::open_embedded(&path) {
                Ok(package) => {
                    vfs.add_package(package);
                }
                // No signature: an executable without a package.
                Err(Error::NotAPackage { .. }) | Err(Error::Io(_)) => {}
                Err(err) => vfs.warnings.push(format!(
                    "  Invalid header in package file '{}': {err}. Ignoring.",
                    path.display()
                )),
            }
        }
        Ok(vfs)
    }

    /// The game directory, when the file system was opened on one.
    pub fn game_directory(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// What the original engine logs while it scans the packages: the
    /// packages it ignores, the outdated ones.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Adds a built-in file, found when no loose file and no package has
    /// the name (`CBResourceFile::Open`, the resources linked into the
    /// original's executable). The name is matched whole, ignoring case.
    pub fn add_resource(&mut self, name: &str, data: Vec<u8>) -> &mut Self {
        self.resources.insert(name.to_ascii_uppercase(), data);
        self
    }

    fn resource(&self, name: &str) -> Option<&Vec<u8>> {
        self.resources.get(&name.to_ascii_uppercase())
    }

    /// Adds a directory of loose files, searched after the ones already added.
    pub fn add_directory(&mut self, dir: impl Into<PathBuf>) -> &mut Self {
        self.directories.push(dir.into());
        self
    }

    /// Registers a package and merges its entries into the lookup table
    /// (`CBFileManager::RegisterPackage`).
    ///
    /// The data of a directory is read from the file `<name>.dcp` found
    /// in the package directories, `name` being the name stored in the
    /// directory record (`CBFileManager::OpenPackage`): that is how a
    /// master index lists the files of other packages. When there is no
    /// such file, or the package is bound to an executable, the data is
    /// read from the file the directory came from (the original cannot
    /// read a package whose file was renamed; Luma can).
    pub fn add_package(&mut self, package: Package) -> &mut Self {
        let package_idx = self.packages.len();
        let priority = package.header().priority;
        let master_index = package.header().master_index;
        let own = package.path().and_then(|path| fs::canonicalize(path).ok());
        let mut sources = Vec::new();
        for (directory_idx, directory) in package.directories().iter().enumerate() {
            let source = if package.is_bound() {
                None
            } else {
                self.package_file(&directory.name)
                    .filter(|path| fs::canonicalize(path).ok() != own || own.is_none())
            };
            sources.push(source);
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
        self.sources.push(sources);
        self.packages.push(package);
        self
    }

    /// The file `<name>.dcp` of the first package directory that has one.
    fn package_file(&self, name: &str) -> Option<PathBuf> {
        if name.is_empty() || name.contains(['/', '\\', ':']) {
            return None;
        }
        let file = format!("{name}.dcp");
        self.package_paths
            .iter()
            .find_map(|dir| find_case_insensitive(dir, &[file.as_str()]))
    }

    /// Reads an entry whose data is in another file than its directory.
    fn read_external(&self, path: &Path, entry: &Entry) -> Result<Vec<u8>> {
        let mut files = self
            .external
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !files.contains_key(path) {
            files.insert(path.to_path_buf(), File::open(path)?);
        }
        let file = files.get_mut(path).expect("just inserted");
        Package::read_entry(file, entry)
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
            source: self.sources[location.package][location.directory].as_deref(),
            vfs: self,
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

    /// Whether the file exists: loose, packaged or built in.
    pub fn exists(&self, name: &str) -> bool {
        self.find_loose(name).is_some()
            || self.find_packaged(name).is_some()
            || self.resource(name).is_some()
    }

    /// Reads a file (`CBFileManager::OpenFileRaw`): the loose copy if
    /// there is one, else the packaged one, else the built-in resource of
    /// that name. A loose file that cannot be inflated is passed over, as
    /// the original then tries the next source; when nothing else has
    /// the name, its error is returned.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let mut loose_error = None;
        if let Some(path) = self.find_loose(name) {
            match read_loose(&path) {
                Ok(data) => return Ok(data),
                Err(err) => loose_error = Some(err),
            }
        }
        if let Some(file) = self.find_packaged(name) {
            return file.read();
        }
        if let Some(data) = self.resource(name) {
            return Ok(data.clone());
        }
        Err(loose_error.unwrap_or_else(|| Error::NotFound {
            name: name.to_string(),
        }))
    }
}

/// Lists the files of a directory with an extension (ignoring case),
/// sorted by file name.
fn files_with_extension(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        })
        .collect();
    paths.sort();
    Ok(paths)
}

/// Splits a game-relative name into components, rejecting anything that
/// could escape the game directory: an absolute path, a drive letter, or
/// `..` components that climb above the root. A `..` that stays inside
/// (`scenes\\room\\..\\shared\\a.png`) is resolved, as the file system
/// of the original does.
pub(crate) fn safe_components(name: &str) -> Result<Vec<&str>> {
    let invalid = || Error::InvalidName {
        name: name.to_string(),
    };
    if name.is_empty() || name.starts_with(['/', '\\']) || name.contains(':') {
        return Err(invalid());
    }
    let mut components: Vec<&str> = Vec::new();
    for component in name.split(['/', '\\']) {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(invalid());
                }
            }
            other => components.push(other),
        }
    }
    if components.is_empty() {
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
        // Loose files are only looked up in the game directory: `data` is
        // a directory of packages.
        assert!(vfs.read("loose.txt").is_err());
        assert_eq!(vfs.read("readme.txt").unwrap(), b"not a package");
        assert_eq!(vfs.game_directory(), Some(dir.as_path()));
        assert!(vfs.warnings().is_empty());
        assert!(Vfs::open_game(dir.join("nope")).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn safe_components_rejects_escapes() {
        assert_eq!(safe_components("a\\b/c.txt").unwrap(), ["a", "b", "c.txt"]);
        assert_eq!(safe_components("./a//b").unwrap(), ["a", "b"]);
        // `..` is resolved while it stays inside the root.
        assert_eq!(safe_components("a/../b").unwrap(), ["b"]);
        assert_eq!(safe_components("a\\b\\..\\..\\c").unwrap(), ["c"]);
        for bad in ["", "/abs", "\\abs", "C:\\x", "a/../../b", "..", ".", "a/.."] {
            assert!(safe_components(bad).is_err(), "{bad:?}");
        }
    }

    /// `CBFileManager::RegisterPackages`: a package that cannot be read
    /// is logged and skipped, an old one is reported, and the packages
    /// bound to an executable are registered last.
    #[test]
    fn bad_packages_are_skipped_and_bound_ones_registered() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let mut b = PackageBuilder::new("data");
        b.add_file("a.txt", b"from data".to_vec());
        fs::write(dir.join("data.dcp"), b.build()).unwrap();
        fs::write(dir.join("broken.dcp"), b"not a package at all").unwrap();
        // A package appended to an executable, past its first megabyte.
        let mut exe = vec![0x90u8; (1 << 20) + 123];
        let mut b = PackageBuilder::new("bound");
        b.add_file("a.txt", b"from the exe".to_vec());
        b.add_file("only.txt", b"bound".to_vec());
        exe.extend_from_slice(&b.build());
        fs::write(dir.join("game.exe"), exe).unwrap();
        fs::write(dir.join("tool.exe"), vec![0u8; 2048]).unwrap();

        let vfs = Vfs::open_game(&dir).unwrap();
        assert_eq!(vfs.packages().len(), 2);
        assert_eq!(vfs.read("only.txt").unwrap(), b"bound");
        // Same priority: the package registered first wins.
        assert_eq!(vfs.read("a.txt").unwrap(), b"from data");
        let warnings = vfs.warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("broken.dcp") && warnings[0].contains("Ignoring"));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// `CBFileManager::OpenPackage`: the data of a directory is read from
    /// the `.dcp` file named by the directory record, so a master index
    /// can list the files of other packages.
    #[test]
    fn a_master_index_reads_from_the_packages_it_names() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let mut b = PackageBuilder::new("disc2");
        b.add_file("far.txt", b"on the second disc".to_vec());
        let disc2 = b.build();
        // The index: the directory of `disc2` under another file name,
        // flagged as a master index. Its entries point into `disc2.dcp`.
        let mut index = disc2.clone();
        index[18] = 1;
        let directory_start = u32::from_le_bytes(index[128..132].try_into().unwrap()) as usize;
        let index = [&index[..132], &index[directory_start..]].concat();
        let mut index = index;
        index[128..132].copy_from_slice(&132u32.to_le_bytes());
        fs::write(dir.join("index.dcp"), &index).unwrap();

        // Without `disc2.dcp` the directory is known but the data is not
        // there to read.
        let vfs = Vfs::open_game(&dir).unwrap();
        assert!(vfs.find_packaged("far.txt").is_some());
        assert!(
            vfs.read("far.txt")
                .map_or(true, |data| data != b"on the second disc")
        );

        fs::create_dir_all(dir.join("data")).unwrap();
        fs::write(dir.join("data/Disc2.DCP"), &disc2).unwrap();
        let vfs = Vfs::open_game(&dir).unwrap();
        assert_eq!(vfs.read("far.txt").unwrap(), b"on the second disc");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// `CBFileManager::InitPaths`: the `CustomPaths` and `PackagePaths`
    /// of the game's `wme.ini`.
    #[test]
    fn wme_ini_adds_loose_and_package_directories() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("patch")).unwrap();
        fs::create_dir_all(dir.join("packs")).unwrap();
        fs::write(
            dir.join("WME.INI"),
            "[Other]\nCustomPaths=nope\n[resource]\ncustompaths = patch;..\\outside;C:\\abs\nPackagePaths=packs\n",
        )
        .unwrap();
        fs::write(dir.join("patch/a.txt"), b"patched").unwrap();
        fs::write(dir.join("a.txt"), b"plain").unwrap();
        fs::write(dir.join("b.txt"), b"plain b").unwrap();
        let mut b = PackageBuilder::new("extra");
        b.add_file("packed.txt", b"packed".to_vec());
        fs::write(dir.join("packs/extra.dcp"), b.build()).unwrap();

        let vfs = Vfs::open_game(&dir).unwrap();
        assert_eq!(
            vfs.read("a.txt").unwrap(),
            b"patched",
            "custom paths come first"
        );
        assert_eq!(vfs.read("b.txt").unwrap(), b"plain b");
        assert_eq!(vfs.read("packed.txt").unwrap(), b"packed");
        assert_eq!(vfs.game_directory(), Some(dir.as_path()));
        // The two paths outside the game directory are refused.
        assert_eq!(vfs.warnings().len(), 2, "{:?}", vfs.warnings());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// `CBFileManager::OpenFileRaw`: built-in resources come last, and a
    /// loose file that cannot be inflated is passed over.
    #[test]
    fn resources_come_last_and_a_broken_loose_file_is_passed_over() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).unwrap();
        let mut b = PackageBuilder::new("data");
        b.add_file("packed.txt", b"packed".to_vec());
        fs::write(dir.join("data.dcp"), b.build()).unwrap();
        // A compressed container whose stream is garbage.
        let mut broken = Vec::new();
        broken.extend_from_slice(&MAGIC_1.to_le_bytes());
        broken.extend_from_slice(&COMPRESSED_FILE_MAGIC.to_le_bytes());
        broken.extend_from_slice(&20u32.to_le_bytes());
        broken.extend_from_slice(&4u32.to_le_bytes());
        broken.extend_from_slice(&10u32.to_le_bytes());
        broken.extend_from_slice(b"junk");
        fs::write(dir.join("packed.txt"), &broken).unwrap();
        fs::write(dir.join("alone.txt"), &broken).unwrap();

        let mut vfs = Vfs::open_game(&dir).unwrap();
        vfs.add_resource("invalid.bmp", b"built in".to_vec());
        vfs.add_resource("packed.txt", b"never reached".to_vec());
        assert_eq!(vfs.read("packed.txt").unwrap(), b"packed");
        assert!(matches!(
            vfs.read("alone.txt"),
            Err(Error::Decompress { .. })
        ));
        assert_eq!(vfs.read("INVALID.BMP").unwrap(), b"built in");
        assert!(vfs.exists("invalid.bmp"));
        // The name is matched whole: no directory.
        assert!(!vfs.exists("art\\invalid.bmp"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
