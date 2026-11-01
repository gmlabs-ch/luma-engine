//! `luma`: command-line tools for Luma Engine.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use luma_archive::{Package, PackageBuilder, PackagedFile, Vfs};
use luma_formats::{Document, Scene, SceneNode, Sprite};
use luma_script::Program;

const USAGE: &str = "\
luma - command-line tools for Luma Engine

USAGE:
    luma <COMMAND> [ARGS]

COMMANDS:
    list <game>            List the files contained in a game's packages
    unpack <game> <dir>    Extract every file from a game's packages
    pack <dir> <file.dcp>  Build a package from the files of a directory
    dump <file>            Parse a definition file and print it in canonical form
    dump <game> <name>     Same, for a file inside a game's packages
    sprite <game> <name>   Describe a sprite and the images it uses
    scene <game> <name>    Describe a scene: layers, entities, regions
    script <game> <name>   Disassemble a compiled script
    lumen check <story>    Check a Lumen story (a folder or a .lumen file)
    lumen play <story>     Play a Lumen story as text in the terminal
    help                   Show this message
    version                Show the version

<game> is a game directory (its *.dcp packages, and those of its `data`
sub-directory, are read the way the engine does) or a single .dcp file.

lumen play takes --start <knot> to begin elsewhere than the manifest's
start knot, --seed <n> for the random functions, and --choose <n,n,...>
to answer the first choices without asking (then quits when they run out).
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
        Some("pack") => match args.as_slice() {
            [_, dir, file] => pack(Path::new(dir), Path::new(file)),
            _ => usage_error("pack takes exactly two arguments: <dir> <file.dcp>"),
        },
        Some("sprite") => match args.as_slice() {
            [_, game, name] => sprite(Path::new(game), name),
            _ => usage_error("sprite takes exactly two arguments: <game> <name>"),
        },
        Some("scene") => match args.as_slice() {
            [_, game, name] => scene(Path::new(game), name),
            _ => usage_error("scene takes exactly two arguments: <game> <name>"),
        },
        Some("script") => match args.as_slice() {
            [_, game, name] => script(Path::new(game), name),
            _ => usage_error("script takes exactly two arguments: <game> <name>"),
        },
        Some("lumen") => lumen::run(&args[1..]),
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

mod lumen;

/// How a command fails: a usage error (exit code 2) or an error (1).
pub enum Failure {
    /// Wrong arguments; the usage is printed.
    Usage(String),
    /// Something went wrong.
    Error(String),
}

impl<E: std::fmt::Display> From<E> for Failure {
    fn from(err: E) -> Self {
        Failure::Error(err.to_string())
    }
}

/// A usage error, for a command given the wrong arguments.
pub fn usage_error(message: &str) -> Result<(), Failure> {
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

/// Packs the files under `dir` into one package, the way the project
/// manager builds `data.dcp`: paths relative to `dir` with `\\`, each file
/// compressed when that makes it smaller. Files are added in path order with
/// no timestamps, so packing the same files twice gives the same bytes.
/// Hidden files and directories (names starting with `.`) are skipped.
fn pack(dir: &Path, file: &Path) -> Result<(), Failure> {
    if !dir.is_dir() {
        return Err(Failure::Error(format!(
            "{}: not a directory",
            dir.display()
        )));
    }
    let mut paths = Vec::new();
    collect_files(dir, dir, &mut paths)?;
    paths.sort();
    let name = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("data");
    let mut builder = PackageBuilder::new(name);
    let mut total = 0u64;
    for relative in &paths {
        let data = fs::read(dir.join(relative))
            .map_err(|err| Failure::Error(format!("{}: {err}", dir.join(relative).display())))?;
        total += data.len() as u64;
        let name = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\\");
        builder.add_file(name, data);
    }
    let out =
        File::create(file).map_err(|err| Failure::Error(format!("{}: {err}", file.display())))?;
    let mut out = BufWriter::new(out);
    builder.write(&mut out)?;
    out.into_inner()
        .map_err(|err| err.into_error())?
        .sync_all()?;
    eprintln!(
        "Packed {} files, {} bytes, into {}",
        paths.len(),
        total,
        file.display()
    );
    Ok(())
}

/// Collects the files under `dir`, as paths relative to `root`.
fn collect_files(root: &Path, dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), Failure> {
    for entry in
        fs::read_dir(dir).map_err(|err| Failure::Error(format!("{}: {err}", dir.display())))?
    {
        let entry = entry?;
        let path = entry.path();
        let hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, paths)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("under the root")
                .to_path_buf();
            paths.push(relative);
        }
    }
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

fn script(game: &Path, name: &str) -> Result<(), Failure> {
    let vfs = open(game)?;
    let bytes = vfs.read(name)?;
    let program = Program::parse(bytes).map_err(|err| Failure::Error(format!("{name}: {err}")))?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    writeln!(
        out,
        "{}: version {}, {} bytes",
        program.filename(),
        program.version(),
        program.bytes().len()
    )?;
    writeln!(
        out,
        "  code at {}..{} ({} bytes)",
        program.code_start(),
        program.code_end(),
        program.code_end() - program.code_start()
    )?;
    writeln!(
        out,
        "  {} symbol(s), {} function(s), {} event(s), {} method(s), {} external(s)",
        program.symbols().len(),
        program.functions().len(),
        program.events().len(),
        program.methods().len(),
        program.externals().len()
    )?;
    for (kind, entries) in [
        ("function", program.functions()),
        ("event", program.events()),
        ("method", program.methods()),
    ] {
        for entry in entries {
            writeln!(out, "  {kind} {} at {}", entry.name, entry.position)?;
        }
    }
    for external in program.externals() {
        writeln!(
            out,
            "  external {}!{} {:?} -> {:?} ({} parameter(s))",
            external.dll,
            external.name,
            external.call_type,
            external.returns,
            external.params.len()
        )?;
    }
    writeln!(out)?;
    let listing = program
        .listing()
        .map_err(|err| Failure::Error(format!("{name}: {err}")))?;
    out.write_all(listing.as_bytes())?;
    out.flush()?;
    Ok(())
}

/// Builds the extraction path of an entry, refusing names that would land
/// outside `dir`.
fn scene(game: &Path, name: &str) -> Result<(), Failure> {
    let vfs = open(game)?;
    let bytes = vfs.read(name)?;
    let scene = Scene::parse(&bytes).map_err(|err| Failure::Error(format!("{name}: {err}")))?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    writeln!(
        out,
        "{name}: {:?} {}x{}{}",
        scene.name,
        scene.width(),
        scene.height(),
        if scene.main_layer().is_none() {
            " (no main layer)"
        } else {
            ""
        }
    )?;
    writeln!(
        out,
        "  {} script(s), {} waypoint group(s), {} scale level(s), {} rotation level(s){}",
        scene.scripts.len(),
        scene.waypoint_groups.len(),
        scene.scale_levels.len(),
        scene.rotation_levels.len(),
        if scene.persistent_state {
            ", persistent state"
        } else {
            ""
        }
    )?;
    for (i, layer) in scene.layers.iter().enumerate() {
        let mut flags = Vec::new();
        for (set, label) in [
            (layer.main, "main"),
            (!layer.active, "inactive"),
            (layer.close_up, "close-up"),
        ] {
            if set {
                flags.push(label);
            }
        }
        writeln!(
            out,
            "  layer {i}: {:?} {}x{} {}, {} entities, {} regions",
            layer.name,
            layer.width,
            layer.height,
            if flags.is_empty() {
                "-".to_string()
            } else {
                flags.join(" ")
            },
            layer.entities().count(),
            layer.regions().count()
        )?;
        for node in &layer.nodes {
            match node {
                SceneNode::Entity(entity) => describe_entity(&mut out, &vfs, "    ", entity)?,
                SceneNode::Region(region) => {
                    let mut flags = Vec::new();
                    for (set, label) in [
                        (region.blocked, "blocked"),
                        (region.decoration, "decoration"),
                        (!region.active, "inactive"),
                    ] {
                        if set {
                            flags.push(label);
                        }
                    }
                    if let Some(zoom) = region.zoom {
                        flags.push(if zoom == 100.0 {
                            "scale 100%"
                        } else {
                            "scaled"
                        });
                    }
                    writeln!(
                        out,
                        "    region {:?}: {} points{}{}",
                        region.name,
                        region.points.len(),
                        if flags.is_empty() {
                            String::new()
                        } else {
                            format!(" {}", flags.join(" "))
                        },
                        region
                            .zoom
                            .filter(|z| *z != 100.0)
                            .map(|z| format!(" {z}%"))
                            .unwrap_or_default()
                    )?;
                }
            }
        }
    }
    if !scene.free_entities.is_empty() {
        writeln!(out, "  free entities: {}", scene.free_entities.len())?;
        for entity in &scene.free_entities {
            describe_entity(&mut out, &vfs, "    ", entity)?;
        }
    }
    let paths = scene.sprite_paths();
    let missing: Vec<&str> = paths.iter().copied().filter(|p| !vfs.exists(p)).collect();
    writeln!(
        out,
        "  sprites: {} referenced, {} missing",
        paths.len(),
        missing.len()
    )?;
    for path in missing {
        writeln!(out, "    missing: {path}")?;
    }
    Ok(())
}

fn describe_entity(
    out: &mut impl Write,
    vfs: &Vfs,
    indent: &str,
    entity: &luma_formats::Entity,
) -> Result<(), Failure> {
    let mut flags = Vec::new();
    for (set, label) in [
        (!entity.active, "inactive"),
        (entity.kind == luma_formats::EntityKind::Sound, "sound"),
        (entity.scalable, "scalable"),
        (entity.interactive, "interactive"),
        (entity.colorable, "colorable"),
        (entity.rotatable, "rotatable"),
        (entity.region.is_some(), "hotspot"),
    ] {
        if set {
            flags.push(label);
        }
    }
    let sprite = match &entity.sprite {
        Some(path) if vfs.exists(path) => format!(" sprite {path}"),
        Some(path) => format!(" sprite {path} (MISSING)"),
        None => String::new(),
    };
    writeln!(
        out,
        "{indent}entity {:?} at {},{}{} [{}]{}",
        entity.name,
        entity.x,
        entity.y,
        sprite,
        flags.join(" "),
        entity
            .cursor
            .as_deref()
            .map(|c| format!(" cursor {c}"))
            .unwrap_or_default()
    )?;
    Ok(())
}

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
