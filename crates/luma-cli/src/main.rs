//! `luma`: command-line tools for Luma Engine.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use luma_archive::{Package, PackagedFile, Vfs};
use luma_formats::{Document, Sprite};

const USAGE: &str = "\
luma - command-line tools for Luma Engine

USAGE:
    luma <COMMAND> [ARGS]

COMMANDS:
    list <game>            List the files contained in a game's packages
    unpack <game> <dir>    Extract every file from a game's packages
    dump <file>            Parse a definition file and print it in canonical form
    dump <game> <name>     Same, for a file inside a game's packages
    sprite <game> <name>   Describe a sprite and the images it uses
    help                   Show this message
    version                Show the version

<game> is a game directory (its *.dcp packages, and those of its `data`
sub-directory, are read the way the engine does) or a single .dcp file.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some("version" | "-V" | "--version") => {
            println!("luma {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("list") => match args.as_slice() {
            [_, game] => list(Path::new(game)),
            _ => usage_error("list takes exactly one argument: <game>"),
        },
        Some("unpack") => match args.as_slice() {
            [_, game, dir] => unpack(Path::new(game), Path::new(dir)),
            _ => usage_error("unpack takes exactly two arguments: <game> <dir>"),
        },
        Some("sprite") => match args.as_slice() {
            [_, game, name] => sprite(Path::new(game), name),
            _ => usage_error("sprite takes exactly two arguments: <game> <name>"),
        },
        Some("dump") => match args.as_slice() {
            [_, file] => dump(Path::new(file), None),
            [_, game, name] => dump(Path::new(game), Some(name)),
            _ => usage_error("dump takes <file> or <game> <name>"),
        },
        Some(other) => usage_error(&format!("unknown command `{other}`")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(message)) => {
            eprintln!("luma: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Error(message)) => {
            eprintln!("luma: {message}");
            ExitCode::FAILURE
        }
    }
}

enum Failure {
    Usage(String),
    Error(String),
}

impl<E: std::fmt::Display> From<E> for Failure {
    fn from(err: E) -> Self {
        Failure::Error(err.to_string())
    }
}

fn usage_error(message: &str) -> Result<(), Failure> {
    Err(Failure::Usage(message.to_string()))
}

/// Opens a game directory, or a single package.
fn open(game: &Path) -> Result<Vfs, Failure> {
    if game.is_file() {
        let mut vfs = Vfs::new();
        vfs.add_package(Package::open(game)?);
        Ok(vfs)
    } else {
        let vfs = Vfs::open_game(game)?;
        if vfs.packages().is_empty() {
            return Err(Failure::Error(format!(
                "{}: no .dcp package found",
                game.display()
            )));
        }
        Ok(vfs)
    }
}

fn list(game: &Path) -> Result<(), Failure> {
    let vfs = open(game)?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let files = vfs.packaged_files();
    let mut total = 0u64;
    for file in &files {
        total += u64::from(file.entry.length);
        writeln!(
            out,
            "{:>10}\t{}\t{}",
            file.entry.length,
            package_name(file.package.label()),
            file.entry.path_name()
        )?;
    }
    out.flush()?;
    eprintln!(
        "{} files, {} bytes, in {} package(s)",
        files.len(),
        total,
        vfs.packages().len()
    );
    Ok(())
}

fn unpack(game: &Path, dir: &Path) -> Result<(), Failure> {
    let vfs = open(game)?;
    fs::create_dir_all(dir)?;
    let files = vfs.packaged_files();
    let mut total = 0u64;
    for file in &files {
        let target = target_path(dir, file)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = file
            .read()
            .map_err(|err| Failure::Error(format!("{}: {err}", file.entry.path_name())))?;
        let out = File::create(&target)
            .map_err(|err| Failure::Error(format!("{}: {err}", target.display())))?;
        let mut out = BufWriter::new(out);
        out.write_all(&data)?;
        let out = out.into_inner().map_err(|err| err.into_error())?;
        if let Some(modified) = file.entry.modified_time() {
            // Keeping the original timestamp is a nicety, not a requirement.
            let _ = out.set_modified(modified);
        }
        total += data.len() as u64;
    }
    eprintln!(
        "Extracted {} files, {} bytes, to {}",
        files.len(),
        total,
        dir.display()
    );
    Ok(())
}

fn dump(path: &Path, name: Option<&str>) -> Result<(), Failure> {
    let (label, bytes) = match name {
        Some(name) => {
            let vfs = open(path)?;
            (name.to_string(), vfs.read(name)?)
        }
        None => (path.display().to_string(), fs::read(path)?),
    };
    let doc = Document::parse(&bytes).map_err(|err| Failure::Error(format!("{label}: {err}")))?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    write!(out, "{doc}")?;
    out.flush()?;
    Ok(())
}

fn sprite(game: &Path, name: &str) -> Result<(), Failure> {
    let vfs = Vfs::open_game(game)?;
    let sprite = if Sprite::is_image_path(name) {
        Sprite::from_image(name)
    } else {
        let bytes = vfs.read(name)?;
        Sprite::parse(&bytes).map_err(|err| Failure::Error(format!("{name}: {err}")))?
    };
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    writeln!(out, "{name}: {:?}", sprite.name)?;
    let mut flags = Vec::new();
    for (set, label) in [
        (sprite.looping, "looping"),
        (sprite.continuous, "continuous"),
        (sprite.precise, "precise"),
        (sprite.streamed, "streamed"),
    ] {
        if set {
            flags.push(label);
        }
    }
    writeln!(
        out,
        "  {} frame(s), {} ms total, flags: {}",
        sprite.frames.len(),
        sprite
            .frames
            .iter()
            .map(|f| u64::from(f.delay_ms))
            .sum::<u64>(),
        if flags.is_empty() {
            "none".to_string()
        } else {
            flags.join(" ")
        }
    )?;
    for (i, frame) in sprite.frames.iter().enumerate() {
        writeln!(out, "  frame {i}: {} ms", frame.delay_ms)?;
        for sub in &frame.subframes {
            let Some(image) = &sub.image else {
                writeln!(out, "    (no image)")?;
                continue;
            };
            let size = match vfs.read(image) {
                Ok(bytes) => match luma_formats::image::decode(&bytes, sub.color_key) {
                    Ok(img) => format!(
                        "{}x{} {}",
                        img.width,
                        img.height,
                        if img.has_alpha_channel {
                            "alpha"
                        } else {
                            "color key"
                        }
                    ),
                    Err(err) => format!("ERROR: {err}"),
                },
                Err(err) => format!("ERROR: {err}"),
            };
            let rect = sub
                .rect
                .map(|r| format!(" rect {},{},{},{}", r.left, r.top, r.right, r.bottom))
                .unwrap_or_default();
            writeln!(
                out,
                "    {image} ({size}) hotspot {},{}{rect}",
                sub.hotspot_x, sub.hotspot_y
            )?;
        }
    }
    out.flush()?;
    Ok(())
}

/// Builds the extraction path of an entry, refusing names that would land
/// outside `dir`.
fn target_path(dir: &Path, file: &PackagedFile<'_>) -> Result<PathBuf, Failure> {
    let name = file.entry.path_name();
    let mut target = dir.to_path_buf();
    let mut components = 0;
    for component in name.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                return Err(Failure::Error(format!(
                    "{name}: refusing to extract outside the target directory"
                )));
            }
            _ if component.contains(':') => {
                return Err(Failure::Error(format!("{name}: invalid file name")));
            }
            _ => {
                target.push(component);
                components += 1;
            }
        }
    }
    if components == 0 {
        return Err(Failure::Error(format!("{name:?}: invalid file name")));
    }
    Ok(target)
}

/// The package file name without directory and extension.
fn package_name(label: &str) -> String {
    Path::new(label)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| label.to_string())
}
